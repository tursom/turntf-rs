use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::password::PasswordInput;
use crate::store::{CursorStore, MemoryCursorStore};

/// 用户凭据，包含节点 ID、用户 ID 和密码。
///
/// 用于客户端登录认证。可通过 `Config::new()` 设置。
///
/// # 示例
///
/// ```ignore
/// use turntf::types::Credentials;
/// use turntf::password::plain_password;
///
/// let credentials = Credentials {
///     node_id: 1,
///     user_id: 42,
///     password: plain_password("secret")?,
/// };
/// ```
#[derive(Clone, Debug)]
pub struct Credentials {
    /// 节点 ID，用于标识用户所属的集群节点
    pub node_id: i64,
    /// 用户 ID，在同一节点下唯一标识一个用户
    pub user_id: i64,
    /// 密码输入，支持明文或已哈希的密码
    pub password: PasswordInput,
}

/// 用户引用，用于在 API 请求中标识一个用户。
///
/// 通过 `node_id` 和 `user_id` 唯一确定一个用户。
/// 此类型实现了 `Serialize` 和 `Deserialize`，可以用于 JSON 序列化。
///
/// # 示例
///
/// ```ignore
/// use turntf::types::UserRef;
///
/// let user = UserRef { node_id: 1, user_id: 42 };
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserRef {
    /// 用户所属的集群节点 ID
    pub node_id: i64,
    /// 用户 ID
    pub user_id: i64,
}

/// 消息游标，用于标识消息在服务器端的存储位置。
///
/// 由 `node_id`（节点 ID）和 `seq`（序列号）组成。
/// 游标用于消息确认、已读消息追踪和断线重连后的消息去重。
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageCursor {
    /// 处理此消息的节点 ID
    pub node_id: i64,
    /// 消息在该节点上的序列号
    pub seq: i64,
}

/// 会话引用，用于标识一个客户端 WebSocket 会话。
///
/// 当需要向特定会话发送数据包时使用，例如只有特定设备才能接收的消息。
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRef {
    /// 提供此会话服务的节点 ID
    pub serving_node_id: i64,
    /// 会话的唯一标识字符串
    pub session_id: String,
}

/// 数据包投递模式，控制消息（特别是瞬时消息/数据包）的投递行为。
///
/// # 变体说明
/// - `Unspecified` - 未指定模式（默认值），在发送数据包时必须显式指定有效模式
/// - `BestEffort` - 尽最大努力投递，不保证送达
/// - `RouteRetry` - 路由重试模式，在路由失败时进行重试
///
/// 使用 `as_str()` 方法可以获取模式对应的字符串标识。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryMode {
    /// 未指定投递模式（默认值）。在发送数据包时不能使用此值。
    #[default]
    Unspecified,
    /// 尽最大努力投递。数据包可能会丢失，适用于对可靠性要求不高的场景。
    BestEffort,
    /// 路由重试模式。如果初始投递失败，系统会进行路由重试。
    RouteRetry,
}

impl DeliveryMode {
    /// 返回投递模式的字符串表示。
    ///
    /// - `Unspecified` → `""`
    /// - `BestEffort` → `"best_effort"`
    /// - `RouteRetry` → `"route_retry"`
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryMode::Unspecified => "",
            DeliveryMode::BestEffort => "best_effort",
            DeliveryMode::RouteRetry => "route_retry",
        }
    }
}

/// 用户附件类型，定义了用户之间关联关系的类型。
///
/// 用于频道订阅、黑名单等功能。
///
/// # 变体说明
/// - `ChannelManager` - 频道管理员，拥有管理频道的权限
/// - `ChannelWriter` - 频道写入者，可以向频道发送消息
/// - `ChannelSubscription` - 频道订阅，表示用户订阅了某个频道
/// - `UserBlacklist` - 用户黑名单，表示用户被屏蔽
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentType {
    /// 频道管理员（默认值）
    #[default]
    ChannelManager,
    /// 频道写入者
    ChannelWriter,
    /// 频道订阅关系
    ChannelSubscription,
    /// 用户黑名单
    UserBlacklist,
}

impl AttachmentType {
    /// 返回附件类型的字符串表示。
    ///
    /// - `ChannelManager` → `"channel_manager"`
    /// - `ChannelWriter` → `"channel_writer"`
    /// - `ChannelSubscription` → `"channel_subscription"`
    /// - `UserBlacklist` → `"user_blacklist"`
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentType::ChannelManager => "channel_manager",
            AttachmentType::ChannelWriter => "channel_writer",
            AttachmentType::ChannelSubscription => "channel_subscription",
            AttachmentType::UserBlacklist => "user_blacklist",
        }
    }
}

/// 用户信息。
///
/// 表示系统中的一个用户或频道（channel）。包含用户的基本信息和元数据。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// 用户所属的节点 ID
    pub node_id: i64,
    /// 用户 ID
    pub user_id: i64,
    /// 用户名
    pub username: String,
    /// 用户角色，如 `"user"`、`"channel"` 等
    pub role: String,
    /// 用户个人资料 JSON 数据
    pub profile_json: Vec<u8>,
    /// 是否为系统保留用户
    pub system_reserved: bool,
    /// 用户创建时间
    pub created_at: String,
    /// 用户信息最后更新时间
    pub updated_at: String,
    /// 用户创建的原始节点 ID
    pub origin_node_id: i64,
    /// 用户的登录名（可为空）
    pub login_name: String,
}

/// 用户元数据，关联到特定用户的键值对数据。
///
/// 元数据支持设置过期时间（`expires_at`），过期后自动失效。
/// 值以字节数组存储，调用方负责编码和解码。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMetadata {
    /// 元数据所属的用户
    pub owner: UserRef,
    /// 元数据键名
    pub key: String,
    /// 元数据值（字节数组）
    pub value: Vec<u8>,
    /// 最后更新时间
    pub updated_at: String,
    /// 删除时间（为空表示未删除）
    pub deleted_at: String,
    /// 过期时间（为空表示永不过期）
    pub expires_at: String,
    /// 创建此元数据的原始节点 ID
    pub origin_node_id: i64,
}

/// 用户元数据扫描结果。
///
/// 当通过 `scan_user_metadata` 批量查询元数据时返回的结果。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMetadataScanResult {
    /// 扫描到的元数据列表
    pub items: Vec<UserMetadata>,
    /// 返回的结果数量
    pub count: i32,
    /// 下一页的游标值，可用于后续扫描的分页参数
    pub next_after: String,
}

/// 消息，表示用户之间传递的持久化消息。
///
/// 消息会被持久化存储，确保可靠投递。每条消息由 `node_id` 和 `seq` 唯一标识。
///
/// 通过 `cursor()` 方法可以获取消息的游标，用于消息确认和去重。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// 消息接收者
    pub recipient: UserRef,
    /// 处理此消息的节点 ID
    pub node_id: i64,
    /// 消息在该节点上的序列号
    pub seq: i64,
    /// 消息发送者
    pub sender: UserRef,
    /// 消息体内容（字节数组）
    pub body: Vec<u8>,
    /// 消息创建的混合逻辑时钟（HLC）时间戳
    pub created_at_hlc: String,
}

impl Message {
    /// 获取此消息的游标。
    ///
    /// 游标由 `node_id` 和 `seq` 组成，用于唯一标识消息在服务器端的位置。
    pub fn cursor(&self) -> MessageCursor {
        MessageCursor {
            node_id: self.node_id,
            seq: self.seq,
        }
    }
}

/// 数据包（瞬时消息），用于实时通信场景。
///
/// 与 `Message` 不同，数据包不会被持久化存储（瞬时消息），适用于对可靠性要求不高的
/// 实时数据传输，如聊天应用中的"正在输入"提示、实时位置更新等。
///
/// 数据包支持指定目标会话和投递模式。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packet {
    /// 数据包 ID
    pub packet_id: u64,
    /// 源节点 ID（发送方所在节点）
    pub source_node_id: i64,
    /// 目标节点 ID（接收方所在节点）
    pub target_node_id: i64,
    /// 接收者引用
    pub recipient: UserRef,
    /// 发送者引用
    pub sender: UserRef,
    /// 数据包体内容（字节数组）
    pub body: Vec<u8>,
    /// 投递模式
    pub delivery_mode: DeliveryMode,
    /// 目标会话（如果指定，只投递到该会话）
    pub target_session: Option<SessionRef>,
}

/// 数据包中转接受确认，表示服务器已接受数据包转发请求。
///
/// 当使用 `send_packet` 发送瞬时消息时，服务器返回此结构表示已成功接收转发请求。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayAccepted {
    /// 数据包 ID
    pub packet_id: u64,
    /// 源节点 ID
    pub source_node_id: i64,
    /// 目标节点 ID
    pub target_node_id: i64,
    /// 接收者引用
    pub recipient: UserRef,
    /// 使用的投递模式
    pub delivery_mode: DeliveryMode,
    /// 目标会话（如果指定）
    pub target_session: Option<SessionRef>,
}

/// 用户附件，表示用户之间的某种关联关系。
///
/// 附件用于实现频道订阅、黑名单、频道管理员和频道写入者等功能。
/// 每个附件包含所有者、主题用户、附件类型和配置信息。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// 附件所有者
    pub owner: UserRef,
    /// 附件主题用户（关联的目标用户）
    pub subject: UserRef,
    /// 附件类型
    pub attachment_type: AttachmentType,
    /// 附件的 JSON 配置信息
    pub config_json: Vec<u8>,
    /// 附件创建时间
    pub attached_at: String,
    /// 附件删除时间（为空表示未删除）
    pub deleted_at: String,
    /// 创建此附件的原始节点 ID
    pub origin_node_id: i64,
}

/// 频道订阅信息，表示用户订阅了一个频道。
///
/// 用户订阅频道后可以收到该频道发送的消息。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    /// 订阅者（用户）
    pub subscriber: UserRef,
    /// 被订阅的频道
    pub channel: UserRef,
    /// 订阅时间
    pub subscribed_at: String,
    /// 取消订阅时间（为空表示仍处于订阅状态）
    pub deleted_at: String,
    /// 创建此订阅的原始节点 ID
    pub origin_node_id: i64,
}

/// 黑名单条目，表示用户屏蔽了另一个用户。
///
/// 处于黑名单中的用户将无法向屏蔽者发送消息。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlacklistEntry {
    /// 黑名单所有者
    pub owner: UserRef,
    /// 被屏蔽的用户
    pub blocked: UserRef,
    /// 屏蔽时间
    pub blocked_at: String,
    /// 解除屏蔽时间（为空表示仍处于屏蔽状态）
    pub deleted_at: String,
    /// 创建此黑名单条目的原始节点 ID
    pub origin_node_id: i64,
}

/// 事件，系统中的操作记录。
///
/// 事件日志记录了系统中发生的各种操作，如用户创建、消息发送等。
/// 每个事件有唯一的 `event_id` 和顺序递增的 `sequence`。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// 事件顺序号（全局递增）
    pub sequence: i64,
    /// 事件 ID（唯一标识）
    pub event_id: i64,
    /// 事件类型，如 `"user_created"`、`"message_sent"` 等
    pub event_type: String,
    /// 聚合类型，表示事件所属的领域对象类型
    pub aggregate: String,
    /// 聚合对象所属的节点 ID
    pub aggregate_node_id: i64,
    /// 聚合对象 ID
    pub aggregate_id: i64,
    /// 事件的 HLC 时间戳
    pub hlc: String,
    /// 事件产生的原始节点 ID
    pub origin_node_id: i64,
    /// 事件的 JSON 详细数据
    pub event_json: Vec<u8>,
}

/// 集群节点信息，表示集群中的一个节点。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterNode {
    /// 节点 ID
    pub node_id: i64,
    /// 是否为本地节点
    pub is_local: bool,
    /// 节点的配置 URL 地址
    pub configured_url: String,
    /// 节点的发现来源
    pub source: String,
}

/// 已登录用户信息，表示当前在某节点上登录的用户。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedInUser {
    /// 用户登录的节点 ID
    pub node_id: i64,
    /// 用户 ID
    pub user_id: i64,
    /// 用户名
    pub username: String,
    /// 用户的登录名
    pub login_name: String,
}

/// 在线节点在线状态，表示用户在某个节点上的在线情况。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineNodePresence {
    /// 提供服务的节点 ID
    pub serving_node_id: i64,
    /// 该节点上的会话数
    pub session_count: i32,
    /// 传输层提示信息（如 WebSocket、HTTP 等）
    pub transport_hint: String,
}

/// 已解析的会话信息。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSession {
    /// 会话引用
    pub session: SessionRef,
    /// 传输层协议类型
    pub transport: String,
    /// 是否支持瞬时消息
    pub transient_capable: bool,
}

/// 用户会话解析结果，包含用户的在线状态和所有活跃会话。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedUserSessions {
    /// 目标用户
    pub user: UserRef,
    /// 用户在各节点上的在线状态列表
    pub presence: Vec<OnlineNodePresence>,
    /// 用户的所有活跃会话
    pub sessions: Vec<ResolvedSession>,
}

/// 消息修剪状态，表示服务器端消息清理的情况。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTrimStatus {
    /// 已修剪（清理）的消息总数
    pub trimmed_total: i64,
    /// 最后一次修剪的时间
    pub last_trimmed_at: String,
}

/// 事件日志修剪状态，表示服务器端事件日志清理的情况。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLogTrimStatus {
    /// 已修剪的事件总数
    pub trimmed_total: i64,
    /// 最后一次修剪的时间
    pub last_trimmed_at: String,
}

/// 投影状态，表示事件投影的处理进度。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionStatus {
    /// 待处理的事件总数
    pub pending_total: i64,
    /// 最近一次投影失败的时间
    pub last_failed_at: String,
}

/// 对等节点起源状态，表示集群中对等节点间事件同步的进度。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerOriginStatus {
    /// 起源节点 ID
    pub origin_node_id: i64,
    /// 已确认的最大事件 ID
    pub acked_event_id: i64,
    /// 已应用的最大事件 ID
    pub applied_event_id: i64,
    /// 未确认的事件数
    pub unconfirmed_events: i64,
    /// 游标更新时间
    pub cursor_updated_at: String,
    /// 远程最新事件 ID
    pub remote_last_event_id: u64,
    /// 是否在追赶同步中
    pub pending_catchup: bool,
}

/// 对等节点状态，表示集群中另一个节点的详细连接和同步状态。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerStatus {
    /// 对等节点 ID
    pub node_id: i64,
    /// 节点配置的 URL
    pub configured_url: String,
    /// 发现来源
    pub source: String,
    /// 自动发现的 URL
    pub discovered_url: String,
    /// 发现状态
    pub discovery_state: String,
    /// 最近一次发现的时间
    pub last_discovered_at: String,
    /// 最近一次成功连接的时间
    pub last_connected_at: String,
    /// 最近一次发现错误的详细信息
    pub last_discovery_error: String,
    /// 当前是否已连接
    pub connected: bool,
    /// 会话方向
    pub session_direction: String,
    /// 各起源节点的同步状态
    pub origins: Vec<PeerOriginStatus>,
    /// 待处理的快照分区数
    pub pending_snapshot_partitions: i32,
    /// 远程快照版本
    pub remote_snapshot_version: String,
    /// 远程消息窗口大小
    pub remote_message_window_size: i32,
    /// 时钟偏移量（毫秒）
    pub clock_offset_ms: i64,
    /// 最近一次时钟同步时间
    pub last_clock_sync: String,
    /// 已发送的快照摘要总数
    pub snapshot_digests_sent_total: u64,
    /// 已接收的快照摘要总数
    pub snapshot_digests_received_total: u64,
    /// 已发送的快照块总数
    pub snapshot_chunks_sent_total: u64,
    /// 已接收的快照块总数
    pub snapshot_chunks_received_total: u64,
    /// 最近一次快照摘要发送时间
    pub last_snapshot_digest_at: String,
    /// 最近一次快照块发送时间
    pub last_snapshot_chunk_at: String,
}

/// 操作状态，表示集群节点当前的运行状态和健康情况。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationsStatus {
    /// 节点 ID
    pub node_id: i64,
    /// 消息窗口大小
    pub message_window_size: i32,
    /// 最近的事件序列号
    pub last_event_sequence: i64,
    /// 写入门是否就绪
    pub write_gate_ready: bool,
    /// 冲突总数
    pub conflict_total: i64,
    /// 消息修剪状态
    pub message_trim: MessageTrimStatus,
    /// 投影状态
    pub projection: ProjectionStatus,
    /// 各对等节点的状态
    pub peers: Vec<PeerStatus>,
    /// 事件日志修剪状态（可选）
    pub event_log_trim: Option<EventLogTrimStatus>,
}

/// 删除用户操作的结果。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteUserResult {
    /// 操作状态，如 `"deleted"`、`"not_found"` 等
    pub status: String,
    /// 被删除的用户引用
    pub user: UserRef,
}

/// 登录信息，在客户端成功登录后返回。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginInfo {
    /// 登录用户的信息
    pub user: User,
    /// 服务器协议版本
    pub protocol_version: String,
    /// 当前会话引用
    pub session_ref: SessionRef,
}

/// 创建用户的请求参数。
#[derive(Clone, Debug, Default)]
pub struct CreateUserRequest {
    /// 用户名（必填）
    pub username: String,
    /// 登录名（可选，用于支持密码登录的用户）
    pub login_name: Option<String>,
    /// 密码（可选，用于支持密码登录的用户）
    pub password: Option<PasswordInput>,
    /// 用户个人资料 JSON 数据
    pub profile_json: Vec<u8>,
    /// 用户角色，如 `"user"`（普通用户）或 `"channel"`（频道）（必填）
    pub role: String,
}

/// 更新用户的请求参数。
///
/// 所有字段均为可选，只更新提供的字段。
#[derive(Clone, Debug, Default)]
pub struct UpdateUserRequest {
    /// 新的用户名
    pub username: Option<String>,
    /// 新的登录名
    pub login_name: Option<String>,
    /// 新的密码
    pub password: Option<PasswordInput>,
    /// 新的个人资料 JSON 数据
    pub profile_json: Option<Vec<u8>>,
    /// 新的用户角色
    pub role: Option<String>,
}

/// 创建或更新用户元数据的请求参数。
#[derive(Clone, Debug, Default)]
pub struct UpsertUserMetadataRequest {
    /// 元数据的值（字节数组）
    pub value: Vec<u8>,
    /// 过期时间（可选）。为空表示永不过期。
    pub expires_at: Option<String>,
}

/// 扫描用户元数据的请求参数。
#[derive(Clone, Debug, Default)]
pub struct ScanUserMetadataRequest {
    /// 键前缀过滤条件，只返回匹配前缀的元数据
    pub prefix: String,
    /// 分页游标，从指定位置开始扫描
    pub after: String,
    /// 返回结果的最大数量（不超过 1000）
    pub limit: i32,
}

/// 客户端配置的默认值。
///
/// 此结构体定义了 `Config` 中各配置项的默认值，通过 `Default` trait 提供默认实现。
#[derive(Clone)]
pub struct ClientConfigDefaults {
    /// 是否启用自动重连（默认：`true`）
    pub reconnect: bool,
    /// 初始重连延迟（默认：1 秒）
    pub initial_reconnect_delay: Duration,
    /// 最大重连延迟（默认：30 秒），重连延迟会以指数退避方式增长至此上限
    pub max_reconnect_delay: Duration,
    /// WebSocket 心跳 ping 的间隔时间（默认：30 秒）
    pub ping_interval: Duration,
    /// RPC 请求超时时间（默认：10 秒）
    pub request_timeout: Duration,
    /// 是否自动确认已接收的消息（默认：`true`）
    pub ack_messages: bool,
    /// 是否仅接收瞬时消息（默认：`false`）
    pub transient_only: bool,
    /// 是否连接到实时流端点（默认：`false`）
    pub realtime_stream: bool,
    /// 事件通道的缓冲容量（默认：256）
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

/// 创建默认的游标存储实例（基于内存的实现）。
pub fn default_cursor_store() -> std::sync::Arc<dyn CursorStore> {
    std::sync::Arc::new(MemoryCursorStore::new())
}

// ===== Relay 类型 =====

/// RelayConnection 的可靠性等级。
///
/// - `BestEffort` — 无 ACK，无重传，无去重，无排序。延迟最低，适合实时音视频帧。
/// - `AtLeastOnce` — ACK + 重传，不保证去重和排序。适合幂等指令。
/// - `ReliableOrdered` — ACK + 重传 + 去重 + 严格有序。适合文件传输和聊天消息。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reliability {
    /// 尽最大努力，无 ACK / 重传 / 去重 / 排序
    BestEffort = 0,
    /// ACK + 重传，不保证去重和排序
    #[default]
    AtLeastOnce = 1,
    /// ACK + 重传 + 去重 + 严格有序
    ReliableOrdered = 2,
}

/// RelayConnection 的当前状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RelayState {
    /// 初始状态或已关闭
    #[default]
    Closed = 0,
    /// 已发送 OPEN，等待 OPEN_ACK
    Opening = 1,
    /// 连接已建立，可收发数据
    Open = 2,
    /// 已发送 CLOSE，等待确认
    Closing = 3,
}

/// Relay 协议帧的类型枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayKind {
    Unspecified = 0,
    Open = 1,
    OpenAck = 2,
    Data = 3,
    Ack = 4,
    Close = 5,
    Ping = 6,
    Error = 7,
}

/// RelayConnection 的配置。
///
/// 使用 `RelayConfig::builder()` 创建配置实例。
#[derive(Clone, Debug)]
pub struct RelayConfig {
    /// 可靠性等级，默认 `ReliableOrdered`。
    pub reliability: Reliability,
    /// 发送窗口大小（在途未确认帧数上限），范围 1-256，默认 16。
    /// BestEffort 模式下忽略此配置。
    pub window_size: u64,
    /// OPEN 等待 OPEN_ACK 超时毫秒数，默认 10000。
    pub open_timeout_ms: u64,
    /// CLOSE 等待确认超时毫秒数，默认 5000。
    pub close_timeout_ms: u64,
    /// DATA 等待 ACK 超时毫秒数，默认 3000。
    /// BestEffort 模式下忽略此配置。
    pub ack_timeout_ms: u64,
    /// 最大重传次数，默认 5。
    /// BestEffort 模式下忽略此配置。
    pub max_retransmits: u32,
    /// 无数据超时断开毫秒数，0 表示不超时（默认）。
    pub idle_timeout_ms: u64,
    /// Send 操作超时毫秒数（缓冲区满时等待上限），0 表示不超时（默认）。
    pub send_timeout_ms: u64,
    /// Receive 操作超时毫秒数（无数据等待上限），0 表示不超时（默认）。
    pub receive_timeout_ms: u64,
    /// 发送缓冲区字节数，默认 65536。
    pub send_buffer_size: usize,
    /// Packet 投递模式，默认 `RouteRetry`。
    pub delivery_mode: DeliveryMode,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            reliability: Reliability::ReliableOrdered,
            window_size: 16,
            open_timeout_ms: 10000,
            close_timeout_ms: 5000,
            ack_timeout_ms: 3000,
            max_retransmits: 5,
            idle_timeout_ms: 0,
            send_timeout_ms: 0,
            receive_timeout_ms: 0,
            send_buffer_size: 65536,
            delivery_mode: DeliveryMode::RouteRetry,
        }
    }
}

impl RelayConfig {
    /// 返回可链式调用的 Builder。
    pub fn builder() -> RelayConfigBuilder {
        RelayConfigBuilder::default()
    }
}

/// RelayConfig 的 Builder。
#[derive(Clone, Debug, Default)]
pub struct RelayConfigBuilder {
    config: RelayConfig,
}

impl RelayConfigBuilder {
    /// 设置可靠性等级。
    pub fn reliability(mut self, v: Reliability) -> Self {
        self.config.reliability = v;
        self
    }

    /// 设置发送窗口大小。
    pub fn window_size(mut self, v: u64) -> Self {
        self.config.window_size = v;
        self
    }

    /// 设置 OPEN 超时毫秒数。
    pub fn open_timeout_ms(mut self, v: u64) -> Self {
        self.config.open_timeout_ms = v;
        self
    }

    /// 设置 ACK 超时毫秒数。
    pub fn ack_timeout_ms(mut self, v: u64) -> Self {
        self.config.ack_timeout_ms = v;
        self
    }

    /// 设置最大重传次数。
    pub fn max_retransmits(mut self, v: u32) -> Self {
        self.config.max_retransmits = v;
        self
    }

    /// 设置空闲超时（毫秒），0 表示不超时。
    pub fn idle_timeout_ms(mut self, v: u64) -> Self {
        self.config.idle_timeout_ms = v;
        self
    }

    /// 设置发送超时毫秒数，0 表示不超时。
    pub fn send_timeout_ms(mut self, v: u64) -> Self {
        self.config.send_timeout_ms = v;
        self
    }

    /// 设置接收超时毫秒数，0 表示不超时。
    pub fn receive_timeout_ms(mut self, v: u64) -> Self {
        self.config.receive_timeout_ms = v;
        self
    }

    /// 设置 Packet 投递模式。
    pub fn delivery_mode(mut self, v: DeliveryMode) -> Self {
        self.config.delivery_mode = v;
        self
    }

    /// 构建最终的 RelayConfig。
    pub fn build(self) -> RelayConfig {
        self.config
    }
}

/// Relay 协议帧，与 proto RelayEnvelope 对应。
#[derive(Clone, Debug)]
pub struct RelayEnvelope {
    pub relay_id: String,
    pub kind: RelayKind,
    pub sender_session: SessionRef,
    pub target_session: SessionRef,
    pub seq: u64,
    pub ack_seq: u64,
    pub payload: Vec<u8>,
    pub sent_at_ms: i64,
}

/// Relay 层的错误。
#[derive(Clone, Debug)]
pub struct RelayError {
    pub code: String,
    pub message: String,
}

impl RelayError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "relay: {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RelayError {}

// 预定义的 relay 错误码
pub const RELAY_ERROR_OPEN_TIMEOUT: &str = "open_timeout";
pub const RELAY_ERROR_ACK_TIMEOUT: &str = "ack_timeout";
pub const RELAY_ERROR_MAX_RETRANSMIT: &str = "max_retransmit";
pub const RELAY_ERROR_IDLE_TIMEOUT: &str = "idle_timeout";
pub const RELAY_ERROR_REMOTE_CLOSE: &str = "remote_close";
pub const RELAY_ERROR_CLIENT_CLOSED: &str = "client_closed";
pub const RELAY_ERROR_PROTOCOL: &str = "protocol_error";
pub const RELAY_ERROR_DUPLICATE_OPEN: &str = "duplicate_open";
pub const RELAY_ERROR_NOT_CONNECTED: &str = "not_connected";
pub const RELAY_ERROR_SEND_TIMEOUT: &str = "send_timeout";
pub const RELAY_ERROR_RECEIVE_TIMEOUT: &str = "receive_timeout";
