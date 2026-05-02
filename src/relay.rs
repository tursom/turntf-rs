use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use std::time::Duration;

use prost::Message as ProstMessage;
use tokio::sync::broadcast;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio_stream::wrappers::BroadcastStream;

use crate::client::Client;
use crate::proto;
use crate::types::{
    DeliveryMode, Packet, RelayConfig, RelayEnvelope, RelayError, RelayKind, RelayState,
    Reliability, SessionRef, UserRef,
    RELAY_ERROR_CLIENT_CLOSED, RELAY_ERROR_DUPLICATE_OPEN, RELAY_ERROR_MAX_RETRANSMIT,
    RELAY_ERROR_NOT_CONNECTED, RELAY_ERROR_OPEN_TIMEOUT, RELAY_ERROR_PROTOCOL,
    RELAY_ERROR_REMOTE_CLOSE, RELAY_ERROR_SEND_TIMEOUT, RELAY_ERROR_RECEIVE_TIMEOUT,
};

// ===== Helper functions =====

fn new_relay_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}{:016x}", now.as_nanos(), count)
}

fn relay_kind_to_proto(kind: RelayKind) -> i32 {
    match kind {
        RelayKind::Open => proto::RelayKind::RelayKindOpen as i32,
        RelayKind::OpenAck => proto::RelayKind::RelayKindOpenAck as i32,
        RelayKind::Data => proto::RelayKind::RelayKindData as i32,
        RelayKind::Ack => proto::RelayKind::RelayKindAck as i32,
        RelayKind::Close => proto::RelayKind::RelayKindClose as i32,
        RelayKind::Ping => proto::RelayKind::RelayKindPing as i32,
        RelayKind::Error => proto::RelayKind::RelayKindError as i32,
        _ => proto::RelayKind::RelayKindUnspecified as i32,
    }
}

fn relay_kind_from_proto(kind: i32) -> RelayKind {
    match proto::RelayKind::try_from(kind) {
        Ok(proto::RelayKind::RelayKindOpen) => RelayKind::Open,
        Ok(proto::RelayKind::RelayKindOpenAck) => RelayKind::OpenAck,
        Ok(proto::RelayKind::RelayKindData) => RelayKind::Data,
        Ok(proto::RelayKind::RelayKindAck) => RelayKind::Ack,
        Ok(proto::RelayKind::RelayKindClose) => RelayKind::Close,
        Ok(proto::RelayKind::RelayKindPing) => RelayKind::Ping,
        Ok(proto::RelayKind::RelayKindError) => RelayKind::Error,
        _ => RelayKind::Unspecified,
    }
}

fn encode_relay_envelope(env: &RelayEnvelope) -> Result<Vec<u8>, String> {
    let pb_env = proto::RelayEnvelope {
        relay_id: env.relay_id.clone(),
        kind: relay_kind_to_proto(env.kind),
        sender_session: Some(proto::SessionRef {
            serving_node_id: env.sender_session.serving_node_id,
            session_id: env.sender_session.session_id.clone(),
        }),
        target_session: Some(proto::SessionRef {
            serving_node_id: env.target_session.serving_node_id,
            session_id: env.target_session.session_id.clone(),
        }),
        seq: env.seq,
        ack_seq: env.ack_seq,
        payload: env.payload.clone(),
        sent_at_ms: env.sent_at_ms,
    };
    Ok(pb_env.encode_to_vec())
}

fn decode_relay_envelope(data: &[u8]) -> Result<RelayEnvelope, String> {
    let pb_env =
        proto::RelayEnvelope::decode(data).map_err(|e| format!("decode relay envelope: {e}"))?;
    Ok(RelayEnvelope {
        relay_id: pb_env.relay_id,
        kind: relay_kind_from_proto(pb_env.kind),
        sender_session: pb_env
            .sender_session
            .as_ref()
            .map(|s| SessionRef {
                serving_node_id: s.serving_node_id,
                session_id: s.session_id.clone(),
            })
            .unwrap_or_default(),
        target_session: pb_env
            .target_session
            .as_ref()
            .map(|s| SessionRef {
                serving_node_id: s.serving_node_id,
                session_id: s.session_id.clone(),
            })
            .unwrap_or_default(),
        seq: pb_env.seq,
        ack_seq: pb_env.ack_seq,
        payload: pb_env.payload,
        sent_at_ms: pb_env.sent_at_ms,
    })
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ===== UnackedFrame =====

#[derive(Clone)]
struct UnackedFrame {
    data: Vec<u8>,
    retransmit: u32,
}

// ===== RelayConnection =====

/// 一条 relay 点对点连接，提供可靠或尽力而为的数据传输。
///
/// 通过 [`Relay::connect`] 或 [`Relay::on_connection`] 回调获得。
#[derive(Clone)]
pub struct RelayConnection {
    inner: Arc<RelayConnectionInner>,
}

impl RelayConnection {
    /// 返回连接的唯一标识。
    pub fn relay_id(&self) -> &str {
        &self.inner.relay_id
    }

    /// 返回当前连接状态。
    pub async fn state(&self) -> RelayState {
        *self.inner.state.lock().await
    }

    /// 返回对端用户引用。
    pub fn remote_peer(&self) -> UserRef {
        self.inner.remote_peer.clone()
    }

    /// 返回对端会话引用。
    pub async fn remote_session(&self) -> SessionRef {
        self.inner.remote_session.lock().await.clone()
    }

    /// 发送数据。行为取决于配置的可靠性等级。
    ///
    /// 当发送缓冲区满时，若配置了 `send_timeout_ms` 则等待该时长后返回超时错误。
    pub async fn send(&self, data: Vec<u8>) -> std::result::Result<(), RelayError> {
        if data.is_empty() {
            return Ok(());
        }

        {
            let state = self.inner.state.lock().await;
            if *state != RelayState::Open {
                return Err(RelayError::new(
                    RELAY_ERROR_NOT_CONNECTED,
                    "connection not open",
                ));
            }
        }

        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RelayError::new(
                RELAY_ERROR_CLIENT_CLOSED,
                "connection closed",
            ));
        }

        let send_timeout = self.inner.config.send_timeout_ms;
        if send_timeout > 0 {
            let timeout = Duration::from_millis(send_timeout);
            tokio::select! {
                result = self.inner.send_ch.send(data) => {
                    result.map_err(|_| RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed"))
                }
                _ = tokio::time::sleep(timeout) => {
                    Err(RelayError::new(RELAY_ERROR_SEND_TIMEOUT, "send timeout waiting for buffer space"))
                }
                _ = self.inner.close_notify.notified() => {
                    Err(RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed"))
                }
            }
        } else {
            tokio::select! {
                result = self.inner.send_ch.send(data) => {
                    result.map_err(|_| RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed"))
                }
                _ = self.inner.close_notify.notified() => {
                    Err(RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed"))
                }
            }
        }
    }

    /// 返回接收广播通道。可以通过订阅此通道接收对端发送的数据。
    pub fn receive(&self) -> broadcast::Receiver<Vec<u8>> {
        self.inner.recv_tx.subscribe()
    }

    /// 从连接读取数据，支持超时。
    ///
    /// 当 `timeout_ms` 为 `Some(t)` 且 `t > 0` 时使用指定的超时；
    /// 当 `timeout_ms` 为 `None` 时使用配置的 `receive_timeout_ms`。
    /// 超时值为 0 时无限等待。
    pub async fn receive_timeout(
        &self,
        timeout_ms: Option<u64>,
    ) -> std::result::Result<Vec<u8>, RelayError> {
        let t = timeout_ms.unwrap_or(self.inner.config.receive_timeout_ms);
        let mut rx = self.inner.recv_tx.subscribe();

        if t > 0 {
            let timeout = Duration::from_millis(t);
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(data) => Ok(data),
                        Err(_) => Err(RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed")),
                    }
                }
                _ = tokio::time::sleep(timeout) => {
                    Err(RelayError::new(RELAY_ERROR_RECEIVE_TIMEOUT, "receive timeout"))
                }
                _ = self.inner.close_notify.notified() => {
                    Err(RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed"))
                }
            }
        } else {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(data) => Ok(data),
                        Err(_) => Err(RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed")),
                    }
                }
                _ = self.inner.close_notify.notified() => {
                    Err(RelayError::new(RELAY_ERROR_CLIENT_CLOSED, "connection closed"))
                }
            }
        }
    }

    /// 注册连接关闭回调。
    pub async fn on_close<F>(&self, callback: F)
    where
        F: Fn(Option<RelayError>) + Send + 'static,
    {
        let mut callbacks = self.inner.on_close.lock().await;
        callbacks.push(Box::new(callback));
    }

    /// 优雅关闭连接，发送 CLOSE 帧。
    pub async fn close(&self) {
        {
            let mut state = self.inner.state.lock().await;
            if *state != RelayState::Open {
                return;
            }
            *state = RelayState::Closing;
        }

        let close_env = RelayEnvelope {
            relay_id: self.inner.relay_id.clone(),
            kind: RelayKind::Close,
            sender_session: self.inner.my_session.clone(),
            target_session: self.inner.remote_session.lock().await.clone(),
            seq: 0,
            ack_seq: 0,
            payload: Vec::new(),
            sent_at_ms: chrono_now_ms(),
        };
        let _ = self.inner.send_relay_envelope(&close_env).await;

        self.inner.handle_close(None).await;
    }

    /// 强制关闭连接，不发送 CLOSE 帧。
    pub async fn abort(&self, reason: RelayError) {
        self.inner.handle_close(Some(reason)).await;
    }

    /// 创建一个用于异步流接收的包装对象。
    pub fn into_stream(self) -> RelayRecvStream {
        RelayRecvStream {
            inner: BroadcastStream::new(self.inner.recv_tx.subscribe()),
        }
    }
}

// ===== RelayRecvStream =====

/// 接收数据的异步流，实现了 `Stream` trait。
///
/// 通过 [`RelayConnection::into_stream`] 创建。
pub struct RelayRecvStream {
    inner: BroadcastStream<Vec<u8>>,
}

impl futures_util::Stream for RelayRecvStream {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(item))) => return Poll::Ready(Some(item)),
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ===== RelayConnectionInner =====

struct RelayConnectionInner {
    // ---- 不可变字段 ----
    relay_id: String,
    config: RelayConfig,
    remote_peer: UserRef,
    my_session: SessionRef,
    client: Client,
    relay: Weak<RelayInner>,

    // ---- 可变状态 ----
    state: Mutex<RelayState>,
    remote_session: Mutex<SessionRef>,
    send_base: Mutex<u64>,
    next_seq: Mutex<u64>,
    expected_seq: Mutex<u64>,
    retrans_cnt: Mutex<u32>,
    unacked: Mutex<HashMap<u64, UnackedFrame>>,
    recv_buf: Mutex<HashMap<u64, Vec<u8>>>,

    // ---- 通道 ----
    /// 发送端（用户调用 send() 时使用）
    send_ch: tokio::sync::mpsc::Sender<Vec<u8>>,
    /// 接收端（将数据广播给所有 subscribe 者）
    recv_tx: broadcast::Sender<Vec<u8>>,

    // ---- 通知 ----
    close_notify: Notify,
    open_notify: Notify,

    // ---- 标记 ----
    closed: AtomicBool,
    closing: AtomicBool,

    // ---- 回调 ----
    on_close: tokio::sync::Mutex<Vec<Box<dyn Fn(Option<RelayError>) + Send>>>,
}

impl RelayConnectionInner {
    async fn send_relay_envelope(&self, env: &RelayEnvelope) -> std::result::Result<(), RelayError> {
        let body =
            encode_relay_envelope(env).map_err(|e| RelayError::new(RELAY_ERROR_PROTOCOL, e))?;

        let delivery_mode = self.config.delivery_mode;
        let mode = if matches!(delivery_mode, DeliveryMode::Unspecified) {
            DeliveryMode::RouteRetry
        } else {
            delivery_mode
        };

        self.client
            .send_packet_to_session(
                self.remote_peer.clone(),
                body,
                mode,
                self.remote_session.lock().await.clone(),
            )
            .await
            .map_err(|e| RelayError::new(RELAY_ERROR_PROTOCOL, e.to_string()))?;

        Ok(())
    }

    async fn handle_envelope(&self, env: &RelayEnvelope) {
        match env.kind {
            RelayKind::Data => self.handle_data(env).await,
            RelayKind::Ack => self.handle_ack(env).await,
            RelayKind::Ping => self.handle_ping(env).await,
            _ => {}
        }
    }

    async fn handle_data(&self, env: &RelayEnvelope) {
        match self.config.reliability {
            Reliability::BestEffort => {
                let _ = self.recv_tx.send(env.payload.clone());
            }

            Reliability::AtLeastOnce => {
                let ack_env = RelayEnvelope {
                    relay_id: self.relay_id.clone(),
                    kind: RelayKind::Ack,
                    sender_session: self.my_session.clone(),
                    target_session: self.remote_session.lock().await.clone(),
                    seq: 0,
                    ack_seq: env.seq,
                    payload: Vec::new(),
                    sent_at_ms: chrono_now_ms(),
                };
                let _ = self.send_relay_envelope(&ack_env).await;
                let _ = self.recv_tx.send(env.payload.clone());
            }

            Reliability::ReliableOrdered => {
                let mut expected = self.expected_seq.lock().await;
                if env.seq == *expected {
                    let _ = self.recv_tx.send(env.payload.clone());
                    *expected += 1;
                    // 交付所有可连续交付的缓冲帧
                    let mut buf = self.recv_buf.lock().await;
                    loop {
                        if let Some(data) = buf.remove(expected) {
                            let _ = self.recv_tx.send(data);
                            *expected += 1;
                        } else {
                            break;
                        }
                    }
                } else if env.seq > *expected {
                    let window = self.config.window_size;
                    if env.seq - *expected < window {
                        self.recv_buf
                            .lock()
                            .await
                            .insert(env.seq, env.payload.clone());
                    }
                }

                let ack_env = RelayEnvelope {
                    relay_id: self.relay_id.clone(),
                    kind: RelayKind::Ack,
                    sender_session: self.my_session.clone(),
                    target_session: self.remote_session.lock().await.clone(),
                    seq: 0,
                    ack_seq: env.seq,
                    payload: Vec::new(),
                    sent_at_ms: chrono_now_ms(),
                };
                let _ = self.send_relay_envelope(&ack_env).await;
            }
        }
    }

    async fn handle_ack(&self, env: &RelayEnvelope) {
        if self.config.reliability == Reliability::BestEffort {
            return;
        }

        let mut send_base = self.send_base.lock().await;
        if env.ack_seq >= *send_base {
            let mut unacked = self.unacked.lock().await;
            for seq in *send_base..=env.ack_seq {
                unacked.remove(&seq);
            }
            *send_base = env.ack_seq + 1;
            *self.retrans_cnt.lock().await = 0;
        }
    }

    async fn handle_ping(&self, env: &RelayEnvelope) {
        let err_env = RelayEnvelope {
            relay_id: self.relay_id.clone(),
            kind: RelayKind::Error,
            sender_session: self.my_session.clone(),
            target_session: self.remote_session.lock().await.clone(),
            seq: 0,
            ack_seq: 0,
            payload: Vec::new(),
            sent_at_ms: chrono_now_ms(),
        };
        let _ = self.send_relay_envelope(&err_env).await;
    }

    async fn handle_close(&self, reason: Option<RelayError>) {
        {
            let mut state = self.state.lock().await;
            if *state == RelayState::Closed {
                return;
            }
            *state = RelayState::Closed;
        }

        self.closed.store(true, Ordering::Release);
        self.close_notify.notify_waiters();

        // 执行关闭回调
        let callbacks = {
            let mut cbs = self.on_close.lock().await;
            std::mem::take(&mut *cbs)
        };
        for cb in callbacks {
            cb(reason.clone());
        }

        // 从 relay 管理器中移除
        if let Some(relay) = self.relay.upgrade() {
            relay.remove_connection(&self.relay_id).await;
        }
    }
}

// ===== Send loops =====

/// 启动发送循环。接收端 rx 将会被消费。
async fn spawn_send_loop(
    inner: Arc<RelayConnectionInner>,
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
) {
    tokio::spawn(async move {
        if inner.config.reliability == Reliability::BestEffort {
            send_loop_best_effort(inner, rx).await;
        } else {
            send_loop_reliable(inner, rx).await;
        }
    });
}

async fn send_loop_best_effort(
    inner: Arc<RelayConnectionInner>,
    mut rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
) {
    loop {
        if inner.closed.load(Ordering::Acquire) {
            return;
        }

        tokio::select! {
            data = rx.recv() => {
                let data = match data {
                    Some(d) => d,
                    None => return,
                };

                let env = RelayEnvelope {
                    relay_id: inner.relay_id.clone(),
                    kind: RelayKind::Data,
                    sender_session: inner.my_session.clone(),
                    target_session: inner.remote_session.lock().await.clone(),
                    seq: 0,
                    ack_seq: 0,
                    payload: data,
                    sent_at_ms: chrono_now_ms(),
                };
                if let Err(err) = inner.send_relay_envelope(&env).await {
                    inner.handle_close(Some(err)).await;
                    return;
                }
            }
            _ = inner.close_notify.notified() => {
                return;
            }
        }
    }
}

async fn send_loop_reliable(
    inner: Arc<RelayConnectionInner>,
    mut rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
) {
    let ack_timeout = Duration::from_millis(inner.config.ack_timeout_ms);
    let mut ack_interval = tokio::time::interval(ack_timeout);
    ack_interval.tick().await; // 跳过第一次立即触发的 tick

    loop {
        if inner.closed.load(Ordering::Acquire) {
            return;
        }

        // 窗口控制：等待窗口空闲
        loop {
            let send_base = *inner.send_base.lock().await;
            let next_seq = *inner.next_seq.lock().await;
            if next_seq - send_base < inner.config.window_size {
                break;
            }
            // 窗口满，等待并可能重传
            tokio::select! {
                _ = inner.close_notify.notified() => {
                    if inner.closed.load(Ordering::Acquire) {
                        return;
                    }
                }
                _ = ack_interval.tick() => {
                    retransmit(&inner).await;
                }
            }
        }

        tokio::select! {
            data = rx.recv() => {
                let data = match data {
                    Some(d) => d,
                    None => {
                        inner.handle_close(Some(RelayError::new(
                            RELAY_ERROR_CLIENT_CLOSED,
                            "send channel closed",
                        ))).await;
                        return;
                    }
                };

                if inner.closed.load(Ordering::Acquire) {
                    return;
                }

                // 分配序列号
                let seq = {
                    let mut next_seq = inner.next_seq.lock().await;
                    let seq = *next_seq;
                    *next_seq += 1;
                    seq
                };

                {
                    let mut unacked = inner.unacked.lock().await;
                    unacked.insert(seq, UnackedFrame { data: data.clone(), retransmit: 0 });
                }

                let env = RelayEnvelope {
                    relay_id: inner.relay_id.clone(),
                    kind: RelayKind::Data,
                    sender_session: inner.my_session.clone(),
                    target_session: inner.remote_session.lock().await.clone(),
                    seq,
                    ack_seq: 0,
                    payload: data,
                    sent_at_ms: chrono_now_ms(),
                };
                if let Err(err) = inner.send_relay_envelope(&env).await {
                    inner.handle_close(Some(err)).await;
                    return;
                }
            }
            _ = inner.close_notify.notified() => {
                return;
            }
            _ = ack_interval.tick() => {
                retransmit(&inner).await;
            }
        }
    }
}

async fn retransmit(inner: &RelayConnectionInner) {
    {
        let mut cnt = inner.retrans_cnt.lock().await;
        *cnt += 1;
        if *cnt > inner.config.max_retransmits {
            inner
                .handle_close(Some(RelayError::new(
                    RELAY_ERROR_MAX_RETRANSMIT,
                    "max retransmits exceeded",
                )))
                .await;
            return;
        }
    }

    let seqs: Vec<u64> = {
        let unacked = inner.unacked.lock().await;
        unacked.keys().copied().collect()
    };

    for seq in seqs {
        let frame = {
            let mut unacked = inner.unacked.lock().await;
            unacked.get_mut(&seq).map(|f| {
                f.retransmit += 1;
                f.clone()
            })
        };

        if let Some(frame) = frame {
            let env = RelayEnvelope {
                relay_id: inner.relay_id.clone(),
                kind: RelayKind::Data,
                sender_session: inner.my_session.clone(),
                target_session: inner.remote_session.lock().await.clone(),
                seq,
                ack_seq: 0,
                payload: frame.data,
                sent_at_ms: chrono_now_ms(),
            };
            let _ = inner.send_relay_envelope(&env).await;
        }
    }
}

// ===== Relay =====

struct RelayInner {
    client: Client,
    conns: tokio::sync::Mutex<HashMap<String, RelayConnection>>,
    on_conn: tokio::sync::RwLock<Option<Box<dyn Fn(RelayConnection) + Send + Sync>>>,
}

/// Relay 管理器，基于 Client 管理 relay 连接。
///
/// 负责入站连接分发和出站连接创建。
///
/// # 示例
///
/// ```ignore
/// use turntf::Relay;
///
/// let relay = Relay::new(client.clone());
/// relay.on_connection(|conn| {
///     println!("new incoming relay connection: {}", conn.relay_id());
/// }).await;
///
/// let conn = relay.connect(target_user, None).await?;
/// conn.send(b"hello".to_vec()).await?;
/// ```
#[derive(Clone)]
pub struct Relay {
    inner: Arc<RelayInner>,
}

impl Relay {
    /// 新建 Relay 管理器。
    pub fn new(client: Client) -> Self {
        Self {
            inner: Arc::new(RelayInner {
                client: client.clone(),
                conns: tokio::sync::Mutex::new(HashMap::new()),
                on_conn: tokio::sync::RwLock::new(None),
            }),
        }
    }

    /// 启动 relay 后台任务，订阅客户端数据包事件以自动处理入站 relay 帧。
    ///
    /// 在调用此方法之前，入站 relay 帧不会被处理。
    pub async fn start(&self) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Packet>();

        // 在客户端注册 relay 包通道
        self.inner.client.set_relay_packet_tx(Some(tx)).await;

        // 后台任务：处理从客户端转发来的数据包
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            while let Some(packet) = rx.recv().await {
                let inner = match weak.upgrade() {
                    Some(i) => i,
                    None => return,
                };
                Self::handle_packet_async(&inner, packet).await;
            }
        });
    }

    /// 注册入站 relay 连接的处理器。每个新入站连接会调用 handler。
    pub async fn on_connection<F>(&self, handler: F)
    where
        F: Fn(RelayConnection) + Send + Sync + 'static,
    {
        *self.inner.on_conn.write().await = Some(Box::new(handler));
    }

    /// 向目标用户发起 relay 连接。
    ///
    /// 自动解析目标用户的在线会话并选择支持瞬时消息的会话。
    /// `config` 为 `None` 时使用 `RelayConfig::default()`。
    pub async fn connect(
        &self,
        target: UserRef,
        config: Option<RelayConfig>,
    ) -> std::result::Result<RelayConnection, RelayError> {
        // 1. 解析目标用户会话
        let sessions = self
            .inner
            .client
            .resolve_user_sessions(target.clone())
            .await
            .map_err(|e| {
                RelayError::new(RELAY_ERROR_NOT_CONNECTED, format!("resolve sessions: {e}"))
            })?;

        // 2. 选择支持瞬时消息的会话
        let target_session = sessions
            .sessions
            .iter()
            .find(|s| s.transient_capable)
            .map(|s| s.session.clone())
            .ok_or_else(|| {
                RelayError::new(
                    RELAY_ERROR_NOT_CONNECTED,
                    "no transient-capable session found for target user",
                )
            })?;

        let cfg = config.unwrap_or_default();

        // 3. 获取当前登录的会话引用
        let my_session = self.inner.client.session_ref().await.ok_or_else(|| {
            RelayError::new(RELAY_ERROR_NOT_CONNECTED, "client not logged in")
        })?;

        let relay_id = new_relay_id();

        // 创建通道
        let send_buffer = (cfg.send_buffer_size / 1024).max(1);
        let (send_tx, send_rx) = tokio::sync::mpsc::channel(send_buffer);
        let (recv_tx, _) = broadcast::channel(64);

        let conn_inner = Arc::new(RelayConnectionInner {
            relay_id: relay_id.clone(),
            config: cfg.clone(),
            remote_peer: target,
            remote_session: Mutex::new(target_session.clone()),
            my_session: my_session.clone(),
            client: self.inner.client.clone(),
            relay: Arc::downgrade(&self.inner),
            state: Mutex::new(RelayState::Opening),
            send_base: Mutex::new(1),
            next_seq: Mutex::new(1),
            expected_seq: Mutex::new(1),
            retrans_cnt: Mutex::new(0),
            unacked: Mutex::new(HashMap::new()),
            recv_buf: Mutex::new(HashMap::new()),
            send_ch: send_tx,
            recv_tx,
            close_notify: Notify::new(),
            open_notify: Notify::new(),
            closed: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            on_close: tokio::sync::Mutex::new(Vec::new()),
        });

        let conn = RelayConnection {
            inner: Arc::clone(&conn_inner),
        };

        // 4. 存储连接
        {
            let mut conns = self.inner.conns.lock().await;
            conns.insert(relay_id.clone(), conn.clone());
        }

        // 5. 发送 OPEN 帧
        let open_env = RelayEnvelope {
            relay_id: relay_id.clone(),
            kind: RelayKind::Open,
            sender_session: my_session,
            target_session: target_session.clone(),
            seq: 0,
            ack_seq: 0,
            payload: Vec::new(),
            sent_at_ms: chrono_now_ms(),
        };

        if let Err(err) = conn_inner.send_relay_envelope(&open_env).await {
            self.inner.conns.lock().await.remove(&relay_id);
            return Err(err);
        }

        // 6. 启动发送循环
        spawn_send_loop(conn_inner.clone(), send_rx).await;

        // 7. 等待 OPEN_ACK
        let open_timeout = Duration::from_millis(cfg.open_timeout_ms);
        tokio::select! {
            _ = conn_inner.open_notify.notified() => {
                Ok(conn)
            }
            _ = tokio::time::sleep(open_timeout) => {
                conn_inner.handle_close(Some(RelayError::new(
                    RELAY_ERROR_OPEN_TIMEOUT,
                    "OPEN timeout waiting for OPEN_ACK",
                ))).await;
                Err(RelayError::new(
                    RELAY_ERROR_OPEN_TIMEOUT,
                    "OPEN timeout waiting for OPEN_ACK",
                ))
            }
        }
    }

    /// 同步处理入站数据包。如果包是 relay 帧则返回 `true`，否则返回 `false`。
    ///
    /// 通常不需要用户直接调用，relay 后台任务会自动处理。
    pub fn handle_packet(&self, packet: &Packet) -> bool {
        let env = match decode_relay_envelope(&packet.body) {
            Ok(env) => env,
            Err(_) => return false,
        };

        // 在非 async 上下文中，spawn 异步处理
        let inner = self.inner.clone();
        tokio::spawn(async move {
            Self::handle_envelope_inner(&inner, env).await;
        });

        true
    }

    /// 获取当前连接数。
    pub async fn connection_count(&self) -> usize {
        self.inner.conns.lock().await.len()
    }

    // ---- 内部方法 ----

    /// 异步处理入站数据包（由后台任务调用）。
    async fn handle_packet_async(inner: &RelayInner, packet: Packet) -> bool {
        let env = match decode_relay_envelope(&packet.body) {
            Ok(env) => env,
            Err(_) => return false,
        };

        Self::handle_envelope_inner(inner, env).await;
        true
    }

    /// 核心分发逻辑。
    async fn handle_envelope_inner(inner: &RelayInner, env: RelayEnvelope) {
        let relay_id = env.relay_id.clone();
        let kind = env.kind;

        // OPEN 帧：入站连接
        if kind == RelayKind::Open {
            accept_incoming(Arc::clone(inner), env).await;
            return;
        }

        // 查找连接
        let conn = {
            let conns = inner.conns.lock().await;
            conns.get(&relay_id).cloned()
        };

        match kind {
            RelayKind::OpenAck => {
                if let Some(conn) = conn {
                    let mut state = conn.inner.state.lock().await;
                    if *state == RelayState::Opening {
                        *state = RelayState::Open;
                        *conn.inner.remote_session.lock().await = env.sender_session;
                        conn.inner.open_notify.notify_waiters();
                    }
                }
            }
            RelayKind::Close => {
                if let Some(conn) = conn {
                    conn.inner
                        .handle_close(Some(RelayError::new(
                            RELAY_ERROR_REMOTE_CLOSE,
                            "remote peer closed connection",
                        )))
                        .await;
                }
            }
            RelayKind::Error => {
                if let Some(conn) = conn {
                    let msg = String::from_utf8_lossy(&env.payload).to_string();
                    conn.inner
                        .handle_close(Some(RelayError::new(RELAY_ERROR_PROTOCOL, msg)))
                        .await;
                }
            }
            _ => {
                if let Some(conn) = conn {
                    conn.inner.handle_envelope(&env).await;
                }
            }
        }
    }
}

impl RelayInner {
    async fn remove_connection(&self, relay_id: &str) {
        self.conns.lock().await.remove(relay_id);
    }
}

/// 接受入站 OPEN 帧，创建新连接。
async fn accept_incoming(inner: Arc<RelayInner>, env: RelayEnvelope) {
    let cfg = RelayConfig::default();

    let relay_id = env.relay_id.clone();

    let send_buffer = (cfg.send_buffer_size / 1024).max(1);
    let (send_tx, send_rx) = tokio::sync::mpsc::channel(send_buffer);
    let (recv_tx, _) = broadcast::channel(64);

    let conn_inner = Arc::new(RelayConnectionInner {
        relay_id: relay_id.clone(),
        config: cfg.clone(),
        remote_peer: UserRef::default(),
        remote_session: Mutex::new(env.sender_session.clone()),
        my_session: env.target_session.clone(),
        client: inner.client.clone(),
        relay: Arc::downgrade(&inner),
        state: Mutex::new(RelayState::Open),
        send_base: Mutex::new(1),
        next_seq: Mutex::new(1),
        expected_seq: Mutex::new(1),
        retrans_cnt: Mutex::new(0),
        unacked: Mutex::new(HashMap::new()),
        recv_buf: Mutex::new(HashMap::new()),
        send_ch: send_tx,
        recv_tx,
        close_notify: Notify::new(),
        open_notify: Notify::new(),
        closed: AtomicBool::new(false),
        closing: AtomicBool::new(false),
        on_close: tokio::sync::Mutex::new(Vec::new()),
    });

    // 立即通知 open 已完成
    conn_inner.open_notify.notify_waiters();

    let conn = RelayConnection {
        inner: Arc::clone(&conn_inner),
    };

    // 检查并发 OPEN：relay_id 字典序小的保留
    {
        let mut conns = inner.conns.lock().await;
        if let Some(existing) = conns.get(&relay_id) {
            if env.relay_id < existing.inner.relay_id {
                // 当前 relay_id 更小，关闭现有连接
                let old = existing.clone();
                tokio::spawn(async move {
                    old.inner
                        .handle_close(Some(RelayError::new(
                            RELAY_ERROR_DUPLICATE_OPEN,
                            "concurrent OPEN, keeping lower relay_id",
                        )))
                        .await;
                });
                conns.insert(relay_id.clone(), conn.clone());
            } else {
                // 当前 relay_id 更大，放弃新连接
                tokio::spawn(async move {
                    conn_inner
                        .handle_close(Some(RelayError::new(
                            RELAY_ERROR_DUPLICATE_OPEN,
                            "concurrent OPEN, keeping lower relay_id",
                        )))
                        .await;
                });
                return;
            }
        } else {
            conns.insert(relay_id.clone(), conn.clone());
        }
    }

    // 启动发送循环
    spawn_send_loop(conn_inner.clone(), send_rx).await;

    // 发送 OPEN_ACK
    let open_ack_env = RelayEnvelope {
        relay_id: relay_id.clone(),
        kind: RelayKind::OpenAck,
        sender_session: conn_inner.my_session.clone(),
        target_session: conn_inner.remote_session.lock().await.clone(),
        seq: 0,
        ack_seq: 0,
        payload: Vec::new(),
        sent_at_ms: chrono_now_ms(),
    };
    let _ = conn_inner.send_relay_envelope(&open_ack_env).await;

    // 调用用户处理器
    let handler = inner.on_conn.read().await;
    if let Some(ref handler) = *handler {
        handler(conn);
    }
}

// ===== Drop 实现 =====

impl Drop for RelayConnectionInner {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.close_notify.notify_waiters();
    }
}
