//! Outbox manager

use uuid::Uuid;

#[cfg(any(feature = "postgres", feature = "mysql"))]
use super::retry::RetryStrategy;
use crate::error::Result;
#[cfg(any(feature = "postgres", feature = "mysql"))]
use rs_broker_config::RetryConfig;
#[cfg(any(feature = "postgres", feature = "mysql"))]
use rs_broker_db::outbox::entity::MessageStatus;
#[cfg(any(feature = "postgres", feature = "mysql"))]
use rs_broker_db::{DbPool, OutboxMessage, OutboxRepository};

/// Outbox manager for managing outbox messages
#[cfg(any(feature = "postgres", feature = "mysql"))]
pub struct OutboxManager {
    repository: std::sync::Arc<dyn OutboxRepository>,
    retry_strategy: RetryStrategy,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl Clone for OutboxManager {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            retry_strategy: self.retry_strategy.clone(),
        }
    }
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl std::fmt::Debug for OutboxManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboxManager")
            .field("repository", &"OutboxRepository")
            .field("retry_strategy", &self.retry_strategy)
            .finish()
    }
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl OutboxManager {
    /// Create a new outbox manager
    pub fn new(pool: DbPool, retry_config: RetryConfig) -> Self {
        #[cfg(all(feature = "postgres", not(feature = "mysql")))]
        let repository = {
            use rs_broker_db::outbox::repository::SqlxOutboxRepository;
            std::sync::Arc::new(SqlxOutboxRepository::new(pool))
                as std::sync::Arc<dyn OutboxRepository>
        };

        #[cfg(all(feature = "mysql", not(feature = "postgres")))]
        let repository = {
            use rs_broker_db::outbox::repository::SqlxOutboxRepository;
            std::sync::Arc::new(SqlxOutboxRepository::new(pool))
                as std::sync::Arc<dyn OutboxRepository>
        };

        Self {
            repository,
            retry_strategy: RetryStrategy::new(retry_config),
        }
    }

    /// Create a new outbox manager with a custom repository (for testing)
    pub fn with_repository(
        repository: std::sync::Arc<dyn OutboxRepository>,
        retry_config: RetryConfig,
    ) -> Self {
        Self {
            repository,
            retry_strategy: RetryStrategy::new(retry_config),
        }
    }

    /// Create a new message in the outbox
    pub async fn create_message(
        &self,
        aggregate_type: String,
        aggregate_id: String,
        event_type: String,
        payload: serde_json::Value,
        topic: String,
    ) -> Result<Uuid> {
        let message = OutboxMessage::new(aggregate_type, aggregate_id, event_type, payload, topic);

        self.repository.create(&message).await?;

        Ok(message.id)
    }

    /// Create multiple messages in the outbox in a batch
    pub async fn create_messages_batch(
        &self,
        messages: Vec<(String, String, String, serde_json::Value, String)>,
    ) -> Result<Vec<Uuid>> {
        let outbox_messages: Vec<OutboxMessage> = messages
            .into_iter()
            .map(
                |(aggregate_type, aggregate_id, event_type, payload, topic)| {
                    OutboxMessage::new(aggregate_type, aggregate_id, event_type, payload, topic)
                },
            )
            .collect();

        // Collect IDs before inserting
        let ids: Vec<Uuid> = outbox_messages.iter().map(|msg| msg.id).collect();

        self.repository.create_batch(&outbox_messages).await?;

        Ok(ids)
    }

    /// Get a message by ID
    pub async fn get_message(&self, id: Uuid) -> Result<OutboxMessage> {
        Ok(self.repository.get_by_id(id).await?)
    }

    /// Get pending messages to publish
    pub async fn get_pending_messages(&self, limit: i64) -> Result<Vec<OutboxMessage>> {
        Ok(self.repository.get_pending(limit).await?)
    }

    /// Mark a message as published
    pub async fn mark_published(&self, id: Uuid) -> Result<()> {
        Ok(self.repository.mark_published(id).await?)
    }

    /// Mark a message as failed
    pub async fn mark_failed(&self, id: Uuid, error: String) -> Result<()> {
        Ok(self
            .repository
            .update_status(id, MessageStatus::Failed, Some(error))
            .await?)
    }

    /// Get the retry strategy
    pub fn retry_strategy(&self) -> &RetryStrategy {
        &self.retry_strategy
    }
}
