//! Dead-letter ports.

use async_trait::async_trait;
use uuid::Uuid;

use super::domain::DlqMessage;

/// Error type for DLQ repository operations.
#[derive(Debug, thiserror::Error)]
pub enum DlqError {
    /// Storage failure; the storage driver's message is carried verbatim so
    /// operator-facing error text is unchanged across adapter swaps.
    #[error("Database error: {0}")]
    Database(String),

    #[error("Message not found: {0}")]
    NotFound(Uuid),
}

/// Port: durable storage for dead-lettered messages.
#[async_trait]
pub trait DlqRepository: Send + Sync {
    /// Create a new DLQ message
    async fn create(&self, message: &DlqMessage) -> Result<(), DlqError>;

    /// Get a DLQ message by ID
    async fn get_by_id(&self, id: Uuid) -> Result<DlqMessage, DlqError>;

    /// Get all DLQ messages with optional filters
    async fn get_all(
        &self,
        topic: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<DlqMessage>, DlqError>;

    /// Count DLQ messages
    async fn count(&self, topic: Option<&str>) -> Result<i64, DlqError>;

    /// Delete a DLQ message
    async fn delete(&self, id: Uuid) -> Result<(), DlqError>;

    /// Delete all DLQ messages
    async fn delete_all(&self) -> Result<(), DlqError>;
}
