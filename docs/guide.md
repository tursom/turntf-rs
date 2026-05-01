# turntf-rs 使用与语义说明

本文档面向 Rust SDK 使用者，重点解释 `turntf-rs` 在源码和测试里已经落地的行为约束，而不是只罗列 API 名称。跨 SDK 共享语义仍以服务端文档和协议定义为准，建议同时对照：

- [`../turntf/docs/client-websocket.md`](../../turntf/docs/client-websocket.md)
- [`../turntf/proto/client.proto`](../../turntf/proto/client.proto)

## 1. 模块定位

`turntf-rs` 是一个“高层封装 + 共享协议映射”风格的 SDK：

- 对外暴露 `UserRef`、`Message`、`SessionRef`、`RelayAccepted` 等 Rust 结构体。
- 内部通过 `build.rs` 把 `proto/client.proto` 生成为 protobuf 代码，但不把生成类型直接暴露给业务侧。
- 长连接能力以 `Client` 为中心，短连接能力以 `HttpClient` 为中心。
- 可靠消息消费边界由 `CursorStore` 决定，而不是由 SDK 内部私有内存状态决定。

对应源码入口：

| 文件 | 作用 |
| --- | --- |
| `src/lib.rs` | 对外导出公共 API |
| `src/http.rs` | `HttpClient` 的 REST JSON 封装 |
| `src/client.rs` | `Client` 的 WebSocket、登录、重连、RPC、多播事件流 |
| `src/store.rs` | `CursorStore` 抽象与 `MemoryCursorStore` 默认实现 |
| `src/errors.rs` | 统一错误模型 |
| `src/types.rs` | 公开数据类型与配置默认值 |
| `proto/client.proto` | Rust SDK 的本地协议定义 |

## 2. `HttpClient` 与 `Client` 的分工

### `HttpClient`

适合下面这些场景：

- 已经通过别的方式拿到了 Bearer Token
- 只需要管理接口、查询接口或后台任务
- 不需要收实时推送
- 不需要自动重连、游标存储和 ACK 顺序控制

它的特点是：

- 每个请求都是独立 HTTP 调用
- 登录通过 `POST /auth/login`，支持 `node_id + user_id + password` 或 `login_name + password`
- 绝大多数方法都显式要求 `token: &str`
- 负责 JSON 编解码、状态码映射、参数校验

典型能力包括：

- 用户管理：`create_user`、`get_user`、`update_user`、`delete_user`
- 订阅与黑名单：`subscribe_channel`、`unsubscribe_channel`、`list_subscriptions`、`block_user`
- 查询：`list_messages`、`list_events`、`list_cluster_nodes`、`list_node_logged_in_users`
- 运维：`operations_status`、`metrics`
- 瞬时包：`post_packet`

### `Client`

适合下面这些场景：

- 需要 `MessagePushed` / `PacketPushed` 实时推送
- 需要自动重连、重登录和历史消息去重
- 需要 `resolve_user_sessions` / `send_packet_to_session`
- 希望在一条长连接里复用管理 RPC 与实时收包

它的特点是：

- 使用 WebSocket binary frame 发送 protobuf
- 首帧登录使用用户密码，不走 HTTP token；支持用户 ID 选择器和 `login_name` 选择器
- 通过 `tokio::sync::broadcast` 向多个订阅者分发 `ClientEvent`
- 内部维护请求 ID、pending RPC、ping 循环和重连状态机

### 二者之间的桥接关系

- `Client::login(...)`、`Client::login_with_password(...)`、`Client::login_by_login_name(...)` 和 `Client::login_by_login_name_with_password(...)` 本质上都是转调内部 `HttpClient`
- `Client::http()` 可以拿到底层 `HttpClient` 的克隆，用于把 REST 能力和长连接能力放在同一个业务对象里使用
- `Client` 的大部分查询/管理方法和 `HttpClient` 同名，但走的是长连接 RPC，不需要 token

## 3. 连接模型与配置项

`Client::new(Config)` 会先做本地校验：

- `base_url` 不能为空
- 当 `Config.login_name` 为空时，`credentials.node_id` / `credentials.user_id` 必须大于 0
- 当 `Config.login_name` 非空时，会改走 `login_name + password` 登录
- `credentials.password` 不能为空

### 连接地址映射

SDK 会基于 `base_url` 自动推导 WebSocket 地址：

- `http://host` -> `ws://host/ws/client`
- `https://host` -> `wss://host/ws/client`
- `http://host/base` -> `ws://host/base/ws/client`
- `realtime_stream=true` 时会改成 `/ws/realtime`

这意味着：

- `base_url` 可以带路径前缀
- `realtime_stream` 不是简单的“实时模式开关”，它会切换到服务端的受限实时通道

### 默认配置

默认值定义在 `types.rs` 的 `ClientConfigDefaults`：

| 配置项 | 默认值 | 含义 |
| --- | --- | --- |
| `reconnect` | `true` | 断线后自动重连 |
| `initial_reconnect_delay` | `1s` | 首次重连等待时间 |
| `max_reconnect_delay` | `30s` | 指数退避上限 |
| `ping_interval` | `30s` | 应用层 ping 周期 |
| `request_timeout` | `10s` | 单个 RPC 等待超时 |
| `ack_messages` | `true` | 收到持久化消息后自动 ACK |
| `transient_only` | `false` | 登录时声明只关心瞬时消息 |
| `realtime_stream` | `false` | 使用 `/ws/realtime` |
| `event_channel_capacity` | `256` | 本地广播事件缓冲区大小 |

### `realtime_stream` 与 `transient_only`

这两个配置不要混为一谈：

- `transient_only=true`：登录帧会把 `transient_only` 传给服务端，表示该连接不需要持久化消息补发。
- `realtime_stream=true`：连接路径变成 `/ws/realtime`，服务端会额外限制可用 RPC。

根据服务端实现，`/ws/realtime` 有两个重要限制：

- 只允许发送瞬时 `send_message`
- 会拒绝大部分管理 RPC，例如 `create_user`、`get_user`、`update_user`、`list_messages`、`metrics`、`operations_status`

仍然适合放在 `/ws/realtime` 上的能力主要是：

- `PacketPushed`
- 瞬时 `send_packet` / `send_packet_to_session`
- `list_cluster_nodes`
- `list_node_logged_in_users`
- `resolve_user_sessions`

如果你的连接既要收实时包，又要跑完整管理 RPC，优先使用默认的 `/ws/client`。

## 4. 事件流与订阅接口

`Client.subscribe().await` 会返回 `ClientSubscription`，底层是 `tokio_stream::wrappers::BroadcastStream<ClientEvent>`。`ClientEvent` 共有五类：

- `Login(LoginInfo)`
- `Message(Message)`
- `Packet(Packet)`
- `Error(Error)`
- `Disconnect(Error)`

### 使用建议

- 想拿到首次登录事件时，最好在 `connect()` 之前先 `subscribe()`
- 事件流没有历史回放；订阅建立之前发生的事件不会补发
- 多个订阅者共享同一个 `broadcast` 通道，任何一个慢消费者都可能收到 `Lagged`

测试 `client_subscription_reports_lagged_and_closes` 已验证：

- 当 `event_channel_capacity` 太小且消费跟不上时，订阅者会收到 `BroadcastStreamRecvError::Lagged(_)`
- 出现 `Lagged` 后流不会立即失效，后续仍能继续消费新事件
- `close()` 后 sender 被移除，事件流最终会结束

### `Disconnect` 与 `Error` 的区别

- `Disconnect`：表示一条已经建立的连接结束了
- `Error`：表示 SDK 内部遇到错误；如果允许重试，它通常会作为“这次连接失败/中断的原因”被发出来

典型序列通常是：

1. `Login`
2. 若连接中断，先收到 `Disconnect`
3. SDK 判断允许重连时，再收到一个 `Error`
4. 下一次连接成功后，再收到新的 `Login`

## 5. 自动重连与重登录

`Client.connect()` 并不是“只拨号一次”，而是确保后台运行任务已经启动，并等待连接进入 `connected` 状态。

内部行为可以概括为：

1. 启动后台 `run()` 任务
2. 调用 `connect_and_serve()`
3. 登录成功后进入读循环 + ping 循环
4. 如果连接断开，根据错误类型和配置决定是否重试
5. 需要重试时按指数退避等待，然后重新拨号、重新登录

### 重登录时会带什么

每次重连登录时，SDK 都会重新发送：

- `credentials.node_id` / `credentials.user_id`，或者 `Config.login_name`
- `credentials.password`
- `cursor_store.load_seen_messages()` 返回的全部游标
- `transient_only`

这正是 Rust SDK 可靠去重的核心：

- 连接内 ACK 只影响服务端当前内存态
- 跨重连真正可靠的是下次 `LoginRequest.seen_messages`

测试 `client_reconnects_with_seen_messages_and_realtime_path` 已验证：

- 第一次连接收到并持久化消息后
- 第二次登录时 `seen_messages` 会包含该消息的 `(node_id, seq)`
- `realtime_stream=true` 时路径会稳定落到 `/ws/realtime`

### 什么时候不会自动重连

以下情况会停止重连：

- `Config.reconnect == false`
- 显式调用 `close()`
- 登录阶段返回 `unauthorized`
- 状态机已进入终态错误

其中 `unauthorized` 很关键：SDK 会把它视为凭据问题，而不是临时网络抖动。

## 6. `session_ref` 与定向瞬时包

`session_ref` 是服务端在登录成功后返回的“在线会话标识”，包含：

- `serving_node_id`
- `session_id`

Rust SDK 在两个地方会用到它：

- `LoginInfo.session_ref`：表示当前这条长连接自己的会话标识
- `Packet.target_session` / `RelayAccepted.target_session`：表示一个瞬时包是否被定向到了特定会话

### 生命周期注意点

`session_ref` 不是用户稳定 ID，而是一次在线会话的标识。测试里首次登录拿到的是 `reconnect-first`，重连后变成 `reconnect-second`，这说明：

- 重登录后 `session_ref` 可能变化
- 旧 `session_ref` 不能跨连接长期缓存
- 定向投递前应重新 `resolve_user_sessions`

### `resolve_user_sessions`

`Client::resolve_user_sessions(user)` 会返回：

- `user`
- `presence`：按 serving node 聚合的在线概览
- `sessions`：可用于定向投递的具体会话列表

每个 `ResolvedSession` 包含：

- `session`
- `transport`
- `transient_capable`

推荐使用方式：

1. 先调用 `resolve_user_sessions`
2. 选择一个 `transient_capable=true` 的会话
3. 把对应的 `SessionRef` 传给 `send_packet_to_session`

### `send_packet_to_session`

`send_packet_to_session(target, body, delivery_mode, target_session)` 的语义是：

- 只能发送瞬时包，不会落库
- `target_session` 只约束目标在线会话，不改变 `target` 用户身份
- 服务端响应的 `RelayAccepted` 和后续可能收到的 `PacketPushed` 都会回显 `target_session`

共享约束：

- `target_session` 只允许出现在瞬时消息路径
- `DeliveryMode::Unspecified` 会被 SDK 本地拒绝
- 即使返回了 `RelayAccepted`，也只表示路由层已经受理，不代表目标端已经收到

测试 `client_resolves_sessions_and_targets_transient_delivery` 已验证：

- `resolve_user_sessions` 会返回 presence 和 sessions
- `send_packet_to_session` 会把 `target_session` 放进请求
- 受理响应与后续 `PacketPushed` 都会带回同一个 `target_session`

## 7. `save_message -> save_cursor -> ack` 顺序

这是 Rust SDK 文档里最需要明确写死的一条共享语义。

### 入站 `MessagePushed`

收到 `MessagePushed` 后，`Client` 的处理顺序是：

1. 把 protobuf 映射成 `Message`
2. 调用 `cursor_store.save_message(message.clone())`
3. 调用 `cursor_store.save_cursor(message.cursor())`
4. 如果 `ack_messages=true`，发送 `AckMessage`
5. 发出 `ClientEvent::Message`

也就是说：

- ACK 不会先于本地持久化
- 事件通知也不会先于本地持久化
- 任何一步存储失败都会中断 ACK

测试 `client_login_message_ack_send_packet_create_user_and_ping` 通过 `RecordingStore` 验证了顺序确实是：

```text
message
cursor
```

### 出站持久化 `send_message`

当你调用 `Client::send_message(...)` 时，如果服务端返回的是持久化 `Message`，Rust SDK 也会先做本地持久化，再把结果返回给调用者：

1. 收到 `SendMessageResponse.message`
2. 执行 `save_message`
3. 执行 `save_cursor`
4. pending RPC resolve
5. `send_message(...).await` 返回 `Message`

这意味着发送侧和接收侧共享同一套本地游标语义。

### 存储失败时的行为

如果 `save_message` 或 `save_cursor` 失败：

- SDK 会包装成 `Error::Store`
- 对于入站 `MessagePushed`，会发出 `ClientEvent::Error`
- 对于需要返回 `Message` 的 RPC，调用方会直接拿到错误
- 不会发送 ACK

测试 `client_does_not_ack_on_persist_failure_and_emits_error` 已验证：

- `save_message` 失败后不会偷偷 ACK
- 订阅者能收到 `ClientEvent::Error(Error::Store(_))`

### `PacketPushed` 不参与这条链路

瞬时包和持久化消息是两条不同语义：

- `PacketPushed` 没有 `(node_id, seq)` 游标
- 不写 `CursorStore`
- 不发送 `AckMessage`
- 重连后不会补发

如果业务需要在本地缓存瞬时包，应自行按 `packet_id` 设计幂等逻辑。

## 8. 频道订阅、附件与常用 RPC

Rust SDK 既提供“业务语义方法”，也保留了底层附件抽象。

### 频道订阅

高层方法：

- `subscribe_channel`
- `unsubscribe_channel`
- `list_subscriptions`

底层对应：

- `AttachmentType::ChannelSubscription`
- `upsert_attachment`
- `delete_attachment`
- `list_attachments`

也就是说，频道订阅本质上是用户附件的一种特例。

### 黑名单

高层方法：

- `block_user`
- `unblock_user`
- `list_blocked_users`

底层对应：

- `AttachmentType::UserBlacklist`

### RPC 与实时通道的边界

默认 `/ws/client` 上，`Client` 能覆盖绝大多数 HTTP 客户端能力。  
但如果你打开了 `realtime_stream=true`，就要注意服务端会拒绝很多管理 RPC，所以文档和代码里都应该把它当成“受限实时链路”，而不是“更快的通用链路”。

## 9. 错误模型

Rust SDK 统一使用 `turntf::Error`，其变体如下：

| 变体 | 含义 | 常见来源 |
| --- | --- | --- |
| `Validation` | 本地参数校验失败 | 空 `base_url`、非法 `UserRef`、空 body、非法 `DeliveryMode` |
| `Closed` | 客户端已关闭 | `close()` 之后再次调用 |
| `NotConnected` | 当前没有活跃 writer | 尚未连上或连接已被清理 |
| `Disconnected` | WebSocket 已断开 | 读循环退出、pending RPC 被整体失败 |
| `Server` | 服务端返回业务错误 | `unauthorized`、`forbidden`、`not_found` |
| `Protocol` | 协议内容不符合预期 | 缺字段、非法 frame、无效 JSON / protobuf |
| `Connection` | 建连、读写、HTTP 请求或超时错误 | `dial`、`write`、`request timeout` |
| `Store` | 本地游标存储失败 | `save_message`、`save_cursor`、`load_seen_messages` |

几个实用判断：

- 需要提示用户修正输入时，优先看 `Validation`
- 需要区分服务端鉴权失败时，看 `ServerError.code == "unauthorized"`
- 需要判断是否还能继续自动重连时，重点看 `Closed` 和 `Server(unauthorized)`
- `Store` 代表本地可靠性边界被破坏，通常比单次网络失败更值得告警

## 10. 共享语义注意点

在多语言 SDK 并行维护时，Rust 文档需要和服务端语义保持一致，尤其是下面这些点：

- `AckMessage` 只是连接内去重提示，不是服务端持久化确认；可靠重连依赖 `seen_messages`
- `save_message -> save_cursor -> ack` 的顺序不能改，否则会把“已确认但未落盘”的窗口重新引入
- `session_ref` 是会话级标识，不是用户稳定标识；重连后要重新获取
- `resolve_user_sessions` 的权限边界应与“是否允许给该用户发消息”保持一致
- `send_packet_to_session` 只适用于瞬时包，不适用于持久化消息
- `RelayAccepted` 只代表路由层受理，不代表用户侧投递成功
- `PacketPushed` 不应该混入消息游标表

如果以后 Rust SDK 在这些点上出现行为修改，应该同步检查：

- `turntf-rs/tests/client.rs`
- `turntf-rs/proto/client.proto`
- `turntf/docs/client-websocket.md`
