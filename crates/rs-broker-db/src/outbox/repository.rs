//! Outbox repository

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
use crate::pool::DbPool;
#[cfg(all(feature = "mysql", not(feature = "postgres")))]
use crate::pool::DbPool;
use async_trait::async_trait;
use uuid::Uuid;

use super::entity::{MessageStatus, OutboxMessage};

/// Error type for outbox repository operations
#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Message not found: {0}")]
    NotFound(Uuid),
}

/// Outbox repository trait
#[async_trait]
pub trait OutboxRepository: Send + Sync {
    /// Create a new outbox message
    async fn create(&self, message: &OutboxMessage) -> Result<(), OutboxError>;

    /// Create multiple outbox messages in a batch
    async fn create_batch(&self, messages: &[OutboxMessage]) -> Result<(), OutboxError>;

    /// Get a message by ID
    async fn get_by_id(&self, id: Uuid) -> Result<OutboxMessage, OutboxError>;

    /// Get pending messages to publish
    async fn get_pending(&self, limit: i64) -> Result<Vec<OutboxMessage>, OutboxError>;

    /// Update message status
    async fn update_status(
        &self,
        id: Uuid,
        status: MessageStatus,
        error_message: Option<String>,
    ) -> Result<(), OutboxError>;

    /// Mark message as published
    async fn mark_published(&self, id: Uuid) -> Result<(), OutboxError>;

    /// Delete a message
    async fn delete(&self, id: Uuid) -> Result<(), OutboxError>;
}

/// SQLx-based outbox repository
pub struct SqlxOutboxRepository {
    pool: DbPool,
}

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
impl SqlxOutboxRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
impl SqlxOutboxRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OutboxRepository for SqlxOutboxRepository {
    async fn create(&self, message: &OutboxMessage) -> Result<(), OutboxError> {
        sqlx::query(
            r#"
            INSERT INTO outbox_messages (
                id, aggregate_type, aggregate_id, event_type,
                payload, headers, topic, partition_key,
                status, retry_count, error_message,
                created_at, updated_at, published_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#,
        )
        .bind(message.id)
        .bind(&message.aggregate_type)
        .bind(&message.aggregate_id)
        .bind(&message.event_type)
        .bind(&message.payload)
        .bind(&message.headers)
        .bind(&message.topic)
        .bind(&message.partition_key)
        .bind(format!("{:?}", message.status).to_lowercase())
        .bind(message.retry_count)
        .bind(&message.error_message)
        .bind(message.created_at)
        .bind(message.updated_at)
        .bind(message.published_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn create_batch(&self, messages: &[OutboxMessage]) -> Result<(), OutboxError> {
        if messages.is_empty() {
            return Ok(());
        }

        // Use a transaction to ensure atomicity
        let mut tx = self.pool.begin().await?;

        for message in messages {
            sqlx::query(
                r#"
                INSERT INTO outbox_messages (
                    id, aggregate_type, aggregate_id, event_type,
                    payload, headers, topic, partition_key,
                    status, retry_count, error_message,
                    created_at, updated_at, published_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                "#,
            )
            .bind(message.id)
            .bind(&message.aggregate_type)
            .bind(&message.aggregate_id)
            .bind(&message.event_type)
            .bind(&message.payload)
            .bind(&message.headers)
            .bind(&message.topic)
            .bind(&message.partition_key)
            .bind(format!("{:?}", message.status).to_lowercase())
            .bind(message.retry_count)
            .bind(&message.error_message)
            .bind(message.created_at)
            .bind(message.updated_at)
            .bind(message.published_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<OutboxMessage, OutboxError> {
        let row =
            sqlx::query_as::<_, OutboxMessageRow>("SELECT * FROM outbox_messages WHERE id = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;

        Ok(row.into())
    }

    async fn get_pending(&self, limit: i64) -> Result<Vec<OutboxMessage>, OutboxError> {
        let rows = sqlx::query_as::<_, OutboxMessageRow>(
            "SELECT * FROM outbox_messages WHERE status = 'pending' ORDER BY created_at ASC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: MessageStatus,
        error_message: Option<String>,
    ) -> Result<(), OutboxError> {
        sqlx::query(
            "UPDATE outbox_messages SET status = ?, error_message = ?, updated_at = NOW() WHERE id = ?"
        )
        .bind(format!("{:?}", status).to_lowercase())
        .bind(&error_message)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn mark_published(&self, id: Uuid) -> Result<(), OutboxError> {
        sqlx::query(
            "UPDATE outbox_messages SET status = 'published', published_at = NOW(), updated_at = NOW() WHERE id = ?"
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), OutboxError> {
        sqlx::query("DELETE FROM outbox_messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

// Internal row type for SQLx
#[derive(sqlx::FromRow)]
struct OutboxMessageRow {
    id: Uuid,
    aggregate_type: String,
    aggregate_id: String,
    event_type: String,
    payload: serde_json::Value,
    headers: Option<serde_json::Value>,
    topic: String,
    partition_key: Option<String>,
    status: String,
    retry_count: i32,
    error_message: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    published_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<OutboxMessageRow> for OutboxMessage {
    fn from(row: OutboxMessageRow) -> Self {
        Self {
            id: row.id,
            aggregate_type: row.aggregate_type,
            aggregate_id: row.aggregate_id,
            event_type: row.event_type,
            payload: row.payload,
            headers: row.headers,
            topic: row.topic,
            partition_key: row.partition_key,
            status: match row.status.as_str() {
                "pending" => MessageStatus::Pending,
                "publishing" => MessageStatus::Publishing,
                "published" => MessageStatus::Published,
                "retrying" => MessageStatus::Retrying,
                "failed" => MessageStatus::Failed,
                "dlq" => MessageStatus::Dlq,
                _ => MessageStatus::Pending,
            },
            retry_count: row.retry_count,
            error_message: row.error_message,
            created_at: row.created_at,
            updated_at: row.updated_at,
            published_at: row.published_at,
        }
    }
}
