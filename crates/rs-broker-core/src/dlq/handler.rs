//! DLQ handler

use uuid::Uuid;

use crate::error::Result;
use rs_broker_db::dlq::repository::SqlxDlqRepository;
use rs_broker_db::{DbPool, DlqMessage, DlqRepository};

/// DLQ handler for managing dead letter messages
pub struct DlqHandler {
    repository: Box<dyn DlqRepository>,
}

impl DlqHandler {
    /// Create a new DLQ handler
    pub fn new(pool: DbPool) -> Self {
        let repository = SqlxDlqRepository::new(pool);
        Self {
            repository: Box::new(repository),
        }
    }

    /// Create a new DLQ handler with a custom repository (for testing)
    pub fn with_repository(repository: Box<dyn DlqRepository>) -> Self {
        Self { repository }
    }

    /// Move a message to the DLQ
    pub async fn move_to_dlq(
        &self,
        original_message_id: Uuid,
        original_topic: String,
        dlq_topic: String,
        failure_reason: String,
        retry_count: i32,
        payload: serde_json::Value,
    ) -> Result<Uuid> {
        let message = DlqMessage::new(
            original_message_id,
            original_topic,
            dlq_topic,
            failure_reason,
            retry_count,
            payload,
        );

        self.repository.create(&message).await?;

        Ok(message.id)
    }

    /// Get DLQ messages
    pub async fn get_messages(
        &self,
        topic: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<DlqMessage>> {
        Ok(self.repository.get_all(topic, limit, offset).await?)
    }

    /// Count DLQ messages
    pub async fn count(&self, topic: Option<&str>) -> Result<i64> {
        Ok(self.repository.count(topic).await?)
    }

    /// Delete a DLQ message
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        Ok(self.repository.delete(id).await?)
    }
}
