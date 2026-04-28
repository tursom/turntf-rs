use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::password::PasswordInput;
use crate::store::{CursorStore, MemoryCursorStore};

#[derive(Clone, Debug)]
pub struct Credentials {
    pub node_id: i64,
    pub user_id: i64,
    pub password: PasswordInput,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserRef {
    pub node_id: i64,
    pub user_id: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageCursor {
    pub node_id: i64,
    pub seq: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRef {
    pub serving_node_id: i64,
    pub session_id: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryMode {
    #[default]
    Unspecified,
    BestEffort,
    RouteRetry,
}

impl DeliveryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryMode::Unspecified => "",
            DeliveryMode::BestEffort => "best_effort",
            DeliveryMode::RouteRetry => "route_retry",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentType {
    #[default]
    ChannelManager,
    ChannelWriter,
    ChannelSubscription,
    UserBlacklist,
}

impl AttachmentType {
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentType::ChannelManager => "channel_manager",
            AttachmentType::ChannelWriter => "channel_writer",
            AttachmentType::ChannelSubscription => "channel_subscription",
            AttachmentType::UserBlacklist => "user_blacklist",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub node_id: i64,
    pub user_id: i64,
    pub username: String,
    pub role: String,
    pub profile_json: Vec<u8>,
    pub system_reserved: bool,
    pub created_at: String,
    pub updated_at: String,
    pub origin_node_id: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub recipient: UserRef,
    pub node_id: i64,
    pub seq: i64,
    pub sender: UserRef,
    pub body: Vec<u8>,
    pub created_at_hlc: String,
}

impl Message {
    pub fn cursor(&self) -> MessageCursor {
        MessageCursor {
            node_id: self.node_id,
            seq: self.seq,
        }
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
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginInfo {
    pub user: User,
    pub protocol_version: String,
    pub session_ref: SessionRef,
}

#[derive(Clone, Debug, Default)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: Option<PasswordInput>,
    pub profile_json: Vec<u8>,
    pub role: String,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub password: Option<PasswordInput>,
    pub profile_json: Option<Vec<u8>>,
    pub role: Option<String>,
}

#[derive(Clone)]
pub struct ClientConfigDefaults {
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

impl Default for ClientConfigDefaults {
    fn default() -> Self {
        Self {
            reconnect: true,
            initial_reconnect_delay: Duration::from_secs(1),
            max_reconnect_delay: Duration::from_secs(30),
            ping_interval: Duration::from_secs(30),
            request_timeout: Duration::from_secs(10),
            ack_messages: true,
            transient_only: false,
            realtime_stream: false,
            event_channel_capacity: 256,
        }
    }
}

pub fn default_cursor_store() -> std::sync::Arc<dyn CursorStore> {
    std::sync::Arc::new(MemoryCursorStore::new())
}
