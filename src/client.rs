use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::{stream::SplitSink, stream::SplitStream, Sink, SinkExt, Stream, StreamExt};
use prost::Message as ProstMessage;
use tokio::sync::{broadcast, oneshot, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::errors::{
    ClosedError, DisconnectedError, Error, NotConnectedError, Result, ServerError,
};
use crate::http::HttpClient;
use crate::mapping::{
    attachment_from_proto, attachment_type_to_proto, blacklist_entry_from_attachment,
    cluster_node_from_proto, cursor_to_proto, delivery_mode_to_proto, event_from_proto,
    logged_in_user_from_proto, login_info_from_proto, message_from_proto,
    operations_status_from_proto, packet_from_proto, relay_accepted_from_proto,
    resolved_user_sessions_from_proto, session_ref_to_proto, subscription_from_attachment,
    user_from_proto, user_metadata_from_proto, user_metadata_scan_result_from_proto,
    user_ref_to_proto,
};
use crate::password::PasswordInput;
use crate::proto;
use crate::store::CursorStore;
use crate::types::{
    default_cursor_store, Attachment, AttachmentType, BlacklistEntry, ClientConfigDefaults,
    ClusterNode, CreateUserRequest, Credentials, DeleteUserResult, DeliveryMode, Event,
    LoggedInUser, LoginInfo, Message, OperationsStatus, RelayAccepted, ResolvedUserSessions,
    ScanUserMetadataRequest, SessionRef, Subscription, UpdateUserRequest,
    UpsertUserMetadataRequest, User, UserMetadata, UserMetadataScanResult, UserRef,
};
use crate::validation::{
    normalize_login_name, validate_delivery_mode, validate_login_name,
    validate_optional_user_metadata_key_fragment, validate_positive_i64, validate_session_ref,
    validate_user_metadata_key, validate_user_metadata_scan_limit, validate_user_ref,
};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type WsWriter = SplitSink<WsStream, WsMessage>;
type WsReader = SplitStream<WsStream>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientEvent {
    Login(LoginInfo),
    Message(Message),
    Packet(crate::types::Packet),
    Error(Error),
    Disconnect(Error),
}

pub struct ClientSubscription {
    inner: BroadcastStream<ClientEvent>,
}

impl Stream for ClientSubscription {
    type Item = std::result::Result<ClientEvent, BroadcastStreamRecvError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

#[derive(Clone)]
pub struct Config {
    pub base_url: String,
    pub credentials: Credentials,
    pub login_name: Option<String>,
    pub cursor_store: Arc<dyn CursorStore>,
    pub reconnect: bool,
    pub initial_reconnect_delay: Duration,
    pub max_reconnect_delay: Duration,
    pub ping_interval: Duration,
    pub request_timeout: Duration,
    pub ack_messages: bool,
    pub transient_only: bool,
    pub realtime_stream: bool,
    pub event_channel_capacity: usize,
}

impl Config {
    pub fn new(base_url: impl Into<String>, credentials: Credentials) -> Self {
        let defaults = ClientConfigDefaults::default();
        Self {
            base_url: base_url.into(),
            credentials,
            login_name: None,
            cursor_store: default_cursor_store(),
            reconnect: defaults.reconnect,
            initial_reconnect_delay: defaults.initial_reconnect_delay,
            max_reconnect_delay: defaults.max_reconnect_delay,
            ping_interval: defaults.ping_interval,
            request_timeout: defaults.request_timeout,
            ack_messages: defaults.ack_messages,
            transient_only: defaults.transient_only,
            realtime_stream: defaults.realtime_stream,
            event_channel_capacity: defaults.event_channel_capacity,
        }
    }

    pub fn new_with_login_name(
        base_url: impl Into<String>,
        login_name: impl Into<String>,
        password: PasswordInput,
    ) -> Self {
        let defaults = ClientConfigDefaults::default();
        Self {
            base_url: base_url.into(),
            credentials: Credentials {
                node_id: 0,
                user_id: 0,
                password,
            },
            login_name: Some(login_name.into()),
            cursor_store: default_cursor_store(),
            reconnect: defaults.reconnect,
            initial_reconnect_delay: defaults.initial_reconnect_delay,
            max_reconnect_delay: defaults.max_reconnect_delay,
            ping_interval: defaults.ping_interval,
            request_timeout: defaults.request_timeout,
            ack_messages: defaults.ack_messages,
            transient_only: defaults.transient_only,
            realtime_stream: defaults.realtime_stream,
            event_channel_capacity: defaults.event_channel_capacity,
        }
    }
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    cfg: Config,
    http: HttpClient,
    events: RwLock<Option<broadcast::Sender<ClientEvent>>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<RpcValue>>>>,
    writer: Mutex<Option<ActiveWriter>>,
    run_task: Mutex<Option<JoinHandle<()>>>,
    state: Mutex<State>,
    state_notify: Notify,
    request_id: AtomicU64,
    connection_id: AtomicU64,
}

struct ActiveWriter {
    session_id: u64,
    sink: WsWriter,
}

#[derive(Default)]
struct State {
    closed: bool,
    connected: bool,
    stop_reconnect: bool,
    terminal_error: Option<Error>,
}

enum RpcValue {
    Unit,
    Message(Message),
    RelayAccepted(RelayAccepted),
    User(User),
    DeleteUserResult(DeleteUserResult),
    UserMetadata(UserMetadata),
    UserMetadataScanResult(UserMetadataScanResult),
    Attachment(Attachment),
    Attachments(Vec<Attachment>),
    Messages(Vec<Message>),
    Events(Vec<Event>),
    ClusterNodes(Vec<ClusterNode>),
    LoggedInUsers(Vec<LoggedInUser>),
    ResolvedUserSessions(ResolvedUserSessions),
    OperationsStatus(OperationsStatus),
    Metrics(String),
}

impl Client {
    pub fn new(mut config: Config) -> Result<Self> {
        if config.base_url.trim().is_empty() {
            return Err(Error::validation("base_url is required"));
        }
        if let Some(login_name) = config.login_name.take() {
            validate_login_name(&login_name, "login_name")?;
            config.login_name = Some(normalize_login_name(&login_name));
        } else {
            validate_positive_i64(config.credentials.node_id, "credentials.node_id")?;
            validate_positive_i64(config.credentials.user_id, "credentials.user_id")?;
        }
        config.credentials.password.validate()?;

        let http = HttpClient::new(config.base_url.clone())?;
        let (events_tx, _) = broadcast::channel(config.event_channel_capacity.max(1));

        Ok(Self {
            inner: Arc::new(Inner {
                cfg: Config {
                    event_channel_capacity: config.event_channel_capacity.max(1),
                    ..config
                },
                http,
                events: RwLock::new(Some(events_tx)),
                pending: Mutex::new(HashMap::new()),
                writer: Mutex::new(None),
                run_task: Mutex::new(None),
                state: Mutex::new(State::default()),
                state_notify: Notify::new(),
                request_id: AtomicU64::new(0),
                connection_id: AtomicU64::new(0),
            }),
        })
    }

    pub fn http(&self) -> HttpClient {
        self.inner.http.clone()
    }

    pub async fn subscribe(&self) -> ClientSubscription {
        let sender = self.inner.events.read().await.clone();
        let inner = match sender {
            Some(sender) => BroadcastStream::new(sender.subscribe()),
            None => {
                let (sender, receiver) = broadcast::channel(1);
                drop(sender);
                BroadcastStream::new(receiver)
            }
        };
        ClientSubscription { inner }
    }

    pub async fn login(
        &self,
        node_id: i64,
        user_id: i64,
        password: impl AsRef<str>,
    ) -> Result<String> {
        self.inner.http.login(node_id, user_id, password).await
    }

    pub async fn login_with_password(
        &self,
        node_id: i64,
        user_id: i64,
        password: PasswordInput,
    ) -> Result<String> {
        self.inner
            .http
            .login_with_password(node_id, user_id, password)
            .await
    }

    pub async fn login_by_login_name(
        &self,
        login_name: impl AsRef<str>,
        password: impl AsRef<str>,
    ) -> Result<String> {
        self.inner
            .http
            .login_by_login_name(login_name, password)
            .await
    }

    pub async fn login_by_login_name_with_password(
        &self,
        login_name: impl AsRef<str>,
        password: PasswordInput,
    ) -> Result<String> {
        self.inner
            .http
            .login_by_login_name_with_password(login_name, password)
            .await
    }

    pub async fn connect(&self) -> Result<()> {
        {
            let state = self.inner.state.lock().await;
            if state.closed {
                return Err(ClosedError.into());
            }
            if state.connected {
                return Ok(());
            }
        }

        self.ensure_started().await;

        loop {
            let notified = self.inner.state_notify.notified();
            {
                let state = self.inner.state.lock().await;
                if state.connected {
                    return Ok(());
                }
                if let Some(error) = &state.terminal_error {
                    return Err(error.clone());
                }
                if state.closed {
                    return Err(ClosedError.into());
                }
            }
            notified.await;
        }
    }

    pub async fn close(&self) -> Result<()> {
        let already_closed = {
            let mut state = self.inner.state.lock().await;
            if state.closed {
                true
            } else {
                state.closed = true;
                state.connected = false;
                state.terminal_error = Some(Error::from(ClosedError));
                false
            }
        };
        self.inner.state_notify.notify_waiters();

        if !already_closed {
            self.inner
                .emit_event(ClientEvent::Disconnect(Error::from(ClosedError)))
                .await;
        }

        self.inner.fail_all_pending(Error::from(ClosedError)).await;
        self.inner.take_writer().await;

        let handle = self.inner.run_task.lock().await.take();
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
        }

        self.inner.events.write().await.take();
        Ok(())
    }

    pub async fn ping(&self) -> Result<()> {
        match self
            .rpc(|request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::Ping(proto::Ping {
                    request_id,
                })),
            })
            .await?
        {
            RpcValue::Unit => Ok(()),
            _ => Err(Error::protocol("missing pong response")),
        }
    }

    pub async fn send_message(&self, target: UserRef, body: Vec<u8>) -> Result<Message> {
        validate_user_ref(&target, "target")?;
        if body.is_empty() {
            return Err(Error::validation("body is required"));
        }
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::SendMessage(
                    proto::SendMessageRequest {
                        request_id,
                        target: Some(user_ref_to_proto(&target)),
                        body: body.into(),
                        delivery_kind: 0,
                        delivery_mode: 0,
                        sync_mode: 0,
                        target_session: None,
                    },
                )),
            })
            .await?
        {
            RpcValue::Message(message) => Ok(message),
            _ => Err(Error::protocol("missing message in send response")),
        }
    }

    pub async fn post_message(&self, target: UserRef, body: Vec<u8>) -> Result<Message> {
        self.send_message(target, body).await
    }

    pub async fn send_packet(
        &self,
        target: UserRef,
        body: Vec<u8>,
        delivery_mode: DeliveryMode,
    ) -> Result<RelayAccepted> {
        self.send_packet_inner(target, body, delivery_mode, None)
            .await
    }

    pub async fn send_packet_to_session(
        &self,
        target: UserRef,
        body: Vec<u8>,
        delivery_mode: DeliveryMode,
        target_session: SessionRef,
    ) -> Result<RelayAccepted> {
        self.send_packet_inner(target, body, delivery_mode, Some(target_session))
            .await
    }

    async fn send_packet_inner(
        &self,
        target: UserRef,
        body: Vec<u8>,
        delivery_mode: DeliveryMode,
        target_session: Option<SessionRef>,
    ) -> Result<RelayAccepted> {
        validate_user_ref(&target, "target")?;
        validate_delivery_mode(delivery_mode)?;
        if let Some(target_session) = target_session.as_ref() {
            validate_session_ref(target_session, "target_session")?;
        }
        if body.is_empty() {
            return Err(Error::validation("body is required"));
        }
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::SendMessage(
                    proto::SendMessageRequest {
                        request_id,
                        target: Some(user_ref_to_proto(&target)),
                        body: body.into(),
                        delivery_kind: proto::ClientDeliveryKind::Transient as i32,
                        delivery_mode: delivery_mode_to_proto(delivery_mode),
                        sync_mode: 0,
                        target_session: target_session.as_ref().map(session_ref_to_proto),
                    },
                )),
            })
            .await?
        {
            RpcValue::RelayAccepted(accepted) => Ok(accepted),
            _ => Err(Error::protocol(
                "missing transient_accepted in send response",
            )),
        }
    }

    pub async fn post_packet(
        &self,
        target: UserRef,
        body: Vec<u8>,
        delivery_mode: DeliveryMode,
    ) -> Result<RelayAccepted> {
        self.send_packet(target, body, delivery_mode).await
    }

    pub async fn post_packet_to_session(
        &self,
        target: UserRef,
        body: Vec<u8>,
        delivery_mode: DeliveryMode,
        target_session: SessionRef,
    ) -> Result<RelayAccepted> {
        self.send_packet_to_session(target, body, delivery_mode, target_session)
            .await
    }

    pub async fn create_user(&self, request: CreateUserRequest) -> Result<User> {
        let CreateUserRequest {
            username,
            login_name,
            password,
            profile_json,
            role,
        } = request;
        if username.is_empty() {
            return Err(Error::validation("username is required"));
        }
        if role.is_empty() {
            return Err(Error::validation("role is required"));
        }
        let login_name = login_name
            .map(|value| normalize_login_name(&value))
            .filter(|value| !value.is_empty());
        if role.trim() == "channel" && login_name.is_some() {
            return Err(Error::validation(
                "login_name requires a login-enabled user",
            ));
        }
        let password = password
            .as_ref()
            .map(|password| password.wire_value().map(str::to_owned))
            .transpose()?
            .unwrap_or_default();
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::CreateUser(
                    proto::CreateUserRequest {
                        request_id,
                        username,
                        password,
                        profile_json: profile_json.into(),
                        role,
                        login_name: login_name.unwrap_or_default(),
                    },
                )),
            })
            .await?
        {
            RpcValue::User(user) => Ok(user),
            _ => Err(Error::protocol("missing user in create_user_response")),
        }
    }

    pub async fn create_channel(&self, mut request: CreateUserRequest) -> Result<User> {
        if request.role.is_empty() {
            request.role = "channel".to_owned();
        }
        self.create_user(request).await
    }

    pub async fn get_user(&self, target: UserRef) -> Result<User> {
        validate_user_ref(&target, "target")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::GetUser(
                    proto::GetUserRequest {
                        request_id,
                        user: Some(user_ref_to_proto(&target)),
                    },
                )),
            })
            .await?
        {
            RpcValue::User(user) => Ok(user),
            _ => Err(Error::protocol("missing user in get_user_response")),
        }
    }

    pub async fn update_user(&self, target: UserRef, request: UpdateUserRequest) -> Result<User> {
        validate_user_ref(&target, "target")?;
        let UpdateUserRequest {
            username,
            login_name,
            password,
            profile_json,
            role,
        } = request;
        let login_name = login_name.map(|value| normalize_login_name(&value));
        if role.as_deref().map(str::trim) == Some("channel")
            && matches!(login_name.as_deref(), Some(value) if !value.is_empty())
        {
            return Err(Error::validation(
                "login_name requires a login-enabled user",
            ));
        }
        let password = password
            .as_ref()
            .map(|value| value.wire_value().map(str::to_owned))
            .transpose()?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::UpdateUser(
                    proto::UpdateUserRequest {
                        request_id,
                        user: Some(user_ref_to_proto(&target)),
                        username: username.map(|value| proto::StringField { value }),
                        login_name: login_name.map(|value| proto::StringField { value }),
                        password: password.map(|value| proto::StringField { value }),
                        profile_json: profile_json.map(|value| proto::BytesField {
                            value: value.into(),
                        }),
                        role: role.map(|value| proto::StringField { value }),
                    },
                )),
            })
            .await?
        {
            RpcValue::User(user) => Ok(user),
            _ => Err(Error::protocol("missing user in update_user_response")),
        }
    }

    pub async fn delete_user(&self, target: UserRef) -> Result<DeleteUserResult> {
        validate_user_ref(&target, "target")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::DeleteUser(
                    proto::DeleteUserRequest {
                        request_id,
                        user: Some(user_ref_to_proto(&target)),
                    },
                )),
            })
            .await?
        {
            RpcValue::DeleteUserResult(result) => Ok(result),
            _ => Err(Error::protocol("missing status in delete_user_response")),
        }
    }

    pub async fn get_user_metadata(
        &self,
        owner: UserRef,
        key: impl Into<String>,
    ) -> Result<UserMetadata> {
        validate_user_ref(&owner, "owner")?;
        let key = key.into();
        validate_user_metadata_key(&key, "key")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::GetUserMetadata(
                    proto::GetUserMetadataRequest {
                        request_id,
                        owner: Some(user_ref_to_proto(&owner)),
                        key,
                    },
                )),
            })
            .await?
        {
            RpcValue::UserMetadata(metadata) => Ok(metadata),
            _ => Err(Error::protocol(
                "missing metadata in get_user_metadata_response",
            )),
        }
    }

    pub async fn upsert_user_metadata(
        &self,
        owner: UserRef,
        key: impl Into<String>,
        request: UpsertUserMetadataRequest,
    ) -> Result<UserMetadata> {
        validate_user_ref(&owner, "owner")?;
        let key = key.into();
        validate_user_metadata_key(&key, "key")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::UpsertUserMetadata(
                    proto::UpsertUserMetadataRequest {
                        request_id,
                        owner: Some(user_ref_to_proto(&owner)),
                        key,
                        value: request.value.into(),
                        expires_at: request.expires_at.map(|value| proto::StringField { value }),
                    },
                )),
            })
            .await?
        {
            RpcValue::UserMetadata(metadata) => Ok(metadata),
            _ => Err(Error::protocol(
                "missing metadata in upsert_user_metadata_response",
            )),
        }
    }

    pub async fn delete_user_metadata(
        &self,
        owner: UserRef,
        key: impl Into<String>,
    ) -> Result<UserMetadata> {
        validate_user_ref(&owner, "owner")?;
        let key = key.into();
        validate_user_metadata_key(&key, "key")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::DeleteUserMetadata(
                    proto::DeleteUserMetadataRequest {
                        request_id,
                        owner: Some(user_ref_to_proto(&owner)),
                        key,
                    },
                )),
            })
            .await?
        {
            RpcValue::UserMetadata(metadata) => Ok(metadata),
            _ => Err(Error::protocol(
                "missing metadata in delete_user_metadata_response",
            )),
        }
    }

    pub async fn scan_user_metadata(
        &self,
        owner: UserRef,
        request: ScanUserMetadataRequest,
    ) -> Result<UserMetadataScanResult> {
        validate_user_ref(&owner, "owner")?;
        validate_optional_user_metadata_key_fragment(&request.prefix, "prefix")?;
        validate_optional_user_metadata_key_fragment(&request.after, "after")?;
        validate_user_metadata_scan_limit(request.limit)?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::ScanUserMetadata(
                    proto::ScanUserMetadataRequest {
                        request_id,
                        owner: Some(user_ref_to_proto(&owner)),
                        prefix: request.prefix,
                        after: request.after,
                        limit: request.limit,
                    },
                )),
            })
            .await?
        {
            RpcValue::UserMetadataScanResult(result) => Ok(result),
            _ => Err(Error::protocol(
                "missing items in scan_user_metadata_response",
            )),
        }
    }

    pub async fn subscribe_channel(
        &self,
        subscriber: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        let attachment = self
            .upsert_attachment(
                subscriber,
                channel,
                AttachmentType::ChannelSubscription,
                b"{}".to_vec(),
            )
            .await?;
        Ok(subscription_from_attachment(&attachment))
    }

    pub async fn create_subscription(
        &self,
        subscriber: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        self.subscribe_channel(subscriber, channel).await
    }

    pub async fn unsubscribe_channel(
        &self,
        subscriber: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        let attachment = self
            .delete_attachment(subscriber, channel, AttachmentType::ChannelSubscription)
            .await?;
        Ok(subscription_from_attachment(&attachment))
    }

    pub async fn list_subscriptions(&self, subscriber: UserRef) -> Result<Vec<Subscription>> {
        self.list_attachments(subscriber, Some(AttachmentType::ChannelSubscription))
            .await?
            .into_iter()
            .map(|attachment| Ok(subscription_from_attachment(&attachment)))
            .collect()
    }

    pub async fn block_user(&self, owner: UserRef, blocked: UserRef) -> Result<BlacklistEntry> {
        let attachment = self
            .upsert_attachment(
                owner,
                blocked,
                AttachmentType::UserBlacklist,
                b"{}".to_vec(),
            )
            .await?;
        Ok(blacklist_entry_from_attachment(&attachment))
    }

    pub async fn unblock_user(&self, owner: UserRef, blocked: UserRef) -> Result<BlacklistEntry> {
        let attachment = self
            .delete_attachment(owner, blocked, AttachmentType::UserBlacklist)
            .await?;
        Ok(blacklist_entry_from_attachment(&attachment))
    }

    pub async fn list_blocked_users(&self, owner: UserRef) -> Result<Vec<BlacklistEntry>> {
        self.list_attachments(owner, Some(AttachmentType::UserBlacklist))
            .await?
            .into_iter()
            .map(|attachment| Ok(blacklist_entry_from_attachment(&attachment)))
            .collect()
    }

    pub async fn upsert_attachment(
        &self,
        owner: UserRef,
        subject: UserRef,
        attachment_type: AttachmentType,
        config_json: Vec<u8>,
    ) -> Result<Attachment> {
        validate_user_ref(&owner, "owner")?;
        validate_user_ref(&subject, "subject")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::UpsertUserAttachment(
                    proto::UpsertUserAttachmentRequest {
                        request_id,
                        owner: Some(user_ref_to_proto(&owner)),
                        subject: Some(user_ref_to_proto(&subject)),
                        attachment_type: attachment_type_to_proto(attachment_type),
                        config_json: config_json.into(),
                    },
                )),
            })
            .await?
        {
            RpcValue::Attachment(attachment) => Ok(attachment),
            _ => Err(Error::protocol(
                "missing attachment in upsert_user_attachment_response",
            )),
        }
    }

    pub async fn delete_attachment(
        &self,
        owner: UserRef,
        subject: UserRef,
        attachment_type: AttachmentType,
    ) -> Result<Attachment> {
        validate_user_ref(&owner, "owner")?;
        validate_user_ref(&subject, "subject")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::DeleteUserAttachment(
                    proto::DeleteUserAttachmentRequest {
                        request_id,
                        owner: Some(user_ref_to_proto(&owner)),
                        subject: Some(user_ref_to_proto(&subject)),
                        attachment_type: attachment_type_to_proto(attachment_type),
                    },
                )),
            })
            .await?
        {
            RpcValue::Attachment(attachment) => Ok(attachment),
            _ => Err(Error::protocol(
                "missing attachment in delete_user_attachment_response",
            )),
        }
    }

    pub async fn list_attachments(
        &self,
        owner: UserRef,
        attachment_type: Option<AttachmentType>,
    ) -> Result<Vec<Attachment>> {
        validate_user_ref(&owner, "owner")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::ListUserAttachments(
                    proto::ListUserAttachmentsRequest {
                        request_id,
                        owner: Some(user_ref_to_proto(&owner)),
                        attachment_type: attachment_type
                            .map(attachment_type_to_proto)
                            .unwrap_or(proto::AttachmentType::Unspecified as i32),
                    },
                )),
            })
            .await?
        {
            RpcValue::Attachments(items) => Ok(items),
            _ => Err(Error::protocol(
                "missing items in list_user_attachments_response",
            )),
        }
    }

    pub async fn list_messages(&self, target: UserRef, limit: i32) -> Result<Vec<Message>> {
        validate_user_ref(&target, "target")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::ListMessages(
                    proto::ListMessagesRequest {
                        request_id,
                        user: Some(user_ref_to_proto(&target)),
                        limit,
                    },
                )),
            })
            .await?
        {
            RpcValue::Messages(items) => Ok(items),
            _ => Err(Error::protocol("missing items in list_messages_response")),
        }
    }

    pub async fn list_events(&self, after: i64, limit: i32) -> Result<Vec<Event>> {
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::ListEvents(
                    proto::ListEventsRequest {
                        request_id,
                        after,
                        limit,
                    },
                )),
            })
            .await?
        {
            RpcValue::Events(items) => Ok(items),
            _ => Err(Error::protocol("missing items in list_events_response")),
        }
    }

    pub async fn list_cluster_nodes(&self) -> Result<Vec<ClusterNode>> {
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::ListClusterNodes(
                    proto::ListClusterNodesRequest { request_id },
                )),
            })
            .await?
        {
            RpcValue::ClusterNodes(items) => Ok(items),
            _ => Err(Error::protocol(
                "missing items in list_cluster_nodes_response",
            )),
        }
    }

    pub async fn list_node_logged_in_users(&self, node_id: i64) -> Result<Vec<LoggedInUser>> {
        validate_positive_i64(node_id, "node_id")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::ListNodeLoggedInUsers(
                    proto::ListNodeLoggedInUsersRequest {
                        request_id,
                        node_id,
                    },
                )),
            })
            .await?
        {
            RpcValue::LoggedInUsers(items) => Ok(items),
            _ => Err(Error::protocol(
                "missing items in list_node_logged_in_users_response",
            )),
        }
    }

    pub async fn resolve_user_sessions(&self, user: UserRef) -> Result<ResolvedUserSessions> {
        validate_user_ref(&user, "user")?;
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::ResolveUserSessions(
                    proto::ResolveUserSessionsRequest {
                        request_id,
                        user: Some(user_ref_to_proto(&user)),
                    },
                )),
            })
            .await?
        {
            RpcValue::ResolvedUserSessions(result) => Ok(result),
            _ => Err(Error::protocol(
                "missing result in resolve_user_sessions_response",
            )),
        }
    }

    pub async fn operations_status(&self) -> Result<OperationsStatus> {
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::OperationsStatus(
                    proto::OperationsStatusRequest { request_id },
                )),
            })
            .await?
        {
            RpcValue::OperationsStatus(status) => Ok(status),
            _ => Err(Error::protocol(
                "missing status in operations_status_response",
            )),
        }
    }

    pub async fn metrics(&self) -> Result<String> {
        match self
            .rpc(move |request_id| proto::ClientEnvelope {
                body: Some(proto::client_envelope::Body::Metrics(
                    proto::MetricsRequest { request_id },
                )),
            })
            .await?
        {
            RpcValue::Metrics(text) => Ok(text),
            _ => Err(Error::protocol("missing text in metrics_response")),
        }
    }

    async fn ensure_started(&self) {
        let mut handle = self.inner.run_task.lock().await;
        if handle.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }

        {
            let mut state = self.inner.state.lock().await;
            state.connected = false;
            state.stop_reconnect = false;
            state.terminal_error = None;
        }
        self.inner.state_notify.notify_waiters();

        let inner = Arc::clone(&self.inner);
        *handle = Some(tokio::spawn(async move {
            inner.run().await;
        }));
    }

    async fn rpc<F>(&self, build: F) -> Result<RpcValue>
    where
        F: FnOnce(u64) -> proto::ClientEnvelope,
    {
        self.inner.rpc(build).await
    }
}

impl Inner {
    async fn run(self: Arc<Self>) {
        let mut delay = self.cfg.initial_reconnect_delay;

        loop {
            let error = match self.connect_and_serve().await {
                Ok(()) => {
                    delay = self.cfg.initial_reconnect_delay;
                    if self.is_closed().await {
                        self.set_terminal_error(Error::from(ClosedError)).await;
                        return;
                    }
                    continue;
                }
                Err(error) => error,
            };

            if !self.should_retry(&error).await {
                self.fail_all_pending(error.clone()).await;
                self.set_terminal_error(error).await;
                return;
            }

            self.emit_event(ClientEvent::Error(error.clone())).await;
            sleep(delay).await;
            delay = std::cmp::min(delay.saturating_mul(2), self.cfg.max_reconnect_delay);
        }
    }

    async fn connect_and_serve(self: &Arc<Self>) -> Result<()> {
        if self.is_closed().await {
            return Err(ClosedError.into());
        }

        let seen_messages = self
            .cfg
            .cursor_store
            .load_seen_messages()
            .await
            .map_err(|err| Error::store("load_seen_messages", err.to_string()))?;

        let ws_url = websocket_url(&self.cfg.base_url, self.cfg.realtime_stream)?;
        let (mut stream, _) = connect_async(ws_url.as_str())
            .await
            .map_err(|err| Error::connection("dial", err.to_string()))?;

        let login_user = if self.cfg.login_name.is_some() {
            None
        } else {
            Some(user_ref_to_proto(&UserRef {
                node_id: self.cfg.credentials.node_id,
                user_id: self.cfg.credentials.user_id,
            }))
        };
        let login = proto::ClientEnvelope {
            body: Some(proto::client_envelope::Body::Login(proto::LoginRequest {
                user: login_user,
                login_name: self.cfg.login_name.clone().unwrap_or_default(),
                password: self.cfg.credentials.password.wire_value()?.to_owned(),
                seen_messages: seen_messages.iter().map(cursor_to_proto).collect(),
                transient_only: self.cfg.transient_only,
            })),
        };
        write_proto(&mut stream, login).await?;
        let login_info =
            self.expect_login(&read_proto(&mut stream, self.is_closed().await).await?)?;

        let session_id = self.connection_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (writer, reader) = stream.split();
        *self.writer.lock().await = Some(ActiveWriter {
            session_id,
            sink: writer,
        });
        self.set_connected(true).await;
        self.emit_event(ClientEvent::Login(login_info)).await;

        let ping_task = {
            let inner = Arc::clone(self);
            tokio::spawn(async move {
                inner.ping_loop().await;
            })
        };

        let read_result = self.read_loop(session_id, reader).await;
        ping_task.abort();
        let _ = ping_task.await;

        self.set_connected(false).await;
        self.clear_writer(session_id).await;
        if !self.is_closed().await {
            self.fail_all_pending(Error::from(DisconnectedError)).await;
            let disconnect_error = match &read_result {
                Ok(()) => Error::from(DisconnectedError),
                Err(error) => error.clone(),
            };
            self.emit_event(ClientEvent::Disconnect(disconnect_error))
                .await;
        }

        read_result
    }

    async fn read_loop(&self, session_id: u64, mut reader: WsReader) -> Result<()> {
        loop {
            let env = match read_proto(&mut reader, self.is_closed().await).await {
                Ok(env) => env,
                Err(error) => return Err(error),
            };
            if let Err(error) = self.handle_server_envelope(session_id, env).await {
                self.emit_event(ClientEvent::Error(error)).await;
            }
        }
    }

    async fn handle_server_envelope(
        &self,
        _session_id: u64,
        env: proto::ServerEnvelope,
    ) -> Result<()> {
        match env.body {
            Some(proto::server_envelope::Body::MessagePushed(message)) => {
                let message = message_from_proto(message.message.as_ref())?;
                self.persist_message(message.clone()).await?;
                if self.cfg.ack_messages {
                    let ack = proto::ClientEnvelope {
                        body: Some(proto::client_envelope::Body::AckMessage(
                            proto::AckMessage {
                                cursor: Some(cursor_to_proto(&message.cursor())),
                            },
                        )),
                    };
                    match self.send_envelope(ack).await {
                        Err(Error::NotConnected(_)) | Err(Error::Closed(_)) => {}
                        Err(error) => self.emit_event(ClientEvent::Error(error)).await,
                        Ok(()) => {}
                    }
                }
                self.emit_event(ClientEvent::Message(message)).await;
            }
            Some(proto::server_envelope::Body::PacketPushed(packet)) => {
                self.emit_event(ClientEvent::Packet(packet_from_proto(
                    packet.packet.as_ref(),
                )?))
                .await;
            }
            Some(proto::server_envelope::Body::SendMessageResponse(response)) => {
                let result = match response.body {
                    Some(proto::send_message_response::Body::Message(message)) => {
                        let message = message_from_proto(Some(&message))?;
                        match self.persist_message(message.clone()).await {
                            Ok(()) => Ok(RpcValue::Message(message)),
                            Err(error) => Err(error),
                        }
                    }
                    Some(proto::send_message_response::Body::TransientAccepted(accepted)) => Ok(
                        RpcValue::RelayAccepted(relay_accepted_from_proto(Some(&accepted))?),
                    ),
                    None => Err(Error::protocol("empty send_message_response")),
                };
                self.resolve_pending(response.request_id, result).await;
            }
            Some(proto::server_envelope::Body::Pong(pong)) => {
                self.resolve_pending(pong.request_id, Ok(RpcValue::Unit))
                    .await;
            }
            Some(proto::server_envelope::Body::CreateUserResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::User(user_from_proto(response.user.as_ref())?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::GetUserResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::User(user_from_proto(response.user.as_ref())?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::UpdateUserResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::User(user_from_proto(response.user.as_ref())?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::DeleteUserResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::DeleteUserResult(DeleteUserResult {
                        status: response.status,
                        user: crate::mapping::user_ref_from_proto(response.user.as_ref()),
                    })),
                )
                .await;
            }
            Some(proto::server_envelope::Body::GetUserMetadataResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::UserMetadata(user_metadata_from_proto(
                        response.metadata.as_ref(),
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::UpsertUserMetadataResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::UserMetadata(user_metadata_from_proto(
                        response.metadata.as_ref(),
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::DeleteUserMetadataResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::UserMetadata(user_metadata_from_proto(
                        response.metadata.as_ref(),
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::ScanUserMetadataResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::UserMetadataScanResult(
                        user_metadata_scan_result_from_proto(&response)?,
                    )),
                )
                .await;
            }
            Some(proto::server_envelope::Body::ListMessagesResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::Messages(messages_from_proto(&response.items)?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::UpsertUserAttachmentResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::Attachment(attachment_from_proto(
                        response.attachment.as_ref(),
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::DeleteUserAttachmentResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::Attachment(attachment_from_proto(
                        response.attachment.as_ref(),
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::ListUserAttachmentsResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::Attachments(attachments_from_proto(
                        &response.items,
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::ListEventsResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::Events(events_from_proto(&response.items)?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::ListClusterNodesResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::ClusterNodes(cluster_nodes_from_proto(
                        &response.items,
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::ListNodeLoggedInUsersResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::LoggedInUsers(logged_in_users_from_proto(
                        &response.items,
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::ResolveUserSessionsResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::ResolvedUserSessions(
                        resolved_user_sessions_from_proto(&response)?,
                    )),
                )
                .await;
            }
            Some(proto::server_envelope::Body::OperationsStatusResponse(response)) => {
                self.resolve_pending(
                    response.request_id,
                    Ok(RpcValue::OperationsStatus(operations_status_from_proto(
                        response.status.as_ref(),
                    )?)),
                )
                .await;
            }
            Some(proto::server_envelope::Body::MetricsResponse(response)) => {
                self.resolve_pending(response.request_id, Ok(RpcValue::Metrics(response.text)))
                    .await;
            }
            Some(proto::server_envelope::Body::Error(error)) => {
                let error = Error::from(ServerError::new(
                    error.code,
                    error.message,
                    error.request_id,
                ));
                let request_id = match &error {
                    Error::Server(server) => server.request_id,
                    _ => 0,
                };
                if request_id != 0 {
                    self.resolve_pending(request_id, Err(error)).await;
                } else {
                    return Err(error);
                }
            }
            Some(proto::server_envelope::Body::LoginResponse(_)) => {
                return Err(Error::protocol(
                    "unexpected login_response after authentication",
                ));
            }
            None => return Err(Error::protocol("unsupported server envelope")),
        }
        Ok(())
    }

    fn expect_login(&self, env: &proto::ServerEnvelope) -> Result<LoginInfo> {
        match env.body.as_ref() {
            Some(proto::server_envelope::Body::LoginResponse(response)) => {
                login_info_from_proto(response)
            }
            Some(proto::server_envelope::Body::Error(error)) => {
                if error.code == "unauthorized" {
                    if let Ok(mut state) = self.state.try_lock() {
                        state.stop_reconnect = true;
                    }
                }
                Err(
                    ServerError::new(error.code.clone(), error.message.clone(), error.request_id)
                        .into(),
                )
            }
            _ => Err(Error::protocol("expected login_response or error")),
        }
    }

    async fn persist_message(&self, message: Message) -> Result<()> {
        self.cfg
            .cursor_store
            .save_message(message.clone())
            .await
            .map_err(|err| Error::store("save_message", err.to_string()))?;
        self.cfg
            .cursor_store
            .save_cursor(message.cursor())
            .await
            .map_err(|err| Error::store("save_cursor", err.to_string()))?;
        Ok(())
    }

    async fn send_envelope(&self, env: proto::ClientEnvelope) -> Result<()> {
        if self.is_closed().await {
            return Err(ClosedError.into());
        }

        let payload = env.encode_to_vec();
        let mut writer = self.writer.lock().await;
        let active = writer
            .as_mut()
            .ok_or_else(|| Error::from(NotConnectedError))?;
        active
            .sink
            .send(WsMessage::Binary(payload.into()))
            .await
            .map_err(|err| Error::connection("write", err.to_string()))
    }

    async fn rpc<F>(&self, build: F) -> Result<RpcValue>
    where
        F: FnOnce(u64) -> proto::ClientEnvelope,
    {
        let request_id = self.request_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, receiver) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id, sender);
        }

        if let Err(error) = self.send_envelope(build(request_id)).await {
            self.pending.lock().await.remove(&request_id);
            return Err(error);
        }

        let response = tokio::time::timeout(self.cfg.request_timeout, receiver).await;
        self.pending.lock().await.remove(&request_id);

        match response {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(DisconnectedError.into()),
            Err(_) => Err(Error::connection(
                format!("request {request_id}"),
                format!("timed out after {}ms", self.cfg.request_timeout.as_millis()),
            )),
        }
    }

    async fn resolve_pending(&self, request_id: u64, result: Result<RpcValue>) {
        let sender = self.pending.lock().await.remove(&request_id);
        if let Some(sender) = sender {
            let _ = sender.send(result);
        }
    }

    async fn fail_all_pending(&self, error: Error) {
        let mut pending = self.pending.lock().await;
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(error.clone()));
        }
    }

    async fn emit_event(&self, event: ClientEvent) {
        if let Some(sender) = self.events.read().await.clone() {
            let _ = sender.send(event);
        }
    }

    async fn should_retry(&self, error: &Error) -> bool {
        let state = self.state.lock().await;
        if state.closed || state.stop_reconnect || !self.cfg.reconnect {
            return false;
        }
        !matches!(error, Error::Closed(_))
            && !matches!(error, Error::Server(err) if err.unauthorized())
    }

    async fn set_connected(&self, connected: bool) {
        let mut state = self.state.lock().await;
        state.connected = connected;
        if connected {
            state.terminal_error = None;
        }
        self.state_notify.notify_waiters();
    }

    async fn set_terminal_error(&self, error: Error) {
        let mut state = self.state.lock().await;
        state.connected = false;
        state.terminal_error = Some(error);
        self.state_notify.notify_waiters();
    }

    async fn clear_writer(&self, session_id: u64) {
        let mut writer = self.writer.lock().await;
        if writer
            .as_ref()
            .is_some_and(|active| active.session_id == session_id)
        {
            writer.take();
        }
    }

    async fn take_writer(&self) {
        self.writer.lock().await.take();
    }

    async fn is_closed(&self) -> bool {
        self.state.lock().await.closed
    }

    async fn ping_loop(self: Arc<Self>) {
        loop {
            sleep(self.cfg.ping_interval).await;
            match self
                .rpc(|request_id| proto::ClientEnvelope {
                    body: Some(proto::client_envelope::Body::Ping(proto::Ping {
                        request_id,
                    })),
                })
                .await
            {
                Ok(RpcValue::Unit) => {}
                Ok(_) => {
                    self.emit_event(ClientEvent::Error(Error::protocol("missing pong response")))
                        .await
                }
                Err(Error::NotConnected(_))
                | Err(Error::Closed(_))
                | Err(Error::Disconnected(_)) => {
                    return;
                }
                Err(error) => {
                    self.emit_event(ClientEvent::Error(error)).await;
                }
            }
        }
    }
}

async fn write_proto<S>(sink: &mut S, env: proto::ClientEnvelope) -> Result<()>
where
    S: Sink<WsMessage> + Unpin,
    S::Error: std::fmt::Display,
{
    sink.send(WsMessage::Binary(env.encode_to_vec().into()))
        .await
        .map_err(|err| Error::connection("write", err.to_string()))
}

async fn read_proto<S>(stream: &mut S, closed: bool) -> Result<proto::ServerEnvelope>
where
    S: Stream<Item = std::result::Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let message = match stream.next().await {
            Some(Ok(message)) => message,
            Some(Err(error)) => {
                if closed {
                    return Err(ClosedError.into());
                }
                return Err(Error::connection("read", error.to_string()));
            }
            None => {
                if closed {
                    return Err(ClosedError.into());
                }
                return Err(Error::connection("read", "websocket closed"));
            }
        };

        match message {
            WsMessage::Binary(payload) => {
                let env = proto::ServerEnvelope::decode(payload.as_ref())
                    .map_err(|_| Error::protocol("invalid protobuf frame"))?;
                return Ok(env);
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            WsMessage::Close(_) => {
                if closed {
                    return Err(ClosedError.into());
                }
                return Err(Error::connection("read", "websocket closed"));
            }
            _ => return Err(Error::protocol("invalid protobuf frame")),
        }
    }
}

fn websocket_url(base: &str, realtime: bool) -> Result<String> {
    let mut url =
        Url::parse(base).map_err(|err| Error::validation(format!("invalid base_url: {err}")))?;
    match url.scheme() {
        "http" => {
            let _ = url.set_scheme("ws");
        }
        "https" => {
            let _ = url.set_scheme("wss");
        }
        "ws" | "wss" => {}
        other => {
            return Err(Error::validation(format!(
                "unsupported base URL scheme {other:?}"
            )))
        }
    }

    let base_path = url.path().trim_end_matches('/');
    let path = if realtime {
        "/ws/realtime"
    } else {
        "/ws/client"
    };
    if base_path.is_empty() {
        url.set_path(path);
    } else {
        url.set_path(&format!("{base_path}{path}"));
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}
fn messages_from_proto(items: &[proto::Message]) -> Result<Vec<Message>> {
    items
        .iter()
        .map(|item| message_from_proto(Some(item)))
        .collect()
}

fn attachments_from_proto(items: &[proto::Attachment]) -> Result<Vec<Attachment>> {
    items
        .iter()
        .map(|item| attachment_from_proto(Some(item)))
        .collect()
}

fn events_from_proto(items: &[proto::Event]) -> Result<Vec<Event>> {
    items
        .iter()
        .map(|item| event_from_proto(Some(item)))
        .collect()
}

fn cluster_nodes_from_proto(items: &[proto::ClusterNode]) -> Result<Vec<ClusterNode>> {
    items
        .iter()
        .map(|item| cluster_node_from_proto(Some(item)))
        .collect()
}

fn logged_in_users_from_proto(items: &[proto::LoggedInUser]) -> Result<Vec<LoggedInUser>> {
    items
        .iter()
        .map(|item| logged_in_user_from_proto(Some(item)))
        .collect()
}
