//! Consuming ports.

use async_trait::async_trait;
use uuid::Uuid;

use super::domain::InboxMessage;

/// Error type for inbox repository operations.
#[derive(Debug, thiserror::Error)]
pub enum InboxError {
    /// Storage failure; the storage driver's message is carried verbatim so
    /// operator-facing error text is unchanged across adapter swaps.
    #[error("Database error: {0}")]
    Database(String),

    #[error("Message not found: {0}")]
    NotFound(Uuid),
}

/// Port: durable storage for inbox messages.
#[async_trait]
pub trait InboxRepository: Send + Sync {
    /// Create a new inbox message
    async fn create(&self, message: &InboxMessage) -> Result<(), InboxError>;

    /// Get a message by ID
    async fn get_by_id(&self, id: Uuid) -> Result<InboxMessage, InboxError>;

    /// Get message by topic and offset for deduplication
    async fn get_by_topic_offset(
        &self,
        topic: &str,
        offset: i64,
    ) -> Result<Option<InboxMessage>, InboxError>;

    /// Update message status
    async fn update_status(
        &self,
        id: Uuid,
        status: super::domain::InboxStatus,
        error_message: Option<String>,
    ) -> Result<(), InboxError>;

    /// Mark message as processed
    async fn mark_processed(&self, id: Uuid) -> Result<(), InboxError>;

    /// Delete a message
    async fn delete(&self, id: Uuid) -> Result<(), InboxError>;
}
