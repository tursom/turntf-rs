# turntf-rs

`turntf-rs` 是 turntf 的 Rust SDK，当前提供：

- HTTP JSON 管理与查询客户端
- WebSocket + Protobuf 长连接客户端
- 自动重连、重登录与 `seen_messages` 回放去重
- `save_message -> save_cursor -> ack` 的可靠消息处理流程
- Rust 风格的广播事件流订阅接口

首版不实现 ZeroMQ，也不公开底层 protobuf 类型。

## 安装

```toml
[dependencies]
turntf = { path = "../turntf-rs" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tokio-stream = "0.1"
```

## HTTP 示例

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

## 实时客户端示例

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

    if let Some(Ok(ClientEvent::Login(info))) = events.next().await {
        println!("login ok: {}", info.protocol_version);
    }

    client
        .send_message(
            turntf::UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            b"hello".to_vec(),
        )
        .await?;

    client.close().await?;
    Ok(())
}
```
