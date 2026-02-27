//! Inbox manager

use uuid::Uuid;

use crate::error::Result;
use rs_broker_db::inbox::repository::SqlxInboxRepository;
use rs_broker_db::{DbPool, InboxMessage, InboxRepository};

/// Inbox manager for managing received messages
pub struct InboxManager {
    repository: Box<dyn InboxRepository>,
}

impl InboxManager {
    /// Create a new inbox manager
    pub fn new(pool: DbPool) -> Self {
        let repository = SqlxInboxRepository::new(pool);
        Self {
            repository: Box::new(repository),
        }
    }

    /// Store a message in the inbox
    pub async fn store_message(
        &self,
        topic: String,
        partition: i32,
        offset: i64,
        payload: serde_json::Value,
    ) -> Result<Uuid> {
        let message = InboxMessage::new(topic, partition, offset, payload);

        self.repository.create(&message).await?;

        Ok(message.id)
    }

    /// Get a message by ID
    pub async fn get_message(&self, id: Uuid) -> Result<InboxMessage> {
        Ok(self.repository.get_by_id(id).await?)
    }

    /// Mark message as processed
    pub async fn mark_processed(&self, id: Uuid) -> Result<()> {
        Ok(self.repository.mark_processed(id).await?)
    }

    /// Check for duplicate by topic and offset
    pub async fn check_duplicate(&self, topic: &str, offset: i64) -> Result<Option<InboxMessage>> {
        Ok(self.repository.get_by_topic_offset(topic, offset).await?)
    }
}
