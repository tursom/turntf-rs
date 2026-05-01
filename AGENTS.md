# turntf-rs SDK 开发指南

## 项目概览

`turntf-rs` 是 `turntf` 分布式通知服务的 Rust SDK。面向两类接入场景：

- **HTTP JSON 管理/查询接口**：通过 `HttpClient` 封装 REST API，使用 Bearer Token 鉴权。
- **WebSocket + Protobuf 实时接口**：通过 `Client` 提供长连接会话，支持消息推送、自动重连、长连接 RPC 和事件广播。

### 技术栈

| 项目 | 版本/说明 |
| --- | --- |
| Rust Edition | 2021 |
| 异步运行时 | tokio (multi-thread) |
| HTTP 客户端 | reqwest (rustls-tls) |
| WebSocket | tokio-tungstenite (rustls-tls-webpki-roots) |
| Protobuf | prost 0.14 + prost-build 0.14 |
| protoc | protoc-bin-vendored（编译期自动下载，无需系统预装） |
| 序列化 | serde / serde_json |
| 错误处理 | thiserror |

### 当前状态

- 不包含 ZeroMQ 客户端。
- 不直接公开生成后的 protobuf 类型；对外暴露的是更贴近 Rust 业务代码的类型、错误模型和订阅接口。
- 通过 `MemoryCursorStore` 提供默认的本地游标存储，业务方可以实现 `CursorStore` trait 对接自己的持久化存储。

## 构建与测试命令

### 编译

```bash
cargo build
```

### 运行全部测试

```bash
cargo test
```

### 常用定向测试

```bash
# 仅 HTTP 集成测试
cargo test --test http

# 仅 Client 集成测试
cargo test --test client

# 验证重连与 seen_messages
cargo test --test client client_reconnects_with_seen_messages_and_realtime_path

# 验证 session_ref 定向瞬时包
cargo test --test client client_resolves_sessions_and_targets_transient_delivery

# 验证登录、消息 ACK、ping 等基础链路
cargo test --test client client_login_message_ack_send_packet_create_user_and_ping

# 验证持久化失败时不 ACK
cargo test --test client client_does_not_ack_on_persist_failure_and_emits_error
```

### 生成 API 文档

```bash
cargo doc --no-deps
```

### 发布

```bash
cargo publish
```

发布前需要：
1. 确保 `Cargo.toml` 中的 `version` 已更新
2. 检查依赖项版本是否兼容
3. 确保所有测试通过
4. 确保 `README.md` 和文档内容与当前版本一致

## Proto 生成说明

Proto 文件位于 `proto/client.proto`，编译期由 `build.rs` 通过 `prost-build` 自动生成 Rust 代码。

### build.rs 工作流程

1. 定位 `proto/client.proto`
2. 通过 `protoc-bin-vendored` 找到 vendored protoc 二进制路径
3. 设置 `PROTOC` 环境变量
4. 使用 `prost_build::Config` 配置 `bytes(["."])`（所有字段使用 `bytes::Bytes` 而非 `Vec<u8>`）
5. 调用 `compile_protos` 生成 Rust 代码到 `OUT_DIR`

关键配置：
```rust
config.bytes(["."]);  // 所有 proto bytes 字段映射为 prost::bytes::Bytes
```

生成后的代码通过以下方式引入：
```rust
pub(crate) mod proto {
    include!(concat!(env!("OUT_DIR"), "/notifier.client.v1.rs"));
}
```

### 修改 proto 后的步骤

1. 重新执行 `cargo build` 即可触发生成
2. 检查 `src/mapping.rs` 是否完整覆盖新类型/新字段的映射
3. 检查 `src/client.rs` 和 `src/http.rs` 是否需要新增或调整公开 API
4. 如果共享语义发生变化，同步更新 `docs/` 下的文档

### 无需系统 protoc

本模块使用 `protoc-bin-vendored`，会在编译期自动下载 vendored protoc，不需要开发机预装系统级 `protoc`。

## Crate 结构

```
turntf-rs/
├── Cargo.toml          # 包配置与依赖声明
├── build.rs            # 构建脚本：protobuf 代码生成
├── proto/
│   └── client.proto    # protobuf 协议定义（本地副本）
├── src/
│   ├── lib.rs          # 库入口，公开 API 导出
│   ├── client.rs       # Client：WebSocket 长连接客户端
│   ├── http.rs         # HttpClient：REST JSON 客户端
│   ├── mapping.rs      # proto/Rust/HTTP JSON 三方类型映射
│   ├── types.rs        # 公开数据类型定义
│   ├── errors.rs       # 统一错误模型
│   ├── password.rs     # 密码编码（bcrypt）
│   ├── store.rs        # CursorStore trait 与 MemoryCursorStore
│   └── validation.rs   # 本地参数校验
├── tests/
│   ├── client.rs       # Client 集成测试
│   └── http.rs         # HttpClient 集成测试
└── docs/
    ├── guide.md              # 使用与语义说明
    ├── build-and-test.md     # 依赖、构建与测试说明
    ├── client-flow.md        # WebSocket 实时客户端流程
    ├── websocket-protocol.md # WebSocket + Protobuf 协议详情
    └── api-reference.md      # 主要类型 API 参考
```

### 模块职责

| 模块 | 职责 |
| --- | --- |
| `lib.rs` | 库入口，声明内部模块并公开公共 API |
| `client.rs` | `Client`、`Config`、`ClientEvent`、`ClientSubscription` — WebSocket 长连接客户端完整实现 |
| `http.rs` | `HttpClient` — REST JSON API 封装 |
| `mapping.rs` | Protobuf 生成类型与 Rust 公开类型之间的双向转换，以及 HTTP JSON 响应的反序列化 |
| `types.rs` | 所有公开数据类型（`UserRef`、`Message`、`SessionRef`、`Credentials` 等） |
| `errors.rs` | `Error` 枚举（8 种变体）及辅助类型 |
| `password.rs` | `PasswordInput` 类型与密码编码函数（`plain_password`、`hashed_password`、`hash_password`） |
| `store.rs` | `CursorStore` trait（`save_message`、`save_cursor`、`load_seen_messages`）与内存默认实现 |
| `validation.rs` | 参数校验函数（`validate_user_ref`、`validate_session_ref`、`validate_delivery_mode` 等） |
| `tests/client.rs` | Client 集成测试，覆盖登录、消息、ACK、重连、session_ref、Lagged 等场景 |
| `tests/http.rs` | HttpClient 集成测试，覆盖用户 CRUD、订阅、消息、事件等 REST 接口 |

## 关键 API 面

### TurntfClient（`client::Client`）

长连接实时客户端。通过 WebSocket binary frame 发送 protobuf，支持自动重连、消息推送、pending RPC 和事件广播。

**构造与生命周期：**
- `Client::new(config: Config) -> Result<Self>` — 创建客户端实例，立即进行本地参数校验
- `client.connect().await -> Result<()>` — 启动后台连接任务，等待进入 connected 状态
- `client.close().await -> Result<()>` — 关闭连接，清理所有 pending RPC，终止后台任务
- `client.subscribe().await -> ClientSubscription` — 获取事件流订阅

**用户管理（长连接 RPC）：**
- `create_user(CreateUserRequest) -> Result<User>`
- `create_channel(CreateUserRequest) -> Result<User>` — role 默认设为 `"channel"`
- `get_user(UserRef) -> Result<User>`
- `update_user(UserRef, UpdateUserRequest) -> Result<User>`
- `delete_user(UserRef) -> Result<DeleteUserResult>`

**消息与瞬时包：**
- `send_message(UserRef, Vec<u8>) -> Result<Message>` — 发送持久化消息，服务端返回后自动保存游标
- `send_packet(UserRef, Vec<u8>, DeliveryMode) -> Result<RelayAccepted>`
- `send_packet_to_session(UserRef, Vec<u8>, DeliveryMode, SessionRef) -> Result<RelayAccepted>`

**用户元数据：**
- `get_user_metadata(UserRef, key) -> Result<UserMetadata>`
- `upsert_user_metadata(UserRef, key, UpsertUserMetadataRequest) -> Result<UserMetadata>`
- `delete_user_metadata(UserRef, key) -> Result<UserMetadata>`
- `scan_user_metadata(UserRef, ScanUserMetadataRequest) -> Result<UserMetadataScanResult>`

**订阅与黑名单（附件抽象）：**
- `subscribe_channel(UserRef, UserRef) / unsubscribe_channel / list_subscriptions`
- `block_user(UserRef, UserRef) / unblock_user / list_blocked_users`
- `upsert_attachment / delete_attachment / list_attachments`

**查询与运维：**
- `list_messages(UserRef, limit) -> Result<Vec<Message>>`
- `list_events(after, limit) -> Result<Vec<Event>>`
- `list_cluster_nodes() -> Result<Vec<ClusterNode>>`
- `list_node_logged_in_users(node_id) -> Result<Vec<LoggedInUser>>`
- `resolve_user_sessions(UserRef) -> Result<ResolvedUserSessions>`
- `operations_status() -> Result<OperationsStatus>`
- `metrics() -> Result<String>`
- `ping() -> Result<()>`

**HTTP 桥接方法：**
- `login(node_id, user_id, password) -> Result<String>` — 委托给内部 HttpClient
- `login_with_password(node_id, user_id, PasswordInput) -> Result<String>`
- `http() -> HttpClient` — 返回内部 HttpClient 的克隆

**事件流（ClientEvent 枚举）：**
- `Login(LoginInfo)` — 登录成功
- `Message(Message)` — 收到持久化消息推送
- `Packet(Packet)` — 收到瞬时包推送
- `Error(Error)` — SDK 内部错误
- `Disconnect(Error)` — 连接断开

**Config 关键字段：**

| 字段 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `base_url` | `String` | — | 服务端 HTTP 基础地址 |
| `credentials` | `Credentials` | — | 登录凭据（node_id, user_id, password） |
| `cursor_store` | `Arc<dyn CursorStore>` | MemoryCursorStore | 消息游标持久化 |
| `reconnect` | `bool` | `true` | 断线后自动重连 |
| `initial_reconnect_delay` | `Duration` | `1s` | 首次重连等待时间 |
| `max_reconnect_delay` | `Duration` | `30s` | 指数退避上限 |
| `ping_interval` | `Duration` | `30s` | 应用层 ping 周期 |
| `request_timeout` | `Duration` | `10s` | 单个 RPC 等待超时 |
| `ack_messages` | `bool` | `true` | 收到消息后自动 ACK |
| `transient_only` | `bool` | `false` | 登录时声明只关心瞬时消息 |
| `realtime_stream` | `bool` | `false` | 使用 `/ws/realtime` 路径 |
| `event_channel_capacity` | `usize` | `256` | broadcast 通道缓冲区大小 |

### TurntfHttpClient（`http::HttpClient`）

短连接 REST 客户端。通过 Bearer Token 调用 HTTP JSON 接口。

**构造：**
- `HttpClient::new(base_url) -> Result<Self>`

**认证：**
- `login(node_id, user_id, password) -> Result<String>` — 使用明文密码，自动做 bcrypt 编码
- `login_with_password(node_id, user_id, PasswordInput) -> Result<String>`

**用户管理：**
- `create_user(token, CreateUserRequest) -> Result<User>`
- `get_user(token, UserRef) -> Result<User>`
- `update_user(token, UserRef, UpdateUserRequest) -> Result<User>`
- `delete_user(token, UserRef) -> Result<DeleteUserResult>`

**消息与瞬时包：**
- `post_message(token, UserRef, Vec<u8>) -> Result<Message>`
- `post_packet(token, target_node_id, UserRef, Vec<u8>, DeliveryMode) -> Result<RelayAccepted>`
- `list_messages(token, UserRef, limit) -> Result<Vec<Message>>`

**订阅、黑名单、附件：** 与 Client 同名方法，但需要显式传入 `token`

**查询与运维：**
- `list_events`, `list_cluster_nodes`, `list_node_logged_in_users`
- `operations_status`, `metrics`

### 核心数据类型

均定义在 `src/types.rs` 中，通过 `lib.rs` 公开导出。

**标识类型：**
- `UserRef { node_id, user_id }` — 用户标识
- `SessionRef { serving_node_id, session_id }` — 会话标识
- `MessageCursor { node_id, seq }` — 消息游标

**消息/包类型：**
- `Message { recipient, node_id, seq, sender, body, created_at_hlc }`
- `Packet { packet_id, source_node_id, target_node_id, recipient, sender, body, delivery_mode, target_session }`
- `RelayAccepted { packet_id, source_node_id, target_node_id, recipient, delivery_mode, target_session }`

**配置/请求类型：**
- `Credentials { node_id, user_id, password: PasswordInput }`
- `Config` — 客户端完整配置
- `CreateUserRequest`, `UpdateUserRequest`
- `UpsertUserMetadataRequest`, `ScanUserMetadataRequest`

**响应类型：**
- `LoginInfo { user, protocol_version, session_ref }`
- `User`, `UserMetadata`, `UserMetadataScanResult`
- `Subscription`, `BlacklistEntry`, `Attachment`
- `Event`, `ClusterNode`, `LoggedInUser`
- `ResolvedUserSessions { user, presence, sessions }`
- `DeleteUserResult`
- `OperationsStatus`

**枚举：**
- `DeliveryMode { Unspecified, BestEffort, RouteRetry }`
- `AttachmentType { ChannelManager, ChannelWriter, ChannelSubscription, UserBlacklist }`
- `PasswordSource { Plain, Hashed }`
- `ClientEvent { Login, Message, Packet, Error, Disconnect }`

### 错误类型

定义在 `src/errors.rs` 中，统一使用 `turntf::Error`：

| 变体 | 含义 | 附加字段 |
| --- | --- | --- |
| `Validation` | 本地参数校验失败 | `message: String` |
| `Closed` | 客户端已关闭 | — |
| `NotConnected` | 没有活跃的 WebSocket writer | — |
| `Disconnected` | WebSocket 连接已断开 | — |
| `Server` | 服务端返回业务错误 | `code`, `server_message`, `request_id` |
| `Protocol` | 协议内容不符合预期 | `protocol_message` |
| `Connection` | 建连、读写、HTTP 请求或超时错误 | `op`, `cause` |
| `Store` | 本地游标存储失败 | `op`, `message` |

判断 `unauthorized` 的方法：`matches!(error, Error::Server(err) if err.unauthorized())`。

### CursorStore trait

```rust
#[async_trait]
pub trait CursorStore: Send + Sync {
    async fn load_seen_messages(&self) -> Result<Vec<MessageCursor>, BoxError>;
    async fn save_message(&self, message: Message) -> Result<(), BoxError>;
    async fn save_cursor(&self, cursor: MessageCursor) -> Result<(), BoxError>;
}
```

自带 `MemoryCursorStore` 默认实现。业务方可以实现此 trait 对接数据库或文件存储。

## 发布说明

### 发布到 crates.io

```bash
# 1. 确保版本号已更新
# 2. 确保所有测试通过
cargo test

# 3. 检查文档
cargo doc --no-deps --open

# 4. 发布
cargo publish
```

### 版本号规范

遵循语义化版本（SemVer）：

- **补丁版本**（0.1.0 -> 0.1.1）：bug 修复、内部重构、文档更新
- **次要版本**（0.1.0 -> 0.2.0）：新增功能、新增公开 API、现有功能增强
- **主要版本**（0.x -> 1.0）：重大 API 变更或架构重构

### 发布前的检查清单

- [ ] `Cargo.toml` 版本号已更新
- [ ] `CHANGELOG.md`（如果有）已更新
- [ ] `cargo test` 全部通过
- [ ] `cargo doc --no-deps` 无文档链接错误
- [ ] 与根仓库 `proto/client.proto` 或 `turntf/proto/client.proto` 同步
- [ ] `docs/` 目录文档与当前行为一致
- [ ] 如有共享语义变更，同步更新 `turntf/docs/client-websocket.md`

## 代码约定

### 命名

- **类型**：大驼峰（`PascalCase`），如 `UserRef`、`HttpClient`、`ClientEvent`
- **函数/方法**：蛇形（`snake_case`），如 `send_message`、`plain_password`
- **枚举变体**：大驼峰，如 `DeliveryMode::BestEffort`
- **模块**：蛇形，与文件名一致
- **错误类型**：`Error` 枚举 + 各变体对应的结构体
- **trait**：大驼峰，如 `CursorStore`

### 公开 API

- 所有公开类型和方法应具有清晰的自文档性
- 方法签名优先使用 `impl Into<String>` 而非 `&str`（对调用者更友好）
- 参数校验在方法入口立即执行，不依赖服务端校验
- 返回类型统一使用 `Result<T>`（即 `std::result::Result<T, Error>`）
- 错误类型优先使用具体的 `Error` 构造器（`Error::validation()`、`Error::protocol()` 等）

### 内部实现

- `pub(crate)` 用于模块间共享但不对外暴露的项
- protobuf 生成类型仅在 `proto` 模块内可见
- 类型映射集中在 `mapping.rs` 中，不分散在各调用处
- WebSocket 帧读写使用独立函数（`write_proto`、`read_proto`），不在 `Client` 方法内内联
- pending RPC 使用 `HashMap<u64, oneshot::Sender>`，通过 request_id 匹配

### 测试

- 集成测试放在 `tests/` 目录
- 使用 `RecordingStore` 等测试辅助类型验证 `save_message -> save_cursor -> ack` 顺序
- 测试名称使用长蛇形风格，描述测试场景，如 `client_reconnects_with_seen_messages_and_realtime_path`
- 每个测试函数只验证一个核心语义

### 文档

- `README.md`：项目概述、快速开始、能力分层对照表
- `docs/guide.md`：面向用户的使用与语义说明
- `docs/build-and-test.md`：面向维护者的构建与测试说明
- `docs/client-flow.md`：WebSocket 实时客户端连接流程
- `docs/websocket-protocol.md`：WebSocket + Protobuf 协议细节
- `docs/api-reference.md`：主要类型 API 参考
- Rustdoc 注释覆盖所有 `pub` 项

### 依赖管理

- 避免引入非必要的依赖
- 使用 `default-features = false` + 按需 feature
- 构建期依赖（`prost-build`、`protoc-bin-vendored`）放在 `[build-dependencies]` 而非 `[dependencies]`
- 更新依赖时注意与 `prost` 0.14 系列的兼容性

## 提交约定

### 提交消息格式

```
<type>: <简短描述>

<可选：详细说明>

Co-Authored-By: tursom <tursom@foxmail.com>
```

### 类型标记

- `feat`：新增功能或公开 API
- `fix`：bug 修复
- `docs`：文档变更（README、docs/、rustdoc）
- `refactor`：代码重构，不改变外部行为
- `test`：测试变更
- `chore`：构建配置、依赖更新、CI 等
- `style`：代码格式变更
- `proto`：protobuf 协议定义变更

### 示例

```
feat: 添加 send_packet_to_session 定向投递接口

实现 WebSocket 长连接上的 session 级定向瞬时包投递，
支持 resolve_user_sessions 查询在线会话列表。

Co-Authored-By: tursom <tursom@foxmail.com>
```

```
fix: 修复重连时 seen_messages 可能丢失的问题

load_seen_messages 在连接建立前调用，若 store 返回空列表则
不会在 LoginRequest 中携带已确认游标。

Co-Authored-By: tursom <tursom@foxmail.com>
```

```
docs: 补充 client-flow.md 和 websocket-protocol.md

新增 WebSocket 实时客户端连接流程文档和 Protobuf 协议详情，
帮助维护者理解连接状态机和帧交互规范。

Co-Authored-By: tursom <tursom@foxmail.com>
```

### 注意事项

- 提交负责人必须是 `tursom <tursom@foxmail.com>`
- 每个提交聚焦一个逻辑变更
- 修改 proto 文件的提交应与对应生成的代码变更同在一个提交中
- 跨模块变更（同时修改 Rust SDK 和服务端 proto）应先在对应仓库分别提交
