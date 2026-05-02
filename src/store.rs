use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::types::{Message, MessageCursor, UserRef};

/// 通用的错误类型别名，用于 `CursorStore` trait 的错误返回。
///
/// 实现了 `Send + Sync + 'static` 约束，允许存储实现返回任意类型的错误。
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// 游标存储 trait，用于管理已读消息的持久化。
///
/// `CursorStore` 负责保存和管理客户端已接收消息的游标信息。当客户端重新连接时，
/// SDK 使用此存储中的信息向服务器同步已确认的消息，避免重复接收。
///
/// 用户可以自定义实现此 trait 来使用不同的存储后端（如文件、数据库等）。
///
/// # 实现要求
/// - 所有方法必须实现 `Send + Sync` 约束
/// - 实现应该是线程安全的
///
/// # 内置实现
/// - `MemoryCursorStore` - 基于内存的默认实现
///
/// # 示例
///
/// ```ignore
/// use async_trait::async_trait;
/// use turntf::store::{BoxError, CursorStore};
/// use turntf::types::{Message, MessageCursor};
///
/// struct MyStore;
///
/// #[async_trait]
/// impl CursorStore for MyStore {
///     async fn load_seen_messages(&self) -> Result<Vec<MessageCursor>, BoxError> {
///         Ok(Vec::new())
///     }
///     async fn save_message(&self, message: Message) -> Result<(), BoxError> {
///         Ok(())
///     }
///     async fn save_cursor(&self, cursor: MessageCursor) -> Result<(), BoxError> {
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait CursorStore: Send + Sync {
    /// 加载所有已确认的消息游标列表。
    ///
    /// 在客户端建立 WebSocket 连接时会调用此方法，
    /// 将已确认的消息游标发送给服务器，用于消息去重。
    async fn load_seen_messages(&self) -> std::result::Result<Vec<MessageCursor>, BoxError>;

    /// 保存一条消息到存储中。
    ///
    /// 当客户端收到服务器推送的消息时会调用此方法。
    ///
    /// # 参数
    /// - `message` - 要保存的消息
    async fn save_message(&self, message: Message) -> std::result::Result<(), BoxError>;

    /// 保存一个消息游标到存储中。
    ///
    /// 用于记录已处理的消息位置。即使消息体本身未被保存，游标也应当被持久化，
    /// 以确保重连时不会重复接收该消息。
    ///
    /// # 参数
    /// - `cursor` - 要保存的消息游标
    async fn save_cursor(&self, cursor: MessageCursor) -> std::result::Result<(), BoxError>;
}

#[derive(Default)]
struct MemoryCursorStoreState {
    messages: HashMap<MessageCursor, Message>,
    order: Vec<MessageCursor>,
}

/// 基于内存的 `CursorStore` 实现。
///
/// 将所有消息和游标存储在内存中，使用 `HashMap` 和 `Vec` 进行管理。
/// 这是 SDK 默认使用的游标存储实现。
///
/// 注意：由于数据存储在内存中，程序重启后数据会丢失。
/// 对于生产环境，建议实现持久化的 `CursorStore`。
///
/// # 线程安全
///
/// 内部使用 `Arc<Mutex<...>>` 保证线程安全，`Clone` 创建的所有实例共享同一份数据。
///
/// # 示例
///
/// ```ignore
/// use turntf::store::MemoryCursorStore;
///
/// let store = MemoryCursorStore::new();
/// ```
#[derive(Clone, Default)]
pub struct MemoryCursorStore {
    inner: Arc<Mutex<MemoryCursorStoreState>>,
}

impl MemoryCursorStore {
    /// 创建一个新的空 `MemoryCursorStore`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查指定游标是否已存在。
    ///
    /// # 参数
    /// - `cursor` - 要检查的消息游标
    ///
    /// # 返回值
    /// 如果游标已存在，返回 `true`。
    pub async fn has_cursor(&self, cursor: &MessageCursor) -> bool {
        self.inner.lock().await.messages.contains_key(cursor)
    }

    /// 获取指定游标对应的消息。
    ///
    /// # 参数
    /// - `cursor` - 要查询的消息游标
    ///
    /// # 返回值
    /// 如果找到对应的消息，返回 `Some(Message)`；否则返回 `None`。
    pub async fn message(&self, cursor: &MessageCursor) -> Option<Message> {
        self.inner.lock().await.messages.get(cursor).cloned()
    }
}

#[async_trait]
impl CursorStore for MemoryCursorStore {
    async fn load_seen_messages(&self) -> std::result::Result<Vec<MessageCursor>, BoxError> {
        Ok(self.inner.lock().await.order.clone())
    }

    async fn save_message(&self, message: Message) -> std::result::Result<(), BoxError> {
        self.inner
            .lock()
            .await
            .messages
            .insert(message.cursor(), message);
        Ok(())
    }

    async fn save_cursor(&self, cursor: MessageCursor) -> std::result::Result<(), BoxError> {
        let mut state = self.inner.lock().await;
        state
            .messages
            .entry(cursor.clone())
            .or_insert_with(|| Message {
                recipient: UserRef::default(),
                node_id: cursor.node_id,
                seq: cursor.seq,
                sender: UserRef::default(),
                body: Vec::new(),
                created_at_hlc: String::new(),
            });
        if !state.order.contains(&cursor) {
            state.order.push(cursor);
        }
        Ok(())
    }
}
