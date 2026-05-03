# turntf-rs

`turntf-rs` 是 `turntf` 实时消息推送平台的 Rust SDK，面向两类接入场景：

- **HTTP REST 客户端**（`HttpClient`）—— 用户管理、元数据操作、消息收发、频道订阅、集群状态查询等管理、查询与运维调用。
- **WebSocket + Protobuf 实时客户端**（`Client`）—— 长连接消息推送、自动重连、会话定向瞬时包（数据包）、事件广播、消息可靠消费与 ACK 游标控制。

当前实现不包含 ZeroMQ 客户端，也不直接公开生成后的 protobuf 类型；对外暴露的是更贴近 Rust 业务代码的类型、错误模型和订阅接口。

## 安装

```toml
[dependencies]
turntf = { path = "../turntf-rs" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tokio-stream = "0.1"
```

密码工具函数：

- `plain_password(...)` —— 在客户端本地把明文密码编码成服务端接受的 bcrypt 字符串。
- `hashed_password(...)` —— 如果你已经持有编码后的密码值，可以直接传入。

HTTP 和 WebSocket 都支持两条登录路径：

- 旧方式：`node_id + user_id + password`
- 新方式：`login_name + password`

`username` 仅用于展示和用户资料管理，不参与认证。

## 快速开始

### HttpClient（HTTP REST API）

用于管理接口、后台任务和一次性查询。每次请求需要携带通过登录获取的 Bearer Token。

```rust
use turntf::{plain_password, CreateUserRequest, HttpClient};

#[tokio::main]
async fn main() -> Result<(), turntf::Error> {
    // 创建 HTTP 客户端，指定服务器基础地址
    let client = HttpClient::new("http://127.0.0.1:8080")?;

    // 使用 node_id + user_id + 明文密码登录，获取 token
    let token = client.login(4096, 1, "root").await?;

    // 创建新用户
    let user = client
        .create_user(
            &token,
            CreateUserRequest {
                username: "alice".into(),
                login_name: Some("alice.login".into()),
                password: Some(plain_password("alice-password")?),
                profile_json: br#"{"tier":"gold"}"#.to_vec(),
                role: "user".into(),
            },
        )
        .await?;

    println!("created user {}:{}", user.node_id, user.user_id);

    // 查询集群节点状态
    let nodes = client.list_cluster_nodes(&token).await?;
    println!("cluster has {} nodes", nodes.len());

    // 查询运维状态
    let status = client.operations_status(&token).await?;
    println!("write gate ready: {}", status.write_gate_ready);

    Ok(())
}
```

如果你要走新的登录名认证：

```rust
let token = client.login_by_login_name("root.login", "root-password").await?;
```

`HttpClient` 通过 `reqwest` 发送 HTTP 请求，完整方法列表包括：

| 类别 | 方法 |
|------|------|
| 认证 | `login`, `login_by_login_name`, `login_with_password`, `login_by_login_name_with_password` |
| 用户管理 | `create_user`, `get_user`, `update_user`, `delete_user`, `create_channel`, `list_users` |
| 元数据 | `get_user_metadata`, `upsert_user_metadata`, `delete_user_metadata`, `scan_user_metadata` |
| 消息收发 | `list_messages`, `post_message`, `post_packet` |
| 频道订阅 | `subscribe_channel`, `unsubscribe_channel`, `list_subscriptions`, `create_subscription` |
| 黑名单 | `block_user`, `unblock_user`, `list_blocked_users` |
| 集群运维 | `list_cluster_nodes`, `list_node_logged_in_users`, `list_events`, `operations_status`, `metrics` |
| 附件 | `upsert_attachment`, `delete_attachment`, `list_attachments` |

### Client（WebSocket 实时客户端）

用于需要实时推送、自动重连、持久化消息接收和会话定向瞬时包的长连接场景。

```rust
use tokio_stream::StreamExt;
use turntf::{plain_password, Client, ClientEvent, Config, Credentials, UserRef};

#[tokio::main]
async fn main() -> Result<(), turntf::Error> {
    // 创建长连接客户端，配置服务器地址和登录凭据
    let client = Client::new(Config::new(
        "http://127.0.0.1:8080",
        Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password")?,
        },
    ))?;

    // 建立 WebSocket 连接（自动发送登录帧进行认证）
    client.connect().await?;

    // 订阅事件流
    let mut events = client.subscribe().await;

    // 通过事件驱动的方式处理消息
    while let Some(event) = events.next().await {
        match event? {
            ClientEvent::Login(info) => {
                println!("login ok, session: {}", info.session_ref.session_id);
            }
            ClientEvent::Message(message) => {
                println!("received message {}:{}", message.node_id, message.seq);
                // SDK 会自动持久化消息、保存游标、发送 ACK
            }
            ClientEvent::Packet(packet) => {
                println!("received packet {}", packet.packet_id);
            }
            ClientEvent::Error(error) => {
                eprintln!("client error: {error}");
            }
            ClientEvent::Disconnect(error) => {
                eprintln!("disconnected: {error}");
                break;
            }
        }
    }

    Ok(())
}
```

如果长连接也要使用 `login_name` 登录：

```rust
let client = Client::new(Config::new_with_login_name(
    "http://127.0.0.1:8080",
    "alice.login",
    plain_password("alice-password")?,
))?;
```

`Client` 在后台自动管理连接生命周期，具备以下开箱即用的能力：

- **自动重连** —— 指数退避策略，网络恢复后自动重建 WebSocket 并重新登录。
- **消息去重** —— 重连时将本地 `CursorStore` 中的已确认游标发送给服务端，避免重复投递。
- **事件驱动** —— 通过 `subscribe()` 返回的 `ClientSubscription` 流接收事件，支持多消费者 broadcast 通道。

### 会话定向瞬时包

`Client` 支持先解析用户的在线会话，然后向特定会话发送瞬时数据包（如"正在输入"提示、实时位置更新等）。

```rust
use turntf::{Client, DeliveryMode, Error, SessionRef, UserRef};

async fn send_targeted_packet(client: &Client, target: UserRef) -> Result<(), Error> {
    // 解析目标用户的所有活跃会话
    let resolved = client.resolve_user_sessions(target.clone()).await?;

    // 选取第一个会话作为目标
    let session: SessionRef = resolved
        .sessions
        .first()
        .ok_or_else(|| Error::protocol("target user is offline"))?
        .session
        .clone();

    // 向该会话发送瞬时数据包（BestEffort 模式，不保证送达）
    client
        .send_packet_to_session(
            target,
            b"hello session".to_vec(),
            DeliveryMode::BestEffort,
            session,
        )
        .await?;

    Ok(())
}
```

## API 概览

### 核心类型

| 类型 | 说明 |
|------|------|
| `HttpClient` | HTTP REST API 客户端，无需长连接即可完成管理和查询操作 |
| `Client` | WebSocket 实时客户端，支持自动重连和事件驱动的消息处理 |
| `Config` | `Client` 的配置项，包括凭据、游标存储、重连策略、心跳间隔等 |
| `Credentials` | 用户凭据（`node_id`, `user_id`, `password`） |
| `UserRef` | 用户引用（`node_id`, `user_id`），所有用户操作通过此类型定位目标 |
| `SessionRef` | 会话引用（`serving_node_id`, `session_id`），用于定向发送 |
| `Message` | 持久化消息，包含游标信息用于去重和 ACK |
| `Packet` | 瞬时数据包，不持久化，适用于实时通信 |
| `ClientEvent` | 事件枚举：`Login`, `Message`, `Packet`, `Error`, `Disconnect` |
| `DeliveryMode` | 投递模式：`BestEffort`（尽力投递）、`RouteRetry`（路由重试） |
| `CursorStore` | 游标存储 trait，默认提供 `MemoryCursorStore` 可自定义持久化实现 |

### Client 长连接 RPC API

`Client` 的大部分业务方法通过 WebSocket RPC 完成，无需额外获取 HTTP token：

| 类别 | 方法 |
|------|------|
| 生命周期 | `new`, `connect`, `close`, `subscribe`, `http` |
| 消息收发 | `send_message` / `post_message`, `send_packet`, `send_packet_to_session`, `list_messages` |
| 用户管理 | `create_user`, `get_user`, `update_user`, `delete_user`, `create_channel`, `list_users` |
| 元数据 | `get_user_metadata`, `upsert_user_metadata`, `delete_user_metadata`, `scan_user_metadata` |
| 频道订阅 | `subscribe_channel`, `create_subscription`, `unsubscribe_channel`, `list_subscriptions` |
| 黑名单 | `block_user`, `unblock_user`, `list_blocked_users` |
| 会话 | `resolve_user_sessions` |
| 集群 | `list_cluster_nodes`, `list_node_logged_in_users`, `list_events`, `operations_status`, `metrics` |
| 工具 | `ping`（心跳检测） |

## 能力对比

| 能力 | `HttpClient` | `Client` |
| --- | --- | --- |
| HTTP 登录拿 token | 支持 | 通过 `Client::login*` 代理到 `HttpClient` |
| 用户管理 / 查询 | 支持 | 支持，走长连接 RPC |
| 历史消息 / 事件 / 运维查询 | 支持 | 支持，走长连接 RPC |
| `MessagePushed` / `PacketPushed` 接收 | 不支持 | 支持 |
| 自动重连 / 重登录 | 不支持 | 支持（指数退避） |
| `session_ref` / `resolve_user_sessions` | 不支持 | 支持 |
| `send_packet_to_session` | 不支持 | 支持 |
| 本地游标存储与 ACK 顺序控制 | 不支持 | 支持 |
| Prometheus 监控指标 | 支持（`metrics`） | 支持（`metrics`） |

**选择建议**：

- 如果你只需要管理接口或后台任务，优先使用 `HttpClient`，它更轻量、无需维持长连接。
- 如果你需要实时推送、自动重连或会话定向瞬时包，使用 `Client`。
- 也可以在 `Client` 实例上通过 `client.http()` 获取内部的 `HttpClient` 实例，同时拥有两种能力。

## 共享语义速记

- `Client` 长连接鉴权使用首帧 `LoginRequest`，不是 HTTP token。
- `list_users` 返回当前登录用户可通讯的活跃用户集合；通过 `ListUsersRequest { name, uid }` 传过滤条件，其中空白 `name` 和 `uid={0,0}` 都表示“不过滤”。
- 收到持久化消息时，Rust SDK 会先执行 `save_message -> save_cursor`，只有两步都成功后才会在 `ack_messages=true` 时发送 `AckMessage`。
- `PacketPushed` 不参与消息游标、不会补发、也不会自动 ACK。
- 自动重连会重新发送登录帧，并把 `cursor_store.load_seen_messages()` 返回的游标放进 `seen_messages`。
- `session_ref` 是一次登录会话的标识；重连或重登录后可能变化，不能把旧值当成长期稳定 ID。
- `realtime_stream=true` 会改走 `/ws/realtime`，此时服务端只允许瞬时消息和少量查询 RPC，不适合作为通用管理通道。

## 文档导航

- [使用与语义说明](docs/guide.md)
- [依赖、构建与测试说明](docs/build-and-test.md)

## 构建与测试

```bash
cargo build
cargo test
```

常用定向测试：

```bash
cargo test --test client client_resolves_sessions_and_targets_transient_delivery
cargo test --test client client_reconnects_with_seen_messages_and_realtime_path
cargo test --test http
```

本模块没有额外的 shell 构建脚本；Cargo 编译阶段会自动执行 [`build.rs`](build.rs)，使用 vendored `protoc` 把 [`proto/client.proto`](proto/client.proto) 生成为 Rust 代码。详细说明见 [docs/build-and-test.md](docs/build-and-test.md)。
