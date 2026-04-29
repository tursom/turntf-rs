# turntf-rs 依赖、构建与测试说明

本文档面向维护者，说明 Rust SDK 的依赖分层、Cargo 构建流程、`build.rs` 的职责，以及现有测试覆盖的语义范围。

## 1. 依赖概览

`Cargo.toml` 里的核心依赖可以按职责分成四层：

### 运行时与异步

- `tokio`
- `tokio-stream`
- `futures-util`

用于：

- 后台连接任务
- ping 循环
- `broadcast` 事件流
- WebSocket 读写拆分

### 传输与编解码

- `reqwest`
- `tokio-tungstenite`
- `prost`
- `url`
- `base64`
- `serde`
- `serde_json`

用于：

- REST JSON API
- WebSocket + protobuf frame
- HTTP body / JSON / base64 转换

### 认证与错误

- `bcrypt`
- `thiserror`

用于：

- `plain_password(...)` 的本地 bcrypt 编码
- SDK 统一错误类型

### 构建期依赖

- `prost-build`
- `protoc-bin-vendored`

用于：

- 在编译阶段把 `proto/client.proto` 生成为 Rust 源码
- 避免维护者必须手动安装系统 `protoc`

## 2. 构建流程

常规构建命令：

```bash
cargo build
```

Cargo 会自动执行根目录下的 [`build.rs`](../build.rs)。这个构建脚本当前只做一件事：

1. 定位 `proto/client.proto`
2. 通过 `protoc-bin-vendored` 找到 vendored `protoc`
3. 设置 `PROTOC` 环境变量
4. 使用 `prost-build` 生成 Rust 代码

这意味着：

- 本模块没有额外的 shell 构建脚本
- 也不要求开发机预装 `protoc`
- 修改 `proto/client.proto` 后重新 `cargo build` 即可触发再生成

## 3. 常用命令

### 编译

```bash
cargo build
```

### 运行全部测试

```bash
cargo test
```

### 只跑 HTTP 集成测试

```bash
cargo test --test http
```

### 只跑 Client 集成测试

```bash
cargo test --test client
```

### 验证重连与 `seen_messages`

```bash
cargo test --test client client_reconnects_with_seen_messages_and_realtime_path
```

### 验证 `session_ref` 定向瞬时包

```bash
cargo test --test client client_resolves_sessions_and_targets_transient_delivery
```

### 生成 API 文档

```bash
cargo doc --no-deps
```

## 4. 测试覆盖面

### `tests/http.rs`

这组测试主要验证 `HttpClient` 的 REST 映射是否正确，包括：

- 登录、用户 CRUD、订阅、黑名单、消息与事件查询
- `post_message` / `post_packet` 的 body 编码
- 响应 JSON 到 Rust 类型的映射
- 非 2xx 状态码到 `Error::Server` 的映射
- 本地参数校验，例如非法 `node_id`

如果改了：

- REST 路径
- JSON 字段名
- HTTP 状态码处理
- `HttpClient` 方法签名

优先检查这组测试。

### `tests/client.rs`

这组测试主要验证 `Client` 的状态机和共享语义，包括：

- 登录首帧、密码编码、`LoginInfo.session_ref`
- `MessagePushed` 后的 `save_message -> save_cursor -> ack`
- `PacketPushed` 的事件投递
- `send_message` / `send_packet` / `create_user` / `ping` 的 RPC 路径
- `resolve_user_sessions` 与 `send_packet_to_session`
- 自动重连、重登录与 `seen_messages`
- 持久化失败时不 ACK
- `broadcast` 事件流的 `Lagged` 行为与关闭行为

如果改了：

- 连接状态机
- ACK 顺序
- `CursorStore` 接口
- `session_ref` / 定向瞬时包
- 自动重连策略

优先检查这组测试。

## 5. 维护建议

### 修改 `proto/client.proto` 时

- 重新执行 `cargo build`
- 检查 `src/mapping.rs` 是否仍然完整映射
- 检查 `src/client.rs` / `src/http.rs` 是否需要新增或调整公开 API
- 如果共享语义变了，要同步更新 [guide.md](guide.md)

### 修改 `Client` 状态机时

重点回归下面几类语义：

- 登录成功后的 `session_ref`
- 重连时 `seen_messages` 是否正确带回
- `save_message -> save_cursor -> ack` 顺序
- `ack_messages=false` 时是否保持无 ACK
- `realtime_stream=true` 的行为是否仍与服务端限制一致

### 修改 `HttpClient` 时

重点回归下面几类语义：

- 路径和查询参数拼接
- `delivery_mode` 与 `body` 的编码
- 服务器错误到 `ServerError.code` 的映射
- 本地参数校验是否仍在“发请求前”完成

## 6. 关于“构建脚本”的说明

如果你在别的语言 SDK 里习惯了 `Makefile`、Gradle task 或 npm script，需要注意 Rust SDK 当前没有独立的仓库脚本层：

- 没有 `make build`
- 没有 `scripts/*.sh`
- 没有单独的 proto 生成命令入口

这里的“构建脚本”就是 Cargo 自动执行的 [`build.rs`](../build.rs)。  
因此维护说明里提到“构建脚本”时，默认指的就是它，而不是额外 shell 命令。
