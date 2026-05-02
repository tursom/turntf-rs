//! turntf Rust SDK
//!
//! turntf 是一个实时消息推送平台的 Rust 客户端 SDK，提供基于 WebSocket 的实时通信
//! 和基于 HTTP REST 的 API 操作能力。
//!
//! # 功能特性
//!
//! - **WebSocket 实时通信** - 通过长连接接收消息推送和事件通知
//! - **HTTP REST API** - 用户管理、元数据管理、消息收发等操作
//! - **自动重连** - 支持指数退避的自动重连机制
//! - **消息去重** - 通过游标存储机制确保消息不重复接收
//! - **频道订阅** - 支持频道订阅/取消订阅
//! - **用户黑名单** - 支持用户屏蔽管理
//! - **集群查询** - 支持集群节点和在线用户查询
//!
//! # 快速开始
//!
//! ```ignore
//! use turntf::{Client, Config};
//! use turntf::password::plain_password;
//!
//! let config = Config::new(
//!     "http://localhost:8080",
//!     turntf::types::Credentials {
//!         node_id: 1,
//!         user_id: 42,
//!         password: plain_password("my_password")?,
//!     },
//! );
//!
//! let client = Client::new(config)?;
//! client.connect().await?;
//!
//! // 订阅消息事件
//! let mut subscription = client.subscribe().await;
//! while let Some(event) = subscription.next().await {
//!     // 处理事件...
//! }
//! ```
//!
//! # 模块结构
//!
//! - [`client`] - WebSocket 客户端，提供实时通信能力
//! - [`errors`] - 错误类型定义
//! - [`http`] - HTTP REST API 客户端
//! - [`types`] - 数据类型定义
//! - [`store`] - 游标存储抽象
//! - [`password`] - 密码处理工具
//! - [`validation`] - 参数验证函数
//! - [`mapping`] - 数据模型转换（内部）

mod client;
mod errors;
mod http;
mod mapping;
mod password;
mod store;
mod types;
mod validation;

/// Protobuf 生成的代码模块，包含客户端与服务器通信的协议定义。
pub(crate) mod proto {
    include!(concat!(env!("OUT_DIR"), "/notifier.client.v1.rs"));
}

// === 公开 API 重导出 ===

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
    ResolvedUserSessions, ScanUserMetadataRequest, SessionRef, Subscription, UpdateUserRequest,
    UpsertUserMetadataRequest, User, UserMetadata, UserMetadataScanResult, UserRef,
};
