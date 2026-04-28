use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("{message}")]
pub struct ValidationError {
    pub message: String,
}

impl ValidationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf client is closed")]
pub struct ClosedError;

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf client is not connected")]
pub struct NotConnectedError;

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf websocket disconnected")]
pub struct DisconnectedError;

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub struct ServerError {
    pub code: String,
    pub server_message: String,
    pub request_id: u64,
}

impl ServerError {
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

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf protocol error: {protocol_message}")]
pub struct ProtocolError {
    pub protocol_message: String,
}

impl ProtocolError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            protocol_message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf connection error during {op}: {cause}")]
pub struct ConnectionError {
    pub op: String,
    pub cause: String,
}

impl ConnectionError {
    pub fn new(op: impl Into<String>, cause: impl Into<String>) -> Self {
        Self {
            op: op.into(),
            cause: cause.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
#[error("turntf store error during {op}: {message}")]
pub struct StoreError {
    pub op: String,
    pub message: String,
}

impl StoreError {
    pub fn new(op: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            op: op.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, ThisError)]
pub enum Error {
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error(transparent)]
    Closed(#[from] ClosedError),
    #[error(transparent)]
    NotConnected(#[from] NotConnectedError),
    #[error(transparent)]
    Disconnected(#[from] DisconnectedError),
    #[error(transparent)]
    Server(#[from] ServerError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl Error {
    pub fn validation(message: impl Into<String>) -> Self {
        ValidationError::new(message).into()
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        ProtocolError::new(message).into()
    }

    pub fn connection(op: impl Into<String>, cause: impl Into<String>) -> Self {
        ConnectionError::new(op, cause).into()
    }

    pub fn store(op: impl Into<String>, message: impl Into<String>) -> Self {
        StoreError::new(op, message).into()
    }
}
