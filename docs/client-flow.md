# turntf-rs WebSocket 实时客户端流程

本文档详细说明 `Client` 的内部运行机制，包括连接生命周期、状态机转换、事件循环和重连策略。面向 SDK 维护者和需要深入理解连接行为的开发者。

## 1. 连接生命周期概述

`Client` 的生命周期可以分为以下几个阶段：

```
创建 -> Connect -> 登录 -> 读循环 + Ping循环 -> (断开 -> 重连 -> 登录 -> ...) -> Close
```

### 状态与状态机

`Client` 内部维护一个 `State` 结构体：

```rust
struct State {
    closed: bool,          // 是否已显式关闭
    connected: bool,       // 当前是否有活跃 WebSocket 连接
    stop_reconnect: bool,  // 是否停止自动重连
    terminal_error: Option<Error>,  // 终态错误（终态错误后不再重试）
}
```

通过 `connect()` 和 `close()` 两个公开方法驱动状态转换。

## 2. 连接建立流程（`connect()`）

`Client::connect()` 并非简单的 HTTP 握手，而是一个异步等待连接就绪的方法：

### 步骤分解

```
connect()
  ├── 检查 closed / connected 状态，快速返回
  ├── ensure_started()
  │     ├── 检查 run_task 是否已在运行
  │     ├── 重置 state（connected=false, stop_reconnect=false）
  │     └── 启动后台 run() 任务
  └── 状态等待循环
        ├── 检查 connected 状态 → 返回 Ok
        ├── 检查 terminal_error → 返回 Err
        ├── 检查 closed → 返回 Err
        └── 等待 state_notify 通知后继续循环
```

`ensure_started()` 确保后台 `run()` 任务已经启动。该任务是一个无限循环，负责：

1. 调用 `connect_and_serve()` 建立连接并处理消息
2. 连接正常结束时重新进入循环
3. 连接异常结束时判断是否应该重连

## 3. WebSocket 建连与登录（`connect_and_serve()`）

### 步骤分解

```
connect_and_serve()
  ├── 检查是否已关闭
  ├── 调用 cursor_store.load_seen_messages() 加载已确认游标
  ├── 根据 base_url + realtime_stream 推导 WebSocket URL
  │     ├── http://host       → ws://host/ws/client
  │     ├── https://host      → wss://host/ws/client
  │     ├── realtime_stream   → /ws/realtime 替换 /ws/client
  │     └── base 带路径前缀时保留前缀
  ├── connect_async() 建立 WebSocket 连接
  ├── 发送 LoginRequest（protobuf binary frame）
  │     ├── user: (node_id, user_id)
  │     ├── password: wire_value （bcrypt 编码后的密码）
  │     ├── seen_messages: 来自 cursor_store 的游标列表
  │     └── transient_only: 来自配置
  ├── 读取首帧 ServerEnvelope
  │     ├── LoginResponse → 提取 LoginInfo（user, session_ref）
  │     └── Error(code="unauthorized") → 设置 stop_reconnect=true
  ├── 初始化 writer（WsWriter，用于发送帧）
  ├── 设置 connected=true，通知等待者
  ├── 发出 ClientEvent::Login(LoginInfo)
  ├── 启动后台 ping 循环任务
  ├── 进入 read_loop（读取服务端帧并处理）
  ├── read_loop 退出后：
  │     ├── 取消 ping 任务
  │     ├── 设置 connected=false
  │     ├── 清理 writer（按 session_id 匹配）
  │     ├── 如果不是 closed 状态：
  │     │     ├── fail_all_pending（所有 pending RPC 返回 DisconnectedError）
  │     │     └── 发出 ClientEvent::Disconnect
  │     └── 返回 read_loop 的退出错误（或 Ok）
```

### 首帧协议约束

- 登录帧是客户端发送的第一个 `ClientEnvelope`，使用 `Body::Login`
- 服务端必须在同一连接上以 `LoginResponse` 或 `Error` 作为首帧回复
- 如果收到非预期的 `LoginResponse`（在已认证状态下），SDK 返回 `ProtocolError`

## 4. 消息读循环（`read_loop()`）

`read_loop()` 是一个无限循环，持续从 WebSocket 流中读取 `ServerEnvelope` 帧。

### 帧读取过程

```
read_loop()
  └── loop:
        ├── read_proto() 从 WebSocket 流读取下一帧
        │     ├── WsMessage::Binary → 解码为 ServerEnvelope
        │     ├── WsMessage::Ping/Pong → 跳过
        │     ├── WsMessage::Close → 返回 ConnectionError
        │     ├── Stream 结束 → 返回 ConnectionError
        │     └── Stream 错误 → 返回 ConnectionError
        └── handle_server_envelope() 处理帧
```

### 帧分发逻辑（`handle_server_envelope()`）

收到服务端帧后，按 `ServerEnvelope.body` 变体分发：

| 变体 | 处理逻辑 |
| --- | --- |
| `MessagePushed` | 持久化消息 → `persist_message` → 可选 ACK → `ClientEvent::Message` |
| `PacketPushed` | 瞬时包推送 → `ClientEvent::Packet`（不持久化、不 ACK） |
| `SendMessageResponse` | RPC 响应 → 解析 message 或 transient_accepted → 持久化 → resolve pending |
| `Pong` | Ping 响应 → resolve pending (RpcValue::Unit) |
| `CreateUserResponse` | RPC 响应 → resolve pending (RpcValue::User) |
| `GetUserResponse` | 同上 |
| `UpdateUserResponse` | 同上 |
| `DeleteUserResponse` | resolve pending (DeleteUserResult) |
| `GetUserMetadataResponse` | resolve pending (UserMetadata) |
| `UpsertUserMetadataResponse` | 同上 |
| `DeleteUserMetadataResponse` | 同上 |
| `ScanUserMetadataResponse` | resolve pending (UserMetadataScanResult) |
| `ListMessagesResponse` | resolve pending (Vec\<Message>) |
| `UpsertUserAttachmentResponse` | resolve pending (Attachment) |
| `DeleteUserAttachmentResponse` | 同上 |
| `ListUserAttachmentsResponse` | resolve pending (Vec\<Attachment>) |
| `ListEventsResponse` | resolve pending (Vec\<Event>) |
| `ListClusterNodesResponse` | resolve pending (Vec\<ClusterNode>) |
| `ListNodeLoggedInUsersResponse` | resolve pending (Vec\<LoggedInUser>) |
| `ResolveUserSessionsResponse` | resolve pending (ResolvedUserSessions) |
| `OperationsStatusResponse` | resolve pending (OperationsStatus) |
| `MetricsResponse` | resolve pending (String) |
| `Error` | 如果 request_id != 0 → resolve pending (Err)；否则 → 返回 Err（触发重连） |
| `LoginResponse` | 在已认证阶段收到 → 返回 ProtocolError |

### `persist_message` 的 ACK 顺序

收到 `MessagePushed` 或 `SendMessageResponse`（持久化消息）时：

```
persist_message(message)
  ├── cursor_store.save_message(message) — 保存完整消息
  ├── cursor_store.save_cursor(cursor) — 保存游标
  └── 如果 ack_messages=true 且 writer 可用：
        └── 发送 AckMessage { cursor }
```

顺序不可交换：ACK 必须发生在本地持久化之后。

## 5. Ping 循环（`ping_loop()`）

`ping_loop()` 是一个独立的后台任务，按 `ping_interval` 周期性发送 Ping RPC：

```
ping_loop()
  └── loop:
        ├── sleep(ping_interval) — 默认 30 秒
        ├── rpc(Ping) → 期望 RpcValue::Unit
        │     ├── Ok(Unit) → 继续循环
        │     ├── Ok(其他) → 发出 ClientEvent::Error(protocol)
        │     ├── NotConnected/Closed/Disconnected → 退出循环（连接已失效）
        │     └── 其他错误 → 发出 ClientEvent::Error(error)
        └── 继续
```

Ping 使用与普通 RPC 相同的 `pending` 通道和 `request_timeout` 超时机制。

## 6. Pending RPC 机制

所有长连接 RPC 通过 pending 表实现请求-响应匹配：

```
rpc(build_envelope)
  ├── request_id = atomic_fetch_add(1) + 1
  ├── 创建 oneshot channel (sender, receiver)
  ├── pending.insert(request_id, sender)
  ├── send_envelope(envelope)
  │     ├── 如果 closed → 移除 pending，返回 ClosedError
  │     ├── 如果没有活跃 writer → 移除 pending，返回 NotConnectedError
  │     └── 写入 WebSocket Binary 帧
  ├── timeout(request_timeout, receiver)
  │     ├── Ok(Ok(result)) → 移除 pending，返回 Ok(result)
  │     ├── Ok(Err(error)) → 移除 pending，返回 Err(error)
  │     └── Err(timeout) → 移除 pending，返回 ConnectionError("timed out")
  └── 返回
```

关键约束：
- `request_id` 从 1 开始递增，使用 `AtomicU64`
- 发送失败时立即移除 pending（避免已失败的请求残留）
- 连接断开时 `fail_all_pending` 批量失败所有 pending RPC
- 超时基于单个连接的 `request_timeout` 配置

## 7. 自动重连机制

重连逻辑由后台 `run()` 任务的循环管理：

```
run()
  └── loop:
        ├── connect_and_serve() — 尝试建立连接
        │     ├── 成功 → 重置退避延迟，继续循环（等待连接结束）
        │     └── 失败 → 进入错误处理
        └── 错误处理：
              ├── should_retry() 判断是否应重试
              │     ├── closed=true → 否
              │     ├── stop_reconnect=true → 否
              │     ├── reconnect=false → 否
              │     ├── Error::Closed → 否
              │     ├── Server(unauthorized) → 否（凭据问题）
              │     └── 其他 → 是
              ├── 不重试 → fail_all_pending + set_terminal_error → 退出
              ├── 重试 → emit ClientEvent::Error(error) → sleep(delay)
              └── delay = min(delay * 2, max_reconnect_delay)
```

### 重连指数退避

- 初始延迟：`initial_reconnect_delay`（默认 1 秒）
- 每次重试失败后加倍
- 上限：`max_reconnect_delay`（默认 30 秒）
- 连接成功（`connect_and_serve` 返回 Ok）后重置为初始值

### 重连时携带的信息

每次重连登录时，SDK 都会重新发送：
- `credentials.node_id` / `credentials.user_id`
- `credentials.password`（bcrypt 编码后的密码值）
- `cursor_store.load_seen_messages()` 返回的全部游标（通过 `seen_messages` 字段）
- `config.transient_only`

### 终止重连的条件

以下情况会终止重连并进入终态：
- 显式调用 `close()`
- `Config.reconnect = false`
- 登录阶段收到 `unauthorized` 错误（凭据问题）
- 连接返回 `Error::Closed`

## 8. 连接关闭流程（`close()`）

```
close()
  ├── 设置 state.closed = true, state.connected = false
  ├── 设置 terminal_error = ClosedError
  ├── notify_waiters() — 通知所有 connect() 等待者
  ├── emit ClientEvent::Disconnect(ClosedError)
  ├── fail_all_pending(ClosedError)
  ├── take_writer() — 移除 writer
  ├── 取消并等待 run_task 结束
  ├── 移除 events sender（events.write().await.take()）
  └── 返回 Ok
```

关闭后：
- `subscribe()` 返回的 BroadcastStream 不会再收到新事件
- 所有等待 `connect()` 的调用立即获得 `Err(ClosedError)`
- 后台任务被 abort
- `close()` 可以重复调用（第二次调用仅返回 Ok）

## 9. 事件流订阅（`subscribe()`）

`Client` 使用 `tokio::sync::broadcast` 实现多播事件流：

```
subscribe()
  ├── 读取 events sender
  │     ├── 如果 sender 存在 → BroadcastStream::new(sender.subscribe())
  │     └── 如果 sender 不存在（已关闭）→ 创建临时 channel 并返回接收端
  └── 返回 ClientSubscription（实现 Stream trait）
```

`ClientSubscription` 是 `BroadcastStream<ClientEvent>` 的封装，实现 `Stream<Item = Result<ClientEvent, BroadcastStreamRecvError>>`。

### Lagged 处理

当消费者处理速度跟不上事件产生速度时，`BroadcastStreamRecvError::Lagged(n)` 会被触发，表示跳过了 n 个事件。SDK 不会自动关闭 Lagged 的流，消费方继续读取仍能获得后续事件。

### 事件分类

- `Login(LoginInfo)` — 连接/重连登录成功
- `Message(Message)` — 收到持久化消息推送（已本地持久化）
- `Packet(Packet)` — 收到瞬时包推送（未持久化）
- `Error(Error)` — 内部错误（通常是重连前的通知）
- `Disconnect(Error)` — 连接关闭/断开

### 消费模式建议

```rust
let mut events = client.subscribe().await;
client.connect().await?;

while let Some(event) = events.next().await {
    match event? {
        ClientEvent::Login(info) => { /* 登录成功 */ }
        ClientEvent::Message(msg) => { /* 收到消息 */ }
        ClientEvent::Packet(pkt) => { /* 收到瞬时包 */ }
        ClientEvent::Error(err) => { /* 连接错误 */ }
        ClientEvent::Disconnect(err) => { break; /* 连接断开 */ }
    }
}
```

建议在 `connect()` 之前先 `subscribe()`，避免错过首次 `Login` 事件。

## 10. 连接地址推导规则

```rust
http://host               → ws://host/ws/client
https://host              → wss://host/ws/client
http://host/base          → ws://host/base/ws/client
http://host (realtime)    → ws://host/ws/realtime
ws://host                 → ws://host/ws/client
wss://host                → wss://host/ws/client
```

`realtime_stream=true` 将路径中的 `/ws/client` 替换为 `/ws/realtime`。

## 11. 并发安全模型

- `Client` 内部使用 `Arc<Inner>` 实现 Clone + Send + Sync
- `Arc<RwLock<Option<broadcast::Sender>>>` — events 发送端
- `Arc<Mutex<HashMap<u64, oneshot::Sender>>>` — pending RPC
- `Arc<Mutex<Option<ActiveWriter>>>` — WebSocket writer
- `Arc<Mutex<Option<JoinHandle>>>` — 后台任务句柄
- `Arc<Mutex<State>>` — 状态机
- `AtomicU64` — request_id 和 connection_id
- `Notify` — connect() 等待通知

## 12. 日志与监控建议

当前 Rust SDK 没有集成日志框架（如 `log`、`tracing`）。需要监控时，建议：

- 监听 `ClientEvent::Error` 和 `ClientEvent::Disconnect` 用于异常检测
- 监听 `ClientEvent::Login` 序列号变化判断重连频率
- 实现自定义 `CursorStore` 时可以在 `save_message` / `save_cursor` 中埋点
- 必要时在 `Client::new` 外部包装日志层统计 RPC 超时和 pending 积压

## 13. 测试中的模拟方法

集成测试使用 `turntf` 服务端的 `turntf-test` 包提供的 mock server，该 server 用 Go 实现。测试流程：

1. 测试启动时启动 mock server（通过 Go binary 或 Docker）
2. 使用 mock server 的地址构造 `HttpClient` 或 `Client`
3. 模拟各种协议场景（正常消息、重连、unauthorized、持久化失败等）

`RecordingStore`（定义在测试文件中）用于验证 `save_message` 和 `save_cursor` 的调用顺序。
