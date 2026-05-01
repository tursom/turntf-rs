use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{Map, Value};

use crate::errors::{Error, Result};
use crate::proto;
use crate::types::{
    Attachment, AttachmentType, BlacklistEntry, ClusterNode, DeleteUserResult, DeliveryMode, Event,
    EventLogTrimStatus, LoggedInUser, LoginInfo, Message, MessageCursor, MessageTrimStatus,
    OnlineNodePresence, OperationsStatus, Packet, PeerOriginStatus, PeerStatus, ProjectionStatus,
    RelayAccepted, ResolvedSession, ResolvedUserSessions, SessionRef, Subscription, User,
    UserMetadata, UserMetadataScanResult, UserRef,
};

pub(crate) fn delivery_mode_to_proto(mode: DeliveryMode) -> i32 {
    match mode {
        DeliveryMode::Unspecified => proto::ClientDeliveryMode::Unspecified as i32,
        DeliveryMode::BestEffort => proto::ClientDeliveryMode::BestEffort as i32,
        DeliveryMode::RouteRetry => proto::ClientDeliveryMode::RouteRetry as i32,
    }
}

pub(crate) fn delivery_mode_from_proto(mode: i32) -> DeliveryMode {
    match proto::ClientDeliveryMode::try_from(mode).ok() {
        Some(proto::ClientDeliveryMode::BestEffort) => DeliveryMode::BestEffort,
        Some(proto::ClientDeliveryMode::RouteRetry) => DeliveryMode::RouteRetry,
        _ => DeliveryMode::Unspecified,
    }
}

pub(crate) fn attachment_type_to_proto(attachment_type: AttachmentType) -> i32 {
    match attachment_type {
        AttachmentType::ChannelManager => proto::AttachmentType::ChannelManager as i32,
        AttachmentType::ChannelWriter => proto::AttachmentType::ChannelWriter as i32,
        AttachmentType::ChannelSubscription => proto::AttachmentType::ChannelSubscription as i32,
        AttachmentType::UserBlacklist => proto::AttachmentType::UserBlacklist as i32,
    }
}

pub(crate) fn attachment_type_from_proto(attachment_type: i32) -> Result<AttachmentType> {
    match proto::AttachmentType::try_from(attachment_type).ok() {
        Some(proto::AttachmentType::ChannelManager) => Ok(AttachmentType::ChannelManager),
        Some(proto::AttachmentType::ChannelWriter) => Ok(AttachmentType::ChannelWriter),
        Some(proto::AttachmentType::ChannelSubscription) => Ok(AttachmentType::ChannelSubscription),
        Some(proto::AttachmentType::UserBlacklist) => Ok(AttachmentType::UserBlacklist),
        _ => Err(Error::protocol(format!(
            "unsupported attachment type {attachment_type}"
        ))),
    }
}

pub(crate) fn user_ref_to_proto(value: &UserRef) -> proto::UserRef {
    proto::UserRef {
        node_id: value.node_id,
        user_id: value.user_id,
    }
}

pub(crate) fn cursor_to_proto(value: &MessageCursor) -> proto::MessageCursor {
    proto::MessageCursor {
        node_id: value.node_id,
        seq: value.seq,
    }
}

pub(crate) fn session_ref_to_proto(value: &SessionRef) -> proto::SessionRef {
    proto::SessionRef {
        serving_node_id: value.serving_node_id,
        session_id: value.session_id.clone(),
    }
}

pub(crate) fn user_ref_from_proto(value: Option<&proto::UserRef>) -> UserRef {
    value.map_or_else(UserRef::default, |value| UserRef {
        node_id: value.node_id,
        user_id: value.user_id,
    })
}

pub(crate) fn session_ref_from_proto(value: Option<&proto::SessionRef>) -> Result<SessionRef> {
    let value = value.ok_or_else(|| Error::protocol("missing session_ref"))?;
    if value.serving_node_id <= 0 {
        return Err(Error::protocol("invalid session_ref.serving_node_id"));
    }
    if value.session_id.is_empty() {
        return Err(Error::protocol("invalid session_ref.session_id"));
    }
    Ok(SessionRef {
        serving_node_id: value.serving_node_id,
        session_id: value.session_id.clone(),
    })
}

pub(crate) fn optional_session_ref_from_proto(
    value: Option<&proto::SessionRef>,
) -> Result<Option<SessionRef>> {
    value
        .map(|value| session_ref_from_proto(Some(value)))
        .transpose()
}

pub(crate) fn user_from_proto(value: Option<&proto::User>) -> Result<User> {
    let value = value.ok_or_else(|| Error::protocol("missing user"))?;
    Ok(User {
        node_id: value.node_id,
        user_id: value.user_id,
        username: value.username.clone(),
        role: value.role.clone(),
        profile_json: value.profile_json.to_vec(),
        system_reserved: value.system_reserved,
        created_at: value.created_at.clone(),
        updated_at: value.updated_at.clone(),
        origin_node_id: value.origin_node_id,
        login_name: value.login_name.clone(),
    })
}

pub(crate) fn user_metadata_from_proto(
    value: Option<&proto::UserMetadata>,
) -> Result<UserMetadata> {
    let value = value.ok_or_else(|| Error::protocol("missing user_metadata"))?;
    Ok(UserMetadata {
        owner: user_ref_from_proto(value.owner.as_ref()),
        key: value.key.clone(),
        value: value.value.to_vec(),
        updated_at: value.updated_at.clone(),
        deleted_at: value.deleted_at.clone(),
        expires_at: value.expires_at.clone(),
        origin_node_id: value.origin_node_id,
    })
}

pub(crate) fn user_metadata_scan_result_from_proto(
    value: &proto::ScanUserMetadataResponse,
) -> Result<UserMetadataScanResult> {
    Ok(UserMetadataScanResult {
        items: value
            .items
            .iter()
            .map(|item| user_metadata_from_proto(Some(item)))
            .collect::<Result<Vec<_>>>()?,
        count: value.count,
        next_after: value.next_after.clone(),
    })
}

pub(crate) fn message_from_proto(value: Option<&proto::Message>) -> Result<Message> {
    let value = value.ok_or_else(|| Error::protocol("missing message"))?;
    Ok(Message {
        recipient: user_ref_from_proto(value.recipient.as_ref()),
        node_id: value.node_id,
        seq: value.seq,
        sender: user_ref_from_proto(value.sender.as_ref()),
        body: value.body.to_vec(),
        created_at_hlc: value.created_at_hlc.clone(),
    })
}

pub(crate) fn packet_from_proto(value: Option<&proto::Packet>) -> Result<Packet> {
    let value = value.ok_or_else(|| Error::protocol("missing packet"))?;
    Ok(Packet {
        packet_id: value.packet_id,
        source_node_id: value.source_node_id,
        target_node_id: value.target_node_id,
        recipient: user_ref_from_proto(value.recipient.as_ref()),
        sender: user_ref_from_proto(value.sender.as_ref()),
        body: value.body.to_vec(),
        delivery_mode: delivery_mode_from_proto(value.delivery_mode),
        target_session: optional_session_ref_from_proto(value.target_session.as_ref())?,
    })
}

pub(crate) fn relay_accepted_from_proto(
    value: Option<&proto::TransientAccepted>,
) -> Result<RelayAccepted> {
    let value = value.ok_or_else(|| Error::protocol("missing transient_accepted"))?;
    Ok(RelayAccepted {
        packet_id: value.packet_id,
        source_node_id: value.source_node_id,
        target_node_id: value.target_node_id,
        recipient: user_ref_from_proto(value.recipient.as_ref()),
        delivery_mode: delivery_mode_from_proto(value.delivery_mode),
        target_session: optional_session_ref_from_proto(value.target_session.as_ref())?,
    })
}

pub(crate) fn online_node_presence_from_proto(
    value: &proto::OnlineNodePresence,
) -> OnlineNodePresence {
    OnlineNodePresence {
        serving_node_id: value.serving_node_id,
        session_count: value.session_count,
        transport_hint: value.transport_hint.clone(),
    }
}

pub(crate) fn resolved_session_from_proto(
    value: &proto::ResolvedSession,
) -> Result<ResolvedSession> {
    Ok(ResolvedSession {
        session: session_ref_from_proto(value.session.as_ref())?,
        transport: value.transport.clone(),
        transient_capable: value.transient_capable,
    })
}

pub(crate) fn resolved_user_sessions_from_proto(
    value: &proto::ResolveUserSessionsResponse,
) -> Result<ResolvedUserSessions> {
    Ok(ResolvedUserSessions {
        user: user_ref_from_proto(value.user.as_ref()),
        presence: value
            .presence
            .iter()
            .map(online_node_presence_from_proto)
            .collect(),
        sessions: value
            .items
            .iter()
            .map(resolved_session_from_proto)
            .collect::<Result<Vec<_>>>()?,
    })
}

pub(crate) fn attachment_from_proto(value: Option<&proto::Attachment>) -> Result<Attachment> {
    let value = value.ok_or_else(|| Error::protocol("missing attachment"))?;
    Ok(Attachment {
        owner: user_ref_from_proto(value.owner.as_ref()),
        subject: user_ref_from_proto(value.subject.as_ref()),
        attachment_type: attachment_type_from_proto(value.attachment_type)?,
        config_json: value.config_json.to_vec(),
        attached_at: value.attached_at.clone(),
        deleted_at: value.deleted_at.clone(),
        origin_node_id: value.origin_node_id,
    })
}

pub(crate) fn subscription_from_attachment(value: &Attachment) -> Subscription {
    Subscription {
        subscriber: value.owner.clone(),
        channel: value.subject.clone(),
        subscribed_at: value.attached_at.clone(),
        deleted_at: value.deleted_at.clone(),
        origin_node_id: value.origin_node_id,
    }
}

pub(crate) fn blacklist_entry_from_attachment(value: &Attachment) -> BlacklistEntry {
    BlacklistEntry {
        owner: value.owner.clone(),
        blocked: value.subject.clone(),
        blocked_at: value.attached_at.clone(),
        deleted_at: value.deleted_at.clone(),
        origin_node_id: value.origin_node_id,
    }
}

pub(crate) fn subscription_from_proto(value: Option<&proto::Attachment>) -> Result<Subscription> {
    Ok(subscription_from_attachment(&attachment_from_proto(value)?))
}

pub(crate) fn blacklist_entry_from_proto(
    value: Option<&proto::Attachment>,
) -> Result<BlacklistEntry> {
    let value = attachment_from_proto(value)?;
    Ok(BlacklistEntry {
        owner: value.owner,
        blocked: value.subject,
        blocked_at: value.attached_at,
        deleted_at: value.deleted_at,
        origin_node_id: value.origin_node_id,
    })
}

pub(crate) fn event_from_proto(value: Option<&proto::Event>) -> Result<Event> {
    let value = value.ok_or_else(|| Error::protocol("missing event"))?;
    Ok(Event {
        sequence: value.sequence,
        event_id: value.event_id,
        event_type: value.event_type.clone(),
        aggregate: value.aggregate.clone(),
        aggregate_node_id: value.aggregate_node_id,
        aggregate_id: value.aggregate_id,
        hlc: value.hlc.clone(),
        origin_node_id: value.origin_node_id,
        event_json: value.event_json.to_vec(),
    })
}

pub(crate) fn cluster_node_from_proto(value: Option<&proto::ClusterNode>) -> Result<ClusterNode> {
    let value = value.ok_or_else(|| Error::protocol("missing cluster node"))?;
    Ok(ClusterNode {
        node_id: value.node_id,
        is_local: value.is_local,
        configured_url: value.configured_url.clone(),
        source: value.source.clone(),
    })
}

pub(crate) fn logged_in_user_from_proto(
    value: Option<&proto::LoggedInUser>,
) -> Result<LoggedInUser> {
    let value = value.ok_or_else(|| Error::protocol("missing logged-in user"))?;
    Ok(LoggedInUser {
        node_id: value.node_id,
        user_id: value.user_id,
        username: value.username.clone(),
        login_name: value.login_name.clone(),
    })
}

pub(crate) fn operations_status_from_proto(
    value: Option<&proto::OperationsStatus>,
) -> Result<OperationsStatus> {
    let value = value.ok_or_else(|| Error::protocol("missing operations status"))?;
    Ok(OperationsStatus {
        node_id: value.node_id,
        message_window_size: value.message_window_size,
        last_event_sequence: value.last_event_sequence,
        write_gate_ready: value.write_gate_ready,
        conflict_total: value.conflict_total,
        message_trim: message_trim_status_from_proto(value.message_trim.as_ref()),
        projection: projection_status_from_proto(value.projection.as_ref()),
        peers: value
            .peers
            .iter()
            .map(peer_status_from_proto)
            .collect::<Result<Vec<_>>>()?,
        event_log_trim: value
            .event_log_trim
            .as_ref()
            .map(event_log_trim_status_from_proto)
            .transpose()?,
    })
}

pub(crate) fn message_trim_status_from_proto(
    value: Option<&proto::MessageTrimStatus>,
) -> MessageTrimStatus {
    value.map_or_else(MessageTrimStatus::default, |value| MessageTrimStatus {
        trimmed_total: value.trimmed_total,
        last_trimmed_at: value.last_trimmed_at.clone(),
    })
}

pub(crate) fn event_log_trim_status_from_proto(
    value: &proto::EventLogTrimStatus,
) -> Result<EventLogTrimStatus> {
    Ok(EventLogTrimStatus {
        trimmed_total: value.trimmed_total,
        last_trimmed_at: value.last_trimmed_at.clone(),
    })
}

pub(crate) fn projection_status_from_proto(
    value: Option<&proto::ProjectionStatus>,
) -> ProjectionStatus {
    value.map_or_else(ProjectionStatus::default, |value| ProjectionStatus {
        pending_total: value.pending_total,
        last_failed_at: value.last_failed_at.clone(),
    })
}

pub(crate) fn peer_origin_status_from_proto(
    value: &proto::PeerOriginStatus,
) -> Result<PeerOriginStatus> {
    Ok(PeerOriginStatus {
        origin_node_id: value.origin_node_id,
        acked_event_id: value.acked_event_id,
        applied_event_id: value.applied_event_id,
        unconfirmed_events: value.unconfirmed_events,
        cursor_updated_at: value.cursor_updated_at.clone(),
        remote_last_event_id: value.remote_last_event_id,
        pending_catchup: value.pending_catchup,
    })
}

pub(crate) fn peer_status_from_proto(value: &proto::PeerStatus) -> Result<PeerStatus> {
    Ok(PeerStatus {
        node_id: value.node_id,
        configured_url: value.configured_url.clone(),
        source: value.source.clone(),
        discovered_url: value.discovered_url.clone(),
        discovery_state: value.discovery_state.clone(),
        last_discovered_at: value.last_discovered_at.clone(),
        last_connected_at: value.last_connected_at.clone(),
        last_discovery_error: value.last_discovery_error.clone(),
        connected: value.connected,
        session_direction: value.session_direction.clone(),
        origins: value
            .origins
            .iter()
            .map(peer_origin_status_from_proto)
            .collect::<Result<Vec<_>>>()?,
        pending_snapshot_partitions: value.pending_snapshot_partitions,
        remote_snapshot_version: value.remote_snapshot_version.clone(),
        remote_message_window_size: value.remote_message_window_size,
        clock_offset_ms: value.clock_offset_ms,
        last_clock_sync: value.last_clock_sync.clone(),
        snapshot_digests_sent_total: value.snapshot_digests_sent_total,
        snapshot_digests_received_total: value.snapshot_digests_received_total,
        snapshot_chunks_sent_total: value.snapshot_chunks_sent_total,
        snapshot_chunks_received_total: value.snapshot_chunks_received_total,
        last_snapshot_digest_at: value.last_snapshot_digest_at.clone(),
        last_snapshot_chunk_at: value.last_snapshot_chunk_at.clone(),
    })
}

pub(crate) fn login_info_from_proto(value: &proto::LoginResponse) -> Result<LoginInfo> {
    Ok(LoginInfo {
        user: user_from_proto(value.user.as_ref())?,
        protocol_version: value.protocol_version.clone(),
        session_ref: session_ref_from_proto(value.session_ref.as_ref())?,
    })
}

pub(crate) fn user_ref_from_http(value: Option<&Value>) -> UserRef {
    let Some(Value::Object(value)) = value else {
        return UserRef::default();
    };
    UserRef {
        node_id: int_value(value.get("node_id")),
        user_id: int_value(value.get("user_id").or_else(|| value.get("id"))),
    }
}

pub(crate) fn user_from_http(value: &Map<String, Value>) -> Result<User> {
    let profile = value.get("profile").or_else(|| value.get("profile_json"));
    Ok(User {
        node_id: int_value(value.get("node_id")),
        user_id: int_value(value.get("user_id").or_else(|| value.get("id"))),
        username: str_value(value.get("username")),
        role: str_value(value.get("role")),
        profile_json: json_value_to_bytes(profile)?,
        system_reserved: bool_value(value.get("system_reserved")),
        created_at: str_value(value.get("created_at")),
        updated_at: str_value(value.get("updated_at")),
        origin_node_id: int_value(value.get("origin_node_id")),
        login_name: str_value(value.get("login_name")),
    })
}

pub(crate) fn user_metadata_from_http(value: &Map<String, Value>) -> Result<UserMetadata> {
    Ok(UserMetadata {
        owner: user_ref_from_http(value.get("owner")),
        key: str_value(value.get("key")),
        value: base64_to_bytes_field(value.get("value"), "value")?,
        updated_at: str_value(value.get("updated_at")),
        deleted_at: str_value(value.get("deleted_at")),
        expires_at: str_value(value.get("expires_at")),
        origin_node_id: int_value(value.get("origin_node_id")),
    })
}

pub(crate) fn user_metadata_scan_result_from_http(
    value: &Map<String, Value>,
) -> Result<UserMetadataScanResult> {
    let Some(Value::Array(items)) = value.get("items") else {
        return Err(Error::protocol(
            "missing items in scan_user_metadata response",
        ));
    };
    Ok(UserMetadataScanResult {
        items: items
            .iter()
            .map(expect_object)
            .map(|item| item.and_then(user_metadata_from_http))
            .collect::<Result<Vec<_>>>()?,
        count: value.get("count").map_or_else(
            || i32::try_from(items.len()).unwrap_or(i32::MAX),
            |count| i32_value(Some(count)),
        ),
        next_after: str_value(value.get("next_after")),
    })
}

pub(crate) fn message_from_http(value: &Map<String, Value>) -> Result<Message> {
    let created_at_hlc = {
        let created_at_hlc = str_value(value.get("created_at_hlc"));
        if created_at_hlc.is_empty() {
            str_value(value.get("created_at"))
        } else {
            created_at_hlc
        }
    };
    Ok(Message {
        recipient: user_ref_from_http(value.get("recipient")),
        node_id: int_value(value.get("node_id")),
        seq: int_value(value.get("seq")),
        sender: user_ref_from_http(value.get("sender")),
        body: base64_to_bytes(value.get("body"))?,
        created_at_hlc,
    })
}

pub(crate) fn relay_accepted_from_http(value: &Map<String, Value>) -> Result<RelayAccepted> {
    Ok(RelayAccepted {
        packet_id: u64_value(value.get("packet_id")),
        source_node_id: int_value(value.get("source_node_id")),
        target_node_id: int_value(value.get("target_node_id")),
        recipient: user_ref_from_http(value.get("recipient")),
        delivery_mode: delivery_mode_from_http(value.get("delivery_mode")),
        target_session: None,
    })
}

pub(crate) fn attachment_from_http(value: &Map<String, Value>) -> Result<Attachment> {
    let config_json = match value.get("config_json") {
        Some(value) => json_value_to_bytes(Some(value))?,
        None => json_value_to_bytes(Some(&Value::Object(Map::new())))?,
    };
    Ok(Attachment {
        owner: user_ref_from_http(value.get("owner")),
        subject: user_ref_from_http(value.get("subject")),
        attachment_type: attachment_type_from_http(value.get("attachment_type"))?,
        config_json,
        attached_at: str_value(value.get("attached_at")),
        deleted_at: str_value(value.get("deleted_at")),
        origin_node_id: int_value(value.get("origin_node_id")),
    })
}

pub(crate) fn subscription_from_http(value: &Map<String, Value>) -> Result<Subscription> {
    Ok(subscription_from_attachment(&attachment_from_http(value)?))
}

pub(crate) fn blacklist_entry_from_http(value: &Map<String, Value>) -> Result<BlacklistEntry> {
    Ok(blacklist_entry_from_attachment(&attachment_from_http(
        value,
    )?))
}

pub(crate) fn event_from_http(value: &Map<String, Value>) -> Result<Event> {
    let event = value.get("event").or_else(|| value.get("event_json"));
    Ok(Event {
        sequence: int_value(value.get("sequence")),
        event_id: int_value(value.get("event_id")),
        event_type: str_value(value.get("event_type")),
        aggregate: str_value(value.get("aggregate")),
        aggregate_node_id: int_value(value.get("aggregate_node_id")),
        aggregate_id: int_value(value.get("aggregate_id")),
        hlc: str_value(value.get("hlc")),
        origin_node_id: int_value(value.get("origin_node_id")),
        event_json: json_value_to_bytes(event)?,
    })
}

pub(crate) fn cluster_node_from_http(value: &Map<String, Value>) -> Result<ClusterNode> {
    Ok(ClusterNode {
        node_id: int_value(value.get("node_id")),
        is_local: bool_value(value.get("is_local")),
        configured_url: str_value(value.get("configured_url")),
        source: str_value(value.get("source")),
    })
}

pub(crate) fn logged_in_user_from_http(value: &Map<String, Value>) -> Result<LoggedInUser> {
    Ok(LoggedInUser {
        node_id: int_value(value.get("node_id")),
        user_id: int_value(value.get("user_id")),
        username: str_value(value.get("username")),
        login_name: str_value(value.get("login_name")),
    })
}

pub(crate) fn operations_status_from_http(value: &Map<String, Value>) -> Result<OperationsStatus> {
    Ok(OperationsStatus {
        node_id: int_value(value.get("node_id")),
        message_window_size: i32_value(value.get("message_window_size")),
        last_event_sequence: int_value(value.get("last_event_sequence")),
        write_gate_ready: bool_value(value.get("write_gate_ready")),
        conflict_total: int_value(value.get("conflict_total")),
        message_trim: message_trim_status_from_http(value.get("message_trim")),
        projection: projection_status_from_http(value.get("projection")),
        peers: list_values(value.get("peers"))
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(peer_status_from_http))
            .collect::<Result<Vec<_>>>()?,
        event_log_trim: match value.get("event_log_trim") {
            Some(Value::Object(value)) => Some(event_log_trim_status_from_http(value)?),
            _ => None,
        },
    })
}

fn attachment_type_from_http(value: Option<&Value>) -> Result<AttachmentType> {
    match str_value(value).as_str() {
        "channel_manager" => Ok(AttachmentType::ChannelManager),
        "channel_writer" => Ok(AttachmentType::ChannelWriter),
        "channel_subscription" => Ok(AttachmentType::ChannelSubscription),
        "user_blacklist" => Ok(AttachmentType::UserBlacklist),
        other => Err(Error::protocol(format!(
            "unsupported attachment type {other:?}"
        ))),
    }
}

pub(crate) fn message_trim_status_from_http(value: Option<&Value>) -> MessageTrimStatus {
    let Some(Value::Object(value)) = value else {
        return MessageTrimStatus::default();
    };
    MessageTrimStatus {
        trimmed_total: int_value(value.get("trimmed_total")),
        last_trimmed_at: str_value(value.get("last_trimmed_at")),
    }
}

pub(crate) fn event_log_trim_status_from_http(
    value: &Map<String, Value>,
) -> Result<EventLogTrimStatus> {
    Ok(EventLogTrimStatus {
        trimmed_total: int_value(value.get("trimmed_total")),
        last_trimmed_at: str_value(value.get("last_trimmed_at")),
    })
}

pub(crate) fn projection_status_from_http(value: Option<&Value>) -> ProjectionStatus {
    let Some(Value::Object(value)) = value else {
        return ProjectionStatus::default();
    };
    ProjectionStatus {
        pending_total: int_value(value.get("pending_total")),
        last_failed_at: str_value(value.get("last_failed_at")),
    }
}

pub(crate) fn peer_origin_status_from_http(value: &Map<String, Value>) -> Result<PeerOriginStatus> {
    Ok(PeerOriginStatus {
        origin_node_id: int_value(value.get("origin_node_id")),
        acked_event_id: int_value(value.get("acked_event_id")),
        applied_event_id: int_value(value.get("applied_event_id")),
        unconfirmed_events: int_value(value.get("unconfirmed_events")),
        cursor_updated_at: str_value(value.get("cursor_updated_at")),
        remote_last_event_id: u64_value(value.get("remote_last_event_id")),
        pending_catchup: bool_value(value.get("pending_catchup")),
    })
}

pub(crate) fn peer_status_from_http(value: &Map<String, Value>) -> Result<PeerStatus> {
    Ok(PeerStatus {
        node_id: int_value(value.get("node_id")),
        configured_url: str_value(value.get("configured_url")),
        source: str_value(value.get("source")),
        discovered_url: str_value(value.get("discovered_url")),
        discovery_state: str_value(value.get("discovery_state")),
        last_discovered_at: str_value(value.get("last_discovered_at")),
        last_connected_at: str_value(value.get("last_connected_at")),
        last_discovery_error: str_value(value.get("last_discovery_error")),
        connected: bool_value(value.get("connected")),
        session_direction: str_value(value.get("session_direction")),
        origins: list_values(value.get("origins"))
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(peer_origin_status_from_http))
            .collect::<Result<Vec<_>>>()?,
        pending_snapshot_partitions: i32_value(value.get("pending_snapshot_partitions")),
        remote_snapshot_version: str_value(value.get("remote_snapshot_version")),
        remote_message_window_size: i32_value(value.get("remote_message_window_size")),
        clock_offset_ms: int_value(value.get("clock_offset_ms")),
        last_clock_sync: str_value(value.get("last_clock_sync")),
        snapshot_digests_sent_total: u64_value(value.get("snapshot_digests_sent_total")),
        snapshot_digests_received_total: u64_value(value.get("snapshot_digests_received_total")),
        snapshot_chunks_sent_total: u64_value(value.get("snapshot_chunks_sent_total")),
        snapshot_chunks_received_total: u64_value(value.get("snapshot_chunks_received_total")),
        last_snapshot_digest_at: str_value(value.get("last_snapshot_digest_at")),
        last_snapshot_chunk_at: str_value(value.get("last_snapshot_chunk_at")),
    })
}

pub(crate) fn delete_user_result_from_http(value: &Map<String, Value>) -> Result<DeleteUserResult> {
    let user = match value.get("user") {
        Some(Value::Object(inner)) => UserRef {
            node_id: int_value(inner.get("node_id")),
            user_id: int_value(inner.get("user_id").or_else(|| inner.get("id"))),
        },
        _ => UserRef {
            node_id: int_value(value.get("node_id")),
            user_id: int_value(value.get("user_id").or_else(|| value.get("id"))),
        },
    };
    Ok(DeleteUserResult {
        status: str_value(value.get("status")),
        user,
    })
}

pub(crate) fn json_bytes_to_value(data: &[u8], field: &str) -> Result<Value> {
    serde_json::from_slice(data)
        .map_err(|err| Error::validation(format!("{field} must be valid JSON: {err}")))
}

pub(crate) fn expect_object(value: &Value) -> Result<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| Error::protocol("unexpected JSON object"))
}

pub(crate) fn items_from_payload<'a>(value: &'a Value, keys: &[&str]) -> Result<Vec<&'a Value>> {
    if let Value::Array(items) = value {
        return Ok(items.iter().collect());
    }
    let object = expect_object(value)?;
    for key in keys {
        if let Some(Value::Array(items)) = object.get(*key) {
            return Ok(items.iter().collect());
        }
    }
    Err(Error::protocol("missing items in list response"))
}

fn json_value_to_bytes(value: Option<&Value>) -> Result<Vec<u8>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    serde_json::to_vec(value).map_err(|err| Error::protocol(format!("invalid JSON value: {err}")))
}

fn base64_to_bytes(value: Option<&Value>) -> Result<Vec<u8>> {
    base64_to_bytes_field(value, "body")
}

fn base64_to_bytes_field(value: Option<&Value>, field: &str) -> Result<Vec<u8>> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(value)) => STANDARD
            .decode(value)
            .map_err(|err| Error::protocol(format!("invalid base64 {field}: {err}"))),
        _ => Err(Error::protocol(format!("invalid base64 {field}"))),
    }
}

fn list_values(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

fn delivery_mode_from_http(value: Option<&Value>) -> DeliveryMode {
    match value.and_then(Value::as_str) {
        Some("best_effort") => DeliveryMode::BestEffort,
        Some("route_retry") => DeliveryMode::RouteRetry,
        _ => DeliveryMode::Unspecified,
    }
}

fn str_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn bool_value(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn int_value(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(value)) => value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .unwrap_or(0),
        Some(Value::String(value)) => value.parse().unwrap_or(0),
        _ => 0,
    }
}

fn i32_value(value: Option<&Value>) -> i32 {
    i32::try_from(int_value(value)).unwrap_or_default()
}

fn u64_value(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(value)) => value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
            .unwrap_or(0),
        Some(Value::String(value)) => value.parse().unwrap_or(0),
        _ => 0,
    }
}
