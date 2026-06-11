//! DLQ handler

use std::sync::Arc;

use uuid::Uuid;

use crate::error::Result;
use rs_broker_db::dlq::repository::SqlxDlqRepository;
use rs_broker_db::outbox::entity::MessageStatus;
use rs_broker_db::outbox::OutboxMessage;
use rs_broker_db::{DbPool, DlqMessage, DlqRepository, OutboxRepository};

/// DLQ handler for managing dead letter messages
pub struct DlqHandler {
    repository: Arc<dyn DlqRepository>,
}

impl DlqHandler {
    /// Create a new DLQ handler
    pub fn new(pool: DbPool) -> Self {
        let repository = SqlxDlqRepository::new(pool);
        Self {
            repository: Arc::new(repository) as Arc<dyn DlqRepository>,
        }
    }

    /// Create a new DLQ handler with a custom repository (for testing)
    pub fn with_repository(repository: Arc<dyn DlqRepository>) -> Self {
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

    /// Reprocess DLQ messages by re-enqueueing them as pending outbox messages.
    ///
    /// `selector` controls which messages are reprocessed:
    /// - `DlqSelector::Id(uuid)` — one specific message
    /// - `DlqSelector::Topic(topic)` — all messages originally from `topic`
    /// - `DlqSelector::All` — every DLQ message
    ///
    /// For each DLQ message a brand-new `OutboxMessage` row is inserted with
    /// status `Pending`. The original DLQ row is deleted after the outbox
    /// insert succeeds, keeping the operation idempotent per DLQ entry.
    pub async fn reprocess(
        &self,
        selector: DlqSelector<'_>,
        outbox_repo: &dyn OutboxRepository,
    ) -> Result<ReprocessResult> {
        let messages = match selector {
            DlqSelector::Id(id) => {
                let msg = self.repository.get_by_id(id).await?;
                vec![msg]
            }
            DlqSelector::Topic(topic) => self.repository.get_all(Some(topic), 10_000, 0).await?,
            DlqSelector::All => self.repository.get_all(None, 10_000, 0).await?,
        };

        let mut result = ReprocessResult::default();

        for dlq_msg in &messages {
            let outbox_msg = OutboxMessage {
                id: Uuid::new_v4(),
                aggregate_type: "reprocess".to_string(),
                aggregate_id: dlq_msg.original_message_id.to_string(),
                event_type: "reprocessed".to_string(),
                payload: dlq_msg.payload.clone(),
                headers: dlq_msg.headers.clone(),
                topic: dlq_msg.original_topic.clone(),
                partition_key: None,
                status: MessageStatus::Pending,
                retry_count: 0,
                error_message: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                published_at: None,
            };

            match outbox_repo.create(&outbox_msg).await {
                Ok(()) => {
                    // Best-effort delete of the DLQ row. If the delete fails we
                    // still count the message as reprocessed because the outbox
                    // row exists and will be published.
                    if let Err(e) = self.repository.delete(dlq_msg.id).await {
                        tracing::warn!(
                            "Failed to delete DLQ message {} after reprocess: {}",
                            dlq_msg.id,
                            e
                        );
                    }
                    result.reprocessed_count += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to create outbox message from DLQ entry {}: {}",
                        dlq_msg.id,
                        e
                    );
                    result.failure_count += 1;
                    result.errors.push(e.to_string());
                }
            }
        }

        Ok(result)
    }
}

/// Selects which DLQ messages to reprocess
pub enum DlqSelector<'a> {
    /// Reprocess a single message by ID
    Id(Uuid),
    /// Reprocess all messages originally from the given topic
    Topic(&'a str),
    /// Reprocess every DLQ message
    All,
}

/// Result of a DLQ reprocess operation
#[derive(Debug, Default)]
pub struct ReprocessResult {
    /// Number of messages successfully re-enqueued
    pub reprocessed_count: i32,
    /// Number of messages that could not be re-enqueued
    pub failure_count: i32,
    /// Error details for failed messages
    pub errors: Vec<String>,
}
