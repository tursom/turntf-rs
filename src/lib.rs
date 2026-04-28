mod client;
mod errors;
mod http;
mod mapping;
mod password;
mod store;
mod types;
mod validation;

pub(crate) mod proto {
    include!(concat!(env!("OUT_DIR"), "/notifier.client.v1.rs"));
}

pub use client::{Client, ClientEvent, ClientSubscription, Config};
pub use errors::{
    ClosedError, ConnectionError, DisconnectedError, Error, NotConnectedError, ProtocolError,
    Result, ServerError, StoreError, ValidationError,
};
pub use http::HttpClient;
pub use password::{hash_password, hashed_password, plain_password, PasswordInput, PasswordSource};
pub use store::{BoxError, CursorStore, MemoryCursorStore};
pub use types::{
    Attachment, AttachmentType, BlacklistEntry, ClusterNode, CreateUserRequest, Credentials,
    DeleteUserResult, DeliveryMode, Event, EventLogTrimStatus, LoggedInUser, LoginInfo, Message,
    MessageCursor, MessageTrimStatus, OnlineNodePresence, OperationsStatus, Packet,
    PeerOriginStatus, PeerStatus, ProjectionStatus, RelayAccepted, ResolvedSession,
    ResolvedUserSessions, SessionRef, Subscription, UpdateUserRequest, User, UserRef,
};
