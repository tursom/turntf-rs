# turntf-rs

`turntf-rs` 是 `turntf` 的 Rust SDK，面向两类接入场景：

- 基于 HTTP JSON 的管理、查询与运维调用
- 基于 WebSocket + Protobuf 的实时连接、消息推送与长连接 RPC

当前实现不包含 ZeroMQ 客户端，也不直接公开生成后的 protobuf 类型；对外暴露的是更贴近 Rust 业务代码的类型、错误模型和订阅接口。

## 模块定位

- `HttpClient`：面向短连接 REST API，使用 Bearer Token 调用 HTTP 接口。
- `Client`：面向长连接实时会话，负责登录、收消息、发消息、自动重连、长连接 RPC 和事件广播。
- `CursorStore`：面向可靠消费语义，负责保存已持久化消息与 `(node_id, seq)` 游标。

如果你只需要管理接口或后台任务，优先使用 `HttpClient`。如果你需要实时推送、自动重连或会话定向瞬时包，使用 `Client`。

## 文档导航

- [使用与语义说明](docs/guide.md)
- [依赖、构建与测试说明](docs/build-and-test.md)

## 能力分层

| 能力 | `HttpClient` | `Client` |
| --- | --- | --- |
| HTTP 登录拿 token | 支持 | 通过 `Client::login*` 代理到 `HttpClient` |
| 用户管理 / 查询 | 支持 | 支持，走长连接 RPC |
| 历史消息 / 事件 / 运维查询 | 支持 | 支持，走长连接 RPC |
| `MessagePushed` / `PacketPushed` 接收 | 不支持 | 支持 |
| 自动重连 / 重登录 | 不支持 | 支持 |
| `session_ref` / `resolve_user_sessions` | 不支持 | 支持 |
| `send_packet_to_session` | 不支持 | 支持 |
| 本地游标存储与 ACK 顺序控制 | 不支持 | 支持 |

## 安装

```toml
[dependencies]
turntf = { path = "../turntf-rs" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tokio-stream = "0.1"
```

`plain_password(...)` 会在客户端本地把明文密码编码成服务端接受的 bcrypt 字符串；如果你已经持有编码后的密码值，可以使用 `hashed_password(...)`。

HTTP 和 WebSocket 现在都支持两条登录路径：

- 旧方式：`node_id + user_id + password`
- 新方式：`login_name + password`

`username` 仅用于展示和用户资料管理，不参与认证。

## 快速开始

### HTTP 客户端

```rust
use turntf::{plain_password, CreateUserRequest, HttpClient};

#[tokio::main]
async fn main() -> Result<(), turntf::Error> {
    let client = HttpClient::new("http://127.0.0.1:8080")?;
    let token = client.login(4096, 1, "root").await?;

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
    Ok(())
}
```

如果你要走新的登录名认证，可以直接调用：

```rust
let token = client.login_by_login_name("root.login", "root-password").await?;
```

### 实时客户端

```rust
use tokio_stream::StreamExt;
use turntf::{plain_password, Client, ClientEvent, Config, Credentials};

#[tokio::main]
async fn main() -> Result<(), turntf::Error> {
    let client = Client::new(Config::new(
        "http://127.0.0.1:8080",
        Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password")?,
        },
    ))?;

    let mut events = client.subscribe().await;
    client.connect().await?;

    while let Some(event) = events.next().await {
        match event? {
            ClientEvent::Login(info) => {
                println!("login ok: {}", info.session_ref.session_id);
            }
            ClientEvent::Message(message) => {
                println!("message {}:{}", message.node_id, message.seq);
            }
            ClientEvent::Packet(packet) => {
                println!("packet {}", packet.packet_id);
            }
            ClientEvent::Error(error) => {
                eprintln!("client error: {error}");
            }
            ClientEvent::Disconnect(error) => {
                eprintln!("disconnect: {error}");
                break;
            }
        }
    }

    Ok(())
}
```

如果长连接也要使用 `login_name` 登录，可以改用：

```rust
let client = Client::new(Config::new_with_login_name(
    "http://127.0.0.1:8080",
    "alice.login",
    plain_password("alice-password")?,
))?;
```

### 会话定向瞬时包

```rust
use turntf::{Client, DeliveryMode, Error, SessionRef, UserRef};

async fn send_targeted_packet(client: &Client, target: UserRef) -> Result<(), Error> {
    let resolved = client.resolve_user_sessions(target.clone()).await?;
    let session: SessionRef = resolved
        .sessions
        .first()
        .ok_or_else(|| Error::protocol("target user is offline"))?
        .session
        .clone();

    client
        .send_packet_to_session(target, b"hello session".to_vec(), DeliveryMode::BestEffort, session)
        .await?;
    Ok(())
}
```

更完整的语义说明见 [docs/guide.md](docs/guide.md)。

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

## 共享语义速记

- `Client` 长连接鉴权使用首帧 `LoginRequest`，不是 HTTP token。
- 收到持久化消息时，Rust SDK 会先执行 `save_message -> save_cursor`，只有两步都成功后才会在 `ack_messages=true` 时发送 `AckMessage`。
- `PacketPushed` 不参与消息游标、不会补发、也不会自动 ACK。
- 自动重连会重新发送登录帧，并把 `cursor_store.load_seen_messages()` 返回的游标放进 `seen_messages`。
- `session_ref` 是一次登录会话的标识；重连或重登录后可能变化，不能把旧值当成长期稳定 ID。
- `realtime_stream=true` 会改走 `/ws/realtime`，此时服务端只允许瞬时消息和少量查询 RPC，不适合作为通用管理通道。
