use thiserror::Error as ThisError;

/// SDK 操作的结果类型。
///
/// 这是 `std::result::Result<T, Error>` 的类型别名，所有可能失败的 SDK 操作都返回此类型。
///
/// # 泛型参数
/// - `T` - 成功时的返回值类型
pub type Result<T> = std::result::Result<T, Error>;

/// 参数验证错误。
///
/// 当提供给 API 的参数不符合要求时返回此错误。例如：空字符串、超出范围的值、无效格式等。
///
/// # 示例
///
/// ```ignore
/// if value <= 0 {
///     return Err(Error::validation("value must be positive"));
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("{message}")]
pub struct ValidationError {
    /// 描述验证失败原因的错误消息
    pub message: String,
}

impl ValidationError {
    /// 创建一个新的验证错误。
    ///
    /// # 参数
    /// - `message` - 描述验证失败原因的消息
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// 客户端已关闭错误。
///
/// 当在客户端已调用 `close()` 方法后尝试进行操作时返回此错误。
/// 此错误表示客户端实例已被永久关闭，需要创建新的客户端实例才能继续操作。
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf client is closed")]
pub struct ClosedError;

/// 客户端未连接错误。
///
/// 当客户端尚未建立 WebSocket 连接时尝试发送请求会返回此错误。
/// 可以通过调用 `connect()` 方法建立连接后重试。
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf client is not connected")]
pub struct NotConnectedError;

/// WebSocket 连接已断开错误。
///
/// 当客户端与服务器的 WebSocket 连接意外中断时返回此错误。
/// 如果配置了自动重连（`Config.reconnect = true`），客户端会自动尝试重新连接。
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf websocket disconnected")]
pub struct DisconnectedError;

/// 服务器返回的错误。
///
/// 当服务器处理请求时返回错误状态码或错误响应时使用此类型。
/// 包含服务器定义的错误码、描述消息和关联的请求 ID。
///
/// # 错误码示例
/// - `"unauthorized"` - 认证失败
/// - `"not_found"` - 资源不存在
/// - `"bad_request"` - 请求参数有误
/// - `"internal_error"` - 服务器内部错误
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct ServerError {
    /// 服务器定义的错误码，如 `"unauthorized"`、`"not_found"` 等
    pub code: String,
    /// 服务器返回的错误描述消息
    pub server_message: String,
    /// 关联的请求 ID，用于在服务器端追踪问题
    pub request_id: u64,
}

impl ServerError {
    /// 创建一个新的服务器错误。
    ///
    /// # 参数
    /// - `code` - 错误码
    /// - `server_message` - 错误描述
    /// - `request_id` - 关联的请求 ID
    pub fn new(
        code: impl Into<String>,
        server_message: impl Into<String>,
        request_id: u64,
    ) -> Self {
        Self {
            code: code.into(),
            server_message: server_message.into(),
            request_id,
        }
    }

    /// 检查此错误是否为未授权错误。
    ///
    /// 当错误码为 `"unauthorized"` 时返回 `true`。
    /// 这通常表示认证令牌已过期或凭据无效，客户端需要重新登录。
    pub fn unauthorized(&self) -> bool {
        self.code == "unauthorized"
    }
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.request_id == 0 {
            write!(
                f,
                "turntf server error: {} ({})",
                self.code, self.server_message
            )
        } else {
            write!(
                f,
                "turntf server error: {} ({}), request_id={}",
                self.code, self.server_message, self.request_id
            )
        }
    }
}

/// 协议错误。
///
/// 当客户端与服务器之间的通信不符合约定的协议格式时返回此错误。
/// 例如：无效的 Protobuf 帧、缺失必需的字段、无法识别的消息类型等。
///
/// 此错误通常表示 SDK 与服务器版本不兼容或数据传输损坏。
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf protocol error: {protocol_message}")]
pub struct ProtocolError {
    /// 描述协议错误的详细信息
    pub protocol_message: String,
}

impl ProtocolError {
    /// 创建一个新的协议错误。
    ///
    /// # 参数
    /// - `message` - 描述协议错误的消息
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            protocol_message: message.into(),
        }
    }
}

/// 连接错误。
///
/// 当客户端与服务器建立或维持网络连接时遇到问题返回此错误。
/// 例如：DNS 解析失败、TCP 连接被拒绝、WebSocket 握手失败、读写超时等。
///
/// 此错误通常由网络问题引起，可以重试解决。
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf connection error during {op}: {cause}")]
pub struct ConnectionError {
    /// 发生错误时的操作名称，如 `"dial"`、`"write"`、`"read"` 等
    pub op: String,
    /// 导致连接错误的具体原因
    pub cause: String,
}

impl ConnectionError {
    /// 创建一个新的连接错误。
    ///
    /// # 参数
    /// - `op` - 发生错误时的操作名称
    /// - `cause` - 连接错误的原因描述
    pub fn new(op: impl Into<String>, cause: impl Into<String>) -> Self {
        Self {
            op: op.into(),
            cause: cause.into(),
        }
    }
}

/// 存储错误。
///
/// 当客户端的本地游标存储（CursorStore）操作失败时返回此错误。
/// 例如：存储已读消息记录或游标信息时发生 I/O 错误。
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf store error during {op}: {message}")]
pub struct StoreError {
    /// 发生存储错误时的操作名称，如 `"save_message"`、`"save_cursor"` 等
    pub op: String,
    /// 存储错误的详细信息
    pub message: String,
}

impl StoreError {
    /// 创建一个新的存储错误。
    ///
    /// # 参数
    /// - `op` - 发生错误时的操作名称
    /// - `message` - 错误描述
    pub fn new(op: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            op: op.into(),
            message: message.into(),
        }
    }
}

/// SDK 统一的错误枚举。
///
/// 涵盖了 SDK 所有可能出现的错误类型，包括参数验证、连接、协议、服务器返回等各类错误。
/// 通过 `thiserror` 派生，支持 `Display` 和 `Error` trait，且可以通过 `From` 从各子错误类型自动转换。
///
/// # 错误处理示例
///
/// ```ignore
/// match client.connect().await {
///     Ok(()) => println!("连接成功"),
///     Err(Error::Closed(_)) => println!("客户端已关闭"),
///     Err(Error::Connection(err)) => println!("连接失败: {} ({})", err.op, err.cause),
///     Err(Error::Server(err)) if err.unauthorized() => println!("认证失败，需要重新登录"),
///     Err(e) => println!("其他错误: {e}"),
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub enum Error {
    /// 参数验证错误，表示参数不符合要求
    #[error(transparent)]
    Validation(#[from] ValidationError),
    /// 客户端已关闭错误
    #[error(transparent)]
    Closed(#[from] ClosedError),
    /// 客户端未连接错误
    #[error(transparent)]
    NotConnected(#[from] NotConnectedError),
    /// WebSocket 连接已断开错误
    #[error(transparent)]
    Disconnected(#[from] DisconnectedError),
    /// 服务器返回错误
    #[error(transparent)]
    Server(#[from] ServerError),
    /// 协议错误
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// 网络连接错误
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    /// 本地存储错误
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl Error {
    /// 创建一个参数验证错误。
    ///
    /// # 参数
    /// - `message` - 验证失败的原因描述
    ///
    /// # 示例
    ///
    /// ```ignore
    /// return Err(Error::validation("username is required"));
    /// ```
    pub fn validation(message: impl Into<String>) -> Self {
        ValidationError::new(message).into()
    }

    /// 创建一个协议错误。
    ///
    /// # 参数
    /// - `message` - 协议错误描述
    pub fn protocol(message: impl Into<String>) -> Self {
        ProtocolError::new(message).into()
    }

    /// 创建一个连接错误。
    ///
    /// # 参数
    /// - `op` - 发生错误时的操作名称（如 `"dial"`、`"write"`）
    /// - `cause` - 连接错误的原因
    pub fn connection(op: impl Into<String>, cause: impl Into<String>) -> Self {
        ConnectionError::new(op, cause).into()
    }

    /// 创建一个存储错误。
    ///
    /// # 参数
    /// - `op` - 发生错误时的操作名称（如 `"save_message"`）
    /// - `message` - 错误描述
    pub fn store(op: impl Into<String>, message: impl Into<String>) -> Self {
        StoreError::new(op, message).into()
    }
}
