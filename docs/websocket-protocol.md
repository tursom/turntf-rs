# turntf-rs WebSocket + Protobuf 协议详情

本文档详细描述 turntf-rs SDK 与 turntf 服务端之间的 WebSocket 协议，包括帧格式、消息序列、类型映射和关键约束。

## 1. 传输层

- **协议**：WebSocket（RFC 6455）
- **帧类型**：Binary Frame
- **编码**：Protobuf（Protocol Buffers 3）
- **端口**：与服务端 HTTP 端口相同（通过 HTTP 升级到 WebSocket）
- **路径**：
  - 默认：`/ws/client`
  - `realtime_stream=true`：`/ws/realtime`

## 2. 帧结构

所有 WebSocket 消息都是 protobuf 编码的二进制帧。两种帧类型：

### 客户端发往服务端

```protobuf
message ClientEnvelope {
  oneof body {
    LoginRequest login = 1;                               // 首帧必需
    SendMessageRequest send_message = 2;                  // 发送消息/瞬时包
    AckMessage ack_message = 3;                           // 确认已消费
    Ping ping = 4;                                        // 心跳
    CreateUserRequest create_user = 5;                    // RPC
    GetUserRequest get_user = 6;                          // RPC
    UpdateUserRequest update_user = 7;                    // RPC
    DeleteUserRequest delete_user = 8;                    // RPC
    ListMessagesRequest list_messages = 9;                // RPC
    UpsertUserAttachmentRequest upsert_user_attachment = 10;
    DeleteUserAttachmentRequest delete_user_attachment = 11;
    ListUserAttachmentsRequest list_user_attachments = 12;
    ListEventsRequest list_events = 13;                   // RPC
    OperationsStatusRequest operations_status = 14;       // RPC
    MetricsRequest metrics = 15;                          // RPC
    ListClusterNodesRequest list_cluster_nodes = 16;      // RPC
    ListNodeLoggedInUsersRequest list_node_logged_in_users = 17;
    ResolveUserSessionsRequest resolve_user_sessions = 18;
    GetUserMetadataRequest get_user_metadata = 19;        // RPC
    UpsertUserMetadataRequest upsert_user_metadata = 20;  // RPC
    DeleteUserMetadataRequest delete_user_metadata = 21;  // RPC
    ScanUserMetadataRequest scan_user_metadata = 22;      // RPC
    ListUsersRequest list_users = 23;                     // RPC
  }
}
```

### 服务端发往客户端

```protobuf
message ServerEnvelope {
  oneof body {
    LoginResponse login_response = 1;                     // 首帧回复
    MessagePushed message_pushed = 2;                     // 消息推送
    SendMessageResponse send_message_response = 3;        // RPC 回复
    Error error = 4;                                      // 错误
    Pong pong = 5;                                        // 心跳回复
    PacketPushed packet_pushed = 6;                       // 瞬时包推送
    CreateUserResponse create_user_response = 7;          // RPC 回复
    GetUserResponse get_user_response = 8;
    UpdateUserResponse update_user_response = 9;
    DeleteUserResponse delete_user_response = 10;
    ListMessagesResponse list_messages_response = 11;
    UpsertUserAttachmentResponse upsert_user_attachment_response = 12;
    DeleteUserAttachmentResponse delete_user_attachment_response = 13;
    ListUserAttachmentsResponse list_user_attachments_response = 14;
    ListEventsResponse list_events_response = 15;
    OperationsStatusResponse operations_status_response = 16;
    MetricsResponse metrics_response = 17;
    ListClusterNodesResponse list_cluster_nodes_response = 18;
    ListNodeLoggedInUsersResponse list_node_logged_in_users_response = 19;
    ResolveUserSessionsResponse resolve_user_sessions_response = 20;
    GetUserMetadataResponse get_user_metadata_response = 21;
    UpsertUserMetadataResponse upsert_user_metadata_response = 22;
    DeleteUserMetadataResponse delete_user_metadata_response = 23;
    ScanUserMetadataResponse scan_user_metadata_response = 24;
    ListUsersResponse list_users_response = 25;
  }
}
```

## 3. 连接序列

### 正常连接序列

```
Client                              Server
  |                                   |
  |--- WebSocket 升级 --->            |
  |<-- 101 Switching Protocols ---    |
  |                                   |
  |--- ClientEnvelope(Login) ---->    |
  |<-- ServerEnvelope(LoginResponse) -|
  |                                   |
  |--- ClientEnvelope(Ping) ----->    |  ← 周期性心跳
  |<-- ServerEnvelope(Pong) --------- |
  |                                   |
  |--- ClientEnvelope(SendMessage) -> |  ← RPC 请求
  |<-- ServerEnvelope(SendMessageRes) |
  |                                   |
  |<-- ServerEnvelope(MessagePushed)  |  ← 服务端推送
  |--- ClientEnvelope(AckMessage) --> |
  |                                   |
  |<-- ServerEnvelope(PacketPushed)   |  ← 瞬时包推送
  |                                   |
  |--- WebSocket Close ---------->    |
  |<-- WebSocket Close ------------   |
```

### 登录失败序列

```
Client                              Server
  |                                   |
  |--- ClientEnvelope(Login) ---->    |
  |<-- ServerEnvelope(Error) --------  |
  |     code="unauthorized"           |
```

登录失败时 SDK 会设置 `stop_reconnect=true`，不会自动重连。

### 连接中断序列

```
Client                              Server
  |                                   |
  |--- ... (已建立连接) ...           |
  |                                   |
  |  (网络中断或服务端关闭连接)        |
  |                                   |
  |--- [WebSocket 关闭帧/IO 错误]      |
  |                                   |
  SDK 内部：
  1. 设置 connected=false
  2. fail_all_pending (DisconnectedError)
  3. emit ClientEvent::Disconnect
  4. 如果应当重试 → 延迟后重新建立连接
```

## 4. 消息类型详解

### LoginRequest（客户端首帧）

```protobuf
message LoginRequest {
  UserRef user = 1;                       // 登录用户
  string password = 3;                    // bcrypt 编码后的密码
  repeated MessageCursor seen_messages = 4; // 已确认游标列表
  bool transient_only = 5;               // 仅瞬时消息模式
}
```

约束：
- 这是客户端发送的**第一个** `ClientEnvelope`
- `password` 字段在 Rust SDK 中由 `PasswordInput.wire_value()` 提供，始终是 bcrypt 编码后的值
- `seen_messages` 为空列表时服务端不做去重过滤
- `transient_only=true` 时服务端不会补发持久化消息

### LoginResponse（服务端首帧回复）

```protobuf
message LoginResponse {
  User user = 1;
  string protocol_version = 2;
  SessionRef session_ref = 3;
}
```

`session_ref` 是本次登录会话的标识，包含 `serving_node_id` 和 `session_id`。重连/重登录后可能变化。

### SendMessageRequest（发送消息/瞬时包）

```protobuf
message SendMessageRequest {
  uint64 request_id = 1;
  UserRef target = 2;                     // 目标用户
  bytes body = 3;                         // 消息内容
  ClientDeliveryKind delivery_kind = 4;   // PERSISTENT / TRANSIENT
  ClientDeliveryMode delivery_mode = 5;   // BEST_EFFORT / ROUTE_RETRY
  ClientMessageSyncMode sync_mode = 6;    // 同步模式（当前未使用）
  SessionRef target_session = 7;          // 定向会话（仅 transient）
}
```

`delivery_kind` 决定消息类型：
- `PERSISTENT`（默认）：消息会落库，服务端回复 `SendMessageResponse.body.message`
- `TRANSIENT`：瞬时包不会落库，服务端回复 `SendMessageResponse.body.transient_accepted`

### SendMessageResponse

```protobuf
message SendMessageResponse {
  uint64 request_id = 1;
  oneof body {
    Message message = 2;                   // 持久化消息
    TransientAccepted transient_accepted = 3; // 瞬时包受理确认
  }
}
```

### ListUsersRequest / ListUsersResponse

```protobuf
message ListUsersRequest {
  uint64 request_id = 1;
  string name = 2;
  UserRef uid = 3;
}

message ListUsersResponse {
  uint64 request_id = 1;
  repeated User items = 2;
  int32 count = 3;
}
```

约束：
- `list_users` 返回当前登录用户可通讯的活跃用户集合。
- `name` 为空或仅包含空白时，表示不按名称过滤。
- `uid` 省略或传 `{ node_id: 0, user_id: 0 }` 时，表示不按 uid 过滤；只填一半会触发 `invalid_request`。
- 普通用户查询其他用户时，返回的 `User.login_name` 可能为空；管理员和查询自己时保持可见。
- `realtime_stream=true` 连接不支持该 RPC。

### MessagePushed（服务端推送）

```protobuf
message MessagePushed {
  Message message = 1;
}
```

当其他用户向你发送消息或频道广播时服务端推送。Rust SDK 的处理顺序：
1. `save_message`
2. `save_cursor`
3. 发送 `AckMessage`（如果 `ack_messages=true`）
4. 发出 `ClientEvent::Message`

### PacketPushed（瞬时包推送）

```protobuf
message PacketPushed {
  Packet packet = 1;
}
```

服务端推送的瞬时包，与 `MessagePushed` 不同：
- 没有 `(node_id, seq)` 游标
- 不写入 `CursorStore`
- 不发送 `AckMessage`
- 重连后不会补发

### AckMessage

```protobuf
message AckMessage {
  MessageCursor cursor = 1;
}
```

客户端通知服务端已确认消费某条消息。Rust SDK 在 `save_message` + `save_cursor` 成功后才会发送。

### Error

```protobuf
message Error {
  string code = 1;
  string message = 2;
  uint64 request_id = 3;
}
```

错误有两种情况：
- `request_id != 0`：对应某个 RPC 请求的错误，会 resolve 对应的 pending RPC
- `request_id == 0`：连接级别的错误，会导致当前连接终止

区分：`code == "unauthorized"` 时 SDK 会设置 `stop_reconnect=true`。

### Ping / Pong

```protobuf
message Ping {
  uint64 request_id = 1;
}

message Pong {
  uint64 request_id = 1;
}
```

应用层心跳，用于检测连接活性。Rust SDK 默认 30 秒发送一次 Ping。

## 5. 枚举定义

### ClientDeliveryKind

| proto 值 | Rust SDK 对应 |
| --- | --- |
| `CLIENT_DELIVERY_KIND_UNSPECIFIED` | 不使用（SDK 不发送此值） |
| `CLIENT_DELIVERY_KIND_PERSISTENT` | `send_message()` 默认值（0） |
| `CLIENT_DELIVERY_KIND_TRANSIENT` | `send_packet()` / `send_packet_to_session()` 使用 |

### ClientDeliveryMode

| proto 值 | Rust `DeliveryMode` | 说明 |
| --- | --- | --- |
| `CLIENT_DELIVERY_MODE_UNSPECIFIED` | `Unspecified` | SDK 本地拒绝发送 |
| `CLIENT_DELIVERY_MODE_BEST_EFFORT` | `BestEffort` | 尽最大努力送达 |
| `CLIENT_DELIVERY_MODE_ROUTE_RETRY` | `RouteRetry` | 路由层会重试 |

### AttachmentType

| proto 值 | Rust `AttachmentType` |
| --- | --- |
| `ATTACHMENT_TYPE_UNSPECIFIED` | 不使用（查询过滤用） |
| `ATTACHMENT_TYPE_CHANNEL_MANAGER` | `ChannelManager` |
| `ATTACHMENT_TYPE_CHANNEL_WRITER` | `ChannelWriter` |
| `ATTACHMENT_TYPE_CHANNEL_SUBSCRIPTION` | `ChannelSubscription` |
| `ATTACHMENT_TYPE_USER_BLACKLIST` | `UserBlacklist` |

## 6. 核心数据类型映射

### UserRef

| proto | Rust |
| --- | --- |
| `int64 node_id` | `i64` |
| `int64 user_id` | `i64` |

### Message

| proto | Rust | 备注 |
| --- | --- | --- |
| `UserRef recipient` | `UserRef` | 接收者 |
| `int64 node_id` | `i64` | 消息所在节点 |
| `int64 seq` | `i64` | 消息序号 |
| `UserRef sender` | `UserRef` | 发送者 |
| `bytes body` | `Vec<u8>` | 消息体 |
| `string created_at_hlc` | `String` | HLC 时间戳 |

### SessionRef

| proto | Rust |
| --- | --- |
| `int64 serving_node_id` | `i64` |
| `string session_id` | `String` |

### MessageCursor

| proto | Rust |
| --- | --- |
| `int64 node_id` | `i64` |
| `int64 seq` | `i64` |

## 7. URL 路径推导规则

```
HTTP 基础地址        → WebSocket 连接地址
====================================================
http://host          → ws://host/ws/client
https://host         → wss://host/ws/client
http://host/path     → ws://host/path/ws/client
ws://host            → ws://host/ws/client
http://host          → ws://host/ws/realtime  (realtime_stream=true)
```

Rust SDK 的路径推导实现在 `client.rs` 的 `websocket_url()` 函数中：

```rust
fn websocket_url(base: &str, realtime: bool) -> Result<String> {
    // 1. 解析 URL
    // 2. http→ws, https→wss 协议替换
    // 3. 路径拼上 /ws/client 或 /ws/realtime
    // 4. 清理 query 和 fragment
}
```

## 8. 请求-响应匹配规则

Rust SDK 通过 `request_id` 关联请求和响应：

| 客户端请求 | 对应服务端响应 | 匹配字段 |
| --- | --- | --- |
| SendMessageRequest | SendMessageResponse | `request_id` |
| Ping | Pong | `request_id` |
| CreateUserRequest | CreateUserResponse | `request_id` |
| GetUserRequest | GetUserResponse | `request_id` |
| UpdateUserRequest | UpdateUserResponse | `request_id` |
| DeleteUserRequest | DeleteUserResponse | `request_id` |
| ListMessagesRequest | ListMessagesResponse | `request_id` |
| ...（所有 RPC 请求） | ...（对应 RPC 响应） | `request_id` |

未匹配的 `request_id`（例如响应到达时 pending 表已移除该 ID）会被静默丢弃。

## 9. 与 HTTP JSON 的对比

### WebSocket 路径的优势

- 支持服务端主动推送（MessagePushed / PacketPushed）
- 支持自动重连和消息去重
- 支持 session 级定向投递
- 单条连接可复用所有 RPC
- 不需要 Bearer Token（登录帧使用密码直接鉴权）

### HTTP 路径的优势

- 无状态，适合一次性请求
- 简单的鉴权模型（Bearer Token）
- 不需要维护长连接
- 适合后台任务和管理脚本

## 10. 协议演进注意事项

### 新增字段

- 给现有 message 新增字段是向后兼容的，旧客户端会忽略未知字段
- Rust SDK 使用 `prost` 生成的类型，未知字段不会导致反序列化失败
- 新增字段后只需更新 `mapping.rs` 中的转换逻辑

### 新增 message 类型

- 在 `ClientEnvelope` 或 `ServerEnvelope` 的 `oneof` 中新增字段
- 更新 `proto/client.proto`
- 更新 `src/client.rs` 中的 `handle_server_envelope` 方法
- 在 `src/mapping.rs` 中添加该消息到 Rust 类型的映射

### 修改含共享语义的字段

- 修改 `seen_messages` 相关逻辑时注意跨语言 SDK 实现一致
- 修改 `ack_message` 语义时确保所有 SDK 的 `save_message -> save_cursor -> ack` 顺序不被打乱
- `session_ref` 的生成逻辑由服务端控制，SDK 仅透传
