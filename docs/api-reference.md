# turntf-rs API 参考

本文档是 `turntf-rs` 公开 API 的快速参考，按模块组织。完整的使用语义和约束请参考 [guide.md](guide.md)。

## 模块：`turntf`（lib.rs 公开导出）

所有公开类型统一从 `turntf::` 路径导出：

```rust
use turntf::{
    // Client
    Client, ClientEvent, ClientSubscription, Config,
    // HttpClient
    HttpClient,
    // 错误类型
    Error, Result, ClosedError, NotConnectedError, DisconnectedError,
    ConnectionError, ProtocolError, ServerError, StoreError, ValidationError,
    // 密码
    PasswordInput, PasswordSource, hash_password, hashed_password, plain_password,
    // 存储
    CursorStore, MemoryCursorStore, BoxError,
    // 类型
    UserRef, SessionRef, MessageCursor, DeliveryMode, AttachmentType,
    User, UserMetadata, UserMetadataScanResult,
    Message, Packet, RelayAccepted,
    Attachment, Subscription, BlacklistEntry, Event,
    ClusterNode, LoggedInUser, OnlineNodePresence, ResolvedSession, ResolvedUserSessions,
    Credentials, LoginInfo, CreateUserRequest, UpdateUserRequest, ListUsersRequest,
    UpsertUserMetadataRequest, ScanUserMetadataRequest,
    DeleteUserResult, OperationsStatus,
};
```

---

## `Client` — 长连接实时客户端

### 构造与生命周期

```rust
// 创建客户端实例，立即进行本地参数校验
pub fn new(config: Config) -> Result<Self>

// 启动后台连接任务，等待进入 connected 状态
pub async fn connect(&self) -> Result<()>

// 关闭连接，清理所有 pending RPC，终止后台任务
pub async fn close(&self) -> Result<()>
```

### 事件流

```rust
// 获取事件流订阅
pub async fn subscribe(&self) -> ClientSubscription

// ClientSubscription 实现 Stream<Item = Result<ClientEvent, BroadcastStreamRecvError>>
// 可通过 StreamExt 的 next() 方法消费
```

### 用户管理（长连接 RPC）

```rust
// 创建用户
pub async fn create_user(&self, request: CreateUserRequest) -> Result<User>

// 创建频道（role 默认设为 "channel"）
pub async fn create_channel(&self, request: CreateUserRequest) -> Result<User>

// 获取用户信息
pub async fn get_user(&self, target: UserRef) -> Result<User>

// 更新用户信息
pub async fn update_user(&self, target: UserRef, request: UpdateUserRequest) -> Result<User>

// 删除用户
pub async fn delete_user(&self, target: UserRef) -> Result<DeleteUserResult>

// 查询当前连接用户可通讯的活跃用户列表
pub async fn list_users(&self, request: ListUsersRequest) -> Result<Vec<User>>
```

### 消息与瞬时包

```rust
// 发送持久化消息（落库）
pub async fn send_message(&self, target: UserRef, body: Vec<u8>) -> Result<Message>

// send_message 的别名
pub async fn post_message(&self, target: UserRef, body: Vec<u8>) -> Result<Message>

// 发送瞬时包（不落库）
pub async fn send_packet(
    &self,
    target: UserRef,
    body: Vec<u8>,
    delivery_mode: DeliveryMode,
) -> Result<RelayAccepted>

// 发送定向瞬时包到指定会话
pub async fn send_packet_to_session(
    &self,
    target: UserRef,
    body: Vec<u8>,
    delivery_mode: DeliveryMode,
    target_session: SessionRef,
) -> Result<RelayAccepted>

// send_packet 的别名
pub async fn post_packet(
    &self,
    target: UserRef,
    body: Vec<u8>,
    delivery_mode: DeliveryMode,
) -> Result<RelayAccepted>

// send_packet_to_session 的别名
pub async fn post_packet_to_session(
    &self,
    target: UserRef,
    body: Vec<u8>,
    delivery_mode: DeliveryMode,
    target_session: SessionRef,
) -> Result<RelayAccepted>

// 查询消息历史
pub async fn list_messages(&self, target: UserRef, limit: i32) -> Result<Vec<Message>>

// 查询事件日志
pub async fn list_events(&self, after: i64, limit: i32) -> Result<Vec<Event>>
```

### 用户元数据

```rust
// 获取用户元数据
pub async fn get_user_metadata(
    &self,
    owner: UserRef,
    key: impl Into<String>,
) -> Result<UserMetadata>

// 写入或更新用户元数据
pub async fn upsert_user_metadata(
    &self,
    owner: UserRef,
    key: impl Into<String>,
    request: UpsertUserMetadataRequest,
) -> Result<UserMetadata>

// 删除用户元数据
pub async fn delete_user_metadata(
    &self,
    owner: UserRef,
    key: impl Into<String>,
) -> Result<UserMetadata>

// 扫描用户元数据
pub async fn scan_user_metadata(
    &self,
    owner: UserRef,
    request: ScanUserMetadataRequest,
) -> Result<UserMetadataScanResult>
```

### 频道订阅与黑名单（附件抽象）

```rust
// 订阅频道
pub async fn subscribe_channel(
    &self,
    subscriber: UserRef,
    channel: UserRef,
) -> Result<Subscription>

// subscribe_channel 的别名
pub async fn create_subscription(
    &self,
    subscriber: UserRef,
    channel: UserRef,
) -> Result<Subscription>

// 取消订阅
pub async fn unsubscribe_channel(
    &self,
    subscriber: UserRef,
    channel: UserRef,
) -> Result<Subscription>

// 列出订阅列表
pub async fn list_subscriptions(&self, subscriber: UserRef) -> Result<Vec<Subscription>>

// 拉黑用户
pub async fn block_user(&self, owner: UserRef, blocked: UserRef) -> Result<BlacklistEntry>

// 取消拉黑
pub async fn unblock_user(&self, owner: UserRef, blocked: UserRef) -> Result<BlacklistEntry>

// 列出黑名单
pub async fn list_blocked_users(&self, owner: UserRef) -> Result<Vec<BlacklistEntry>>

// 写入附件（底层接口）
pub async fn upsert_attachment(
    &self,
    owner: UserRef,
    subject: UserRef,
    attachment_type: AttachmentType,
    config_json: Vec<u8>,
) -> Result<Attachment>

// 删除附件
pub async fn delete_attachment(
    &self,
    owner: UserRef,
    subject: UserRef,
    attachment_type: AttachmentType,
) -> Result<Attachment>

// 列出附件
pub async fn list_attachments(
    &self,
    owner: UserRef,
    attachment_type: Option<AttachmentType>,
) -> Result<Vec<Attachment>>
```

### 查询与运维

```rust
// 列出集群节点
pub async fn list_cluster_nodes(&self) -> Result<Vec<ClusterNode>>

// 列出节点上已登录用户
pub async fn list_node_logged_in_users(&self, node_id: i64) -> Result<Vec<LoggedInUser>>

// 解析用户在线会话
pub async fn resolve_user_sessions(&self, user: UserRef) -> Result<ResolvedUserSessions>

// 运维状态
pub async fn operations_status(&self) -> Result<OperationsStatus>

// 指标数据
pub async fn metrics(&self) -> Result<String>

// 心跳检测
pub async fn ping(&self) -> Result<()>
```

### HTTP 桥接

```rust
// 通过 HTTP 登录（底层委托给 HttpClient）
pub async fn login(&self, node_id: i64, user_id: i64, password: impl AsRef<str>) -> Result<String>

// 通过 HTTP 登录（使用已编码的密码）
pub async fn login_with_password(
    &self,
    node_id: i64,
    user_id: i64,
    password: PasswordInput,
) -> Result<String>

// 通过 login_name + 明文密码登录
pub async fn login_by_login_name(
    &self,
    login_name: impl AsRef<str>,
    password: impl AsRef<str>,
) -> Result<String>

// 通过 login_name + 已编码密码登录
pub async fn login_by_login_name_with_password(
    &self,
    login_name: impl AsRef<str>,
    password: PasswordInput,
) -> Result<String>

// 获取内部 HttpClient 的克隆
pub fn http(&self) -> HttpClient
```

---

## `Config` — 客户端配置

```rust
#[derive(Clone)]
pub struct Config {
    pub base_url: String,                              // 服务端 HTTP 基础地址
    pub credentials: Credentials,                      // 旧式登录凭据
    pub login_name: Option<String>,                    // 新式登录名；非空时优先使用
    pub cursor_store: Arc<dyn CursorStore>,            // 游标存储（默认 MemoryCursorStore）
    pub reconnect: bool,                               // 自动重连（默认 true）
    pub initial_reconnect_delay: Duration,             // 首次重连延迟（默认 1s）
    pub max_reconnect_delay: Duration,                 // 重连退避上限（默认 30s）
    pub ping_interval: Duration,                       // 心跳间隔（默认 30s）
    pub request_timeout: Duration,                     // RPC 超时（默认 10s）
    pub ack_messages: bool,                            // 自动 ACK（默认 true）
    pub transient_only: bool,                          // 仅瞬时消息模式（默认 false）
    pub realtime_stream: bool,                         // 使用 /ws/realtime（默认 false）
    pub event_channel_capacity: usize,                 // 事件通道容量（默认 256）
}

impl Config {
    pub fn new(base_url: impl Into<String>, credentials: Credentials) -> Self
    pub fn new_with_login_name(
        base_url: impl Into<String>,
        login_name: impl Into<String>,
        password: PasswordInput,
    ) -> Self
}
```

---

## `ClientEvent` — 事件枚举

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientEvent {
    Login(LoginInfo),     // 登录成功
    Message(Message),     // 收到持久化消息推送
    Packet(Packet),       // 收到瞬时包推送
    Error(Error),         // 内部错误
    Disconnect(Error),    // 连接断开
}
```

---

## `HttpClient` — HTTP JSON 客户端

### 构造

```rust
// 创建客户端实例
pub fn new(base_url: impl Into<String>) -> Result<Self>

// 获取基础地址
pub fn base_url(&self) -> &str
```

### 认证

```rust
// 使用明文密码登录
pub async fn login(
    &self,
    node_id: i64,
    user_id: i64,
    password: impl AsRef<str>,
) -> Result<String>

// 使用已编码密码登录
pub async fn login_with_password(
    &self,
    node_id: i64,
    user_id: i64,
    password: PasswordInput,
) -> Result<String>

// 使用 login_name + 明文密码登录
pub async fn login_by_login_name(
    &self,
    login_name: impl AsRef<str>,
    password: impl AsRef<str>,
) -> Result<String>

// 使用 login_name + 已编码密码登录
pub async fn login_by_login_name_with_password(
    &self,
    login_name: impl AsRef<str>,
    password: PasswordInput,
) -> Result<String>
```

### 用户管理

```rust
// 创建用户
pub async fn create_user(&self, token: &str, request: CreateUserRequest) -> Result<User>

// 创建频道（role 默认设为 "channel"）
pub async fn create_channel(&self, token: &str, request: CreateUserRequest) -> Result<User>

// 获取用户
pub async fn get_user(&self, token: &str, target: UserRef) -> Result<User>

// 更新用户
pub async fn update_user(
    &self,
    token: &str,
    target: UserRef,
    request: UpdateUserRequest,
) -> Result<User>

// 删除用户
pub async fn delete_user(&self, token: &str, target: UserRef) -> Result<DeleteUserResult>

// 查询当前 token 对应用户可通讯的活跃用户列表
pub async fn list_users(
    &self,
    token: &str,
    request: ListUsersRequest,
) -> Result<Vec<User>>
```

### 消息与瞬时包

```rust
// 发送消息
pub async fn post_message(
    &self,
    token: &str,
    target: UserRef,
    body: Vec<u8>,
) -> Result<Message>

// 发送瞬时包
pub async fn post_packet(
    &self,
    token: &str,
    target_node_id: i64,
    relay_target: UserRef,
    body: Vec<u8>,
    mode: DeliveryMode,
) -> Result<RelayAccepted>

// 查询消息列表
pub async fn list_messages(
    &self,
    token: &str,
    target: UserRef,
    limit: i32,
) -> Result<Vec<Message>>
```

### 用户元数据

```rust
pub async fn get_user_metadata(
    &self, token: &str, owner: UserRef, key: impl Into<String>,
) -> Result<UserMetadata>

pub async fn upsert_user_metadata(
    &self, token: &str, owner: UserRef, key: impl Into<String>,
    request: UpsertUserMetadataRequest,
) -> Result<UserMetadata>

pub async fn delete_user_metadata(
    &self, token: &str, owner: UserRef, key: impl Into<String>,
) -> Result<UserMetadata>

pub async fn scan_user_metadata(
    &self, token: &str, owner: UserRef, request: ScanUserMetadataRequest,
) -> Result<UserMetadataScanResult>
```

### 频道订阅与黑名单

```rust
pub async fn create_subscription(
    &self, token: &str, user: UserRef, channel: UserRef,
) -> Result<Subscription>

pub async fn subscribe_channel(
    &self, token: &str, subscriber: UserRef, channel: UserRef,
) -> Result<Subscription>

pub async fn unsubscribe_channel(
    &self, token: &str, subscriber: UserRef, channel: UserRef,
) -> Result<Subscription>

pub async fn list_subscriptions(
    &self, token: &str, subscriber: UserRef,
) -> Result<Vec<Subscription>>

pub async fn block_user(
    &self, token: &str, owner: UserRef, blocked: UserRef,
) -> Result<BlacklistEntry>

pub async fn unblock_user(
    &self, token: &str, owner: UserRef, blocked: UserRef,
) -> Result<BlacklistEntry>

pub async fn list_blocked_users(
    &self, token: &str, owner: UserRef,
) -> Result<Vec<BlacklistEntry>>
```

### 附件

```rust
pub async fn upsert_attachment(
    &self, token: &str, owner: UserRef, subject: UserRef,
    attachment_type: AttachmentType, config_json: Vec<u8>,
) -> Result<Attachment>

pub async fn delete_attachment(
    &self, token: &str, owner: UserRef, subject: UserRef,
    attachment_type: AttachmentType,
) -> Result<Attachment>

pub async fn list_attachments(
    &self, token: &str, owner: UserRef,
    attachment_type: Option<AttachmentType>,
) -> Result<Vec<Attachment>>
```

### 查询与运维

```rust
pub async fn list_events(
    &self, token: &str, after: i64, limit: i32,
) -> Result<Vec<Event>>

pub async fn list_cluster_nodes(&self, token: &str) -> Result<Vec<ClusterNode>>

pub async fn list_node_logged_in_users(
    &self, token: &str, node_id: i64,
) -> Result<Vec<LoggedInUser>>

pub async fn operations_status(&self, token: &str) -> Result<OperationsStatus>

pub async fn metrics(&self, token: &str) -> Result<String>
```

---

## 核心数据类型

### 标识类型

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserRef {
    pub node_id: i64,
    pub user_id: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRef {
    pub serving_node_id: i64,
    pub session_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageCursor {
    pub node_id: i64,
    pub seq: i64,
}

#[derive(Clone, Debug, Default)]
pub struct ListUsersRequest {
    pub name: String,
    pub uid: UserRef,
}
```

### 消息与瞬时包

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub recipient: UserRef,       // 接收者
    pub node_id: i64,             // 消息所在节点
    pub seq: i64,                 // 消息序号
    pub sender: UserRef,          // 发送者
    pub body: Vec<u8>,            // 消息体
    pub created_at_hlc: String,   // HLC 时间戳
}

impl Message {
    pub fn cursor(&self) -> MessageCursor {
        MessageCursor { node_id: self.node_id, seq: self.seq }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packet {
    pub packet_id: u64,
    pub source_node_id: i64,
    pub target_node_id: i64,
    pub recipient: UserRef,
    pub sender: UserRef,
    pub body: Vec<u8>,
    pub delivery_mode: DeliveryMode,
    pub target_session: Option<SessionRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayAccepted {
    pub packet_id: u64,
    pub source_node_id: i64,
    pub target_node_id: i64,
    pub recipient: UserRef,
    pub delivery_mode: DeliveryMode,
    pub target_session: Option<SessionRef>,
}
```

### 用户与凭据

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub node_id: i64,
    pub user_id: i64,
    pub username: String,
    pub login_name: String,                        // 登录名；空串表示未绑定
    pub role: String,                              // "user" / "channel" / "admin" 等
    pub profile_json: Vec<u8>,                     // JSON 格式的用户资料
    pub system_reserved: bool,
    pub created_at: String,
    pub updated_at: String,
    pub origin_node_id: i64,
}

#[derive(Clone, Debug)]
pub struct Credentials {
    pub node_id: i64,
    pub user_id: i64,
    pub password: PasswordInput,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginInfo {
    pub user: User,
    pub protocol_version: String,
    pub session_ref: SessionRef,
}
```

### 用户元数据

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMetadata {
    pub owner: UserRef,
    pub key: String,
    pub value: Vec<u8>,                            // base64 编码存储
    pub updated_at: String,
    pub deleted_at: String,
    pub expires_at: String,
    pub origin_node_id: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMetadataScanResult {
    pub items: Vec<UserMetadata>,
    pub count: i32,
    pub next_after: String,                        // 分页游标
}
```

### 订阅与黑名单

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub subscriber: UserRef,
    pub channel: UserRef,
    pub subscribed_at: String,
    pub deleted_at: String,
    pub origin_node_id: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlacklistEntry {
    pub owner: UserRef,
    pub blocked: UserRef,
    pub blocked_at: String,
    pub deleted_at: String,
    pub origin_node_id: i64,
}
```

### 附件

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub owner: UserRef,
    pub subject: UserRef,
    pub attachment_type: AttachmentType,
    pub config_json: Vec<u8>,
    pub attached_at: String,
    pub deleted_at: String,
    pub origin_node_id: i64,
}
```

### 会话解析

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineNodePresence {
    pub serving_node_id: i64,
    pub session_count: i32,
    pub transport_hint: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSession {
    pub session: SessionRef,
    pub transport: String,
    pub transient_capable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedUserSessions {
    pub user: UserRef,
    pub presence: Vec<OnlineNodePresence>,
    pub sessions: Vec<ResolvedSession>,
}
```

### 集群与运维

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterNode {
    pub node_id: i64,
    pub is_local: bool,
    pub configured_url: String,
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedInUser {
    pub node_id: i64,
    pub user_id: i64,
    pub username: String,
    pub login_name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub sequence: i64,
    pub event_id: i64,
    pub event_type: String,
    pub aggregate: String,
    pub aggregate_node_id: i64,
    pub aggregate_id: i64,
    pub hlc: String,
    pub origin_node_id: i64,
    pub event_json: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationsStatus {
    pub node_id: i64,
    pub message_window_size: i32,
    pub last_event_sequence: i64,
    pub write_gate_ready: bool,
    pub conflict_total: i64,
    pub message_trim: MessageTrimStatus,
    pub projection: ProjectionStatus,
    pub peers: Vec<PeerStatus>,
    pub event_log_trim: Option<EventLogTrimStatus>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteUserResult {
    pub status: String,
    pub user: UserRef,
}
```

### 运维子类型

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTrimStatus {
    pub trimmed_total: i64,
    pub last_trimmed_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLogTrimStatus {
    pub trimmed_total: i64,
    pub last_trimmed_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionStatus {
    pub pending_total: i64,
    pub last_failed_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerOriginStatus {
    pub origin_node_id: i64,
    pub acked_event_id: i64,
    pub applied_event_id: i64,
    pub unconfirmed_events: i64,
    pub cursor_updated_at: String,
    pub remote_last_event_id: u64,
    pub pending_catchup: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerStatus {
    pub node_id: i64,
    pub configured_url: String,
    pub source: String,
    pub discovered_url: String,
    pub discovery_state: String,
    pub last_discovered_at: String,
    pub last_connected_at: String,
    pub last_discovery_error: String,
    pub connected: bool,
    pub session_direction: String,
    pub origins: Vec<PeerOriginStatus>,
    pub pending_snapshot_partitions: i32,
    pub remote_snapshot_version: String,
    pub remote_message_window_size: i32,
    pub clock_offset_ms: i64,
    pub last_clock_sync: String,
    pub snapshot_digests_sent_total: u64,
    pub snapshot_digests_received_total: u64,
    pub snapshot_chunks_sent_total: u64,
    pub snapshot_chunks_received_total: u64,
    pub last_snapshot_digest_at: String,
    pub last_snapshot_chunk_at: String,
}
```

---

## 请求类型

```rust
#[derive(Clone, Debug, Default)]
pub struct CreateUserRequest {
    pub username: String,
    pub login_name: Option<String>,  // Some(non-empty) 绑定登录名
    pub password: Option<PasswordInput>,
    pub profile_json: Vec<u8>,       // JSON bytes
    pub role: String,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub login_name: Option<String>,  // None 不改；Some("") 解绑
    pub password: Option<PasswordInput>,
    pub profile_json: Option<Vec<u8>>,
    pub role: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct UpsertUserMetadataRequest {
    pub value: Vec<u8>,              // base64 编码
    pub expires_at: Option<String>,  // 过期时间
}

#[derive(Clone, Debug, Default)]
pub struct ScanUserMetadataRequest {
    pub prefix: String,              // 键前缀过滤
    pub after: String,               // 分页游标
    pub limit: i32,                  // 最大返回条数（默认 0，最大 1000）
}
```

---

## 枚举

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryMode {
    #[default]
    Unspecified,   // 无效值，SDK 本地拒绝
    BestEffort,    // 尽最大努力
    RouteRetry,    // 路由重试
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentType {
    #[default]
    ChannelManager,
    ChannelWriter,
    ChannelSubscription,
    UserBlacklist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordSource {
    Plain,    // 明文密码（SDK 自动做 bcrypt 编码）
    Hashed,   // 已编码的 bcrypt 密码
}
```

---

## 密码函数

```rust
// 将明文密码编码为 PasswordInput（自动做 bcrypt）
pub fn plain_password(plain: impl AsRef<str>) -> Result<PasswordInput>

// 使用已编码的 bcrypt 密码创建 PasswordInput
pub fn hashed_password(value: impl Into<String>) -> PasswordInput

// 直接进行 bcrypt 哈希
pub fn hash_password(plain: impl AsRef<str>) -> Result<String>
```

---

## 存储 trait

```rust
#[async_trait]
pub trait CursorStore: Send + Sync {
    /// 加载已确认的消息游标列表
    async fn load_seen_messages(&self) -> std::result::Result<Vec<MessageCursor>, BoxError>;

    /// 保存消息内容
    async fn save_message(&self, message: Message) -> std::result::Result<(), BoxError>;

    /// 保存消息游标
    async fn save_cursor(&self, cursor: MessageCursor) -> std::result::Result<(), BoxError>;
}
```

### MemoryCursorStore

内存实现的 `CursorStore`，提供额外的测试辅助方法：

```rust
#[derive(Clone, Default)]
pub struct MemoryCursorStore { /* ... */ }

impl MemoryCursorStore {
    pub fn new() -> Self;
    pub async fn has_cursor(&self, cursor: &MessageCursor) -> bool;
    pub async fn message(&self, cursor: &MessageCursor) -> Option<Message>;
}
```

---

## 错误类型

```rust
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub enum Error {
    Validation(ValidationError),       // 本地参数校验失败
    Closed(ClosedError),               // 客户端已关闭
    NotConnected(NotConnectedError),   // 没有活跃的 WebSocket writer
    Disconnected(DisconnectedError),   // WebSocket 已断开
    Server(ServerError),               // 服务端业务错误
    Protocol(ProtocolError),           // 协议内容不符合预期
    Connection(ConnectionError),       // 建连/读写/HTTP 错误
    Store(StoreError),                 // 本地游标存储失败
}
```

### 辅助构造器

```rust
impl Error {
    pub fn validation(message: impl Into<String>) -> Self;
    pub fn protocol(message: impl Into<String>) -> Self;
    pub fn connection(op: impl Into<String>, cause: impl Into<String>) -> Self;
    pub fn store(op: impl Into<String>, message: impl Into<String>) -> Self;
}
```

### 各变体详情

```rust
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct ValidationError {
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct ClosedError;   // "turntf client is closed"

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct NotConnectedError;  // "turntf client is not connected"

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct DisconnectedError;  // "turntf websocket disconnected"

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct ServerError {
    pub code: String,          // "unauthorized", "forbidden", "not_found" 等
    pub server_message: String,
    pub request_id: u64,
}

impl ServerError {
    pub fn unauthorized(&self) -> bool {
        self.code == "unauthorized"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct ProtocolError {
    pub protocol_message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct ConnectionError {
    pub op: String,    // 操作名，如 "dial", "write", "read", "login"
    pub cause: String, // 底层错误描述
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct StoreError {
    pub op: String,     // "save_message", "save_cursor", "load_seen_messages"
    pub message: String,
}
```

### 类型别名

```rust
pub type Result<T> = std::result::Result<T, Error>;
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
```
