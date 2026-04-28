use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::types::{Message, MessageCursor, UserRef};

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[async_trait]
pub trait CursorStore: Send + Sync {
    async fn load_seen_messages(&self) -> std::result::Result<Vec<MessageCursor>, BoxError>;
    async fn save_message(&self, message: Message) -> std::result::Result<(), BoxError>;
    async fn save_cursor(&self, cursor: MessageCursor) -> std::result::Result<(), BoxError>;
}

#[derive(Default)]
struct MemoryCursorStoreState {
    messages: HashMap<MessageCursor, Message>,
    order: Vec<MessageCursor>,
}

#[derive(Clone, Default)]
pub struct MemoryCursorStore {
    inner: Arc<Mutex<MemoryCursorStoreState>>,
}

impl MemoryCursorStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn has_cursor(&self, cursor: &MessageCursor) -> bool {
        self.inner.lock().await.messages.contains_key(cursor)
    }

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
        state.messages.entry(cursor.clone()).or_insert_with(|| Message {
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
