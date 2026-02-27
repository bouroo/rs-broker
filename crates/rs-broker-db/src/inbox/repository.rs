//! Inbox repository

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
use crate::pool::DbPool;
#[cfg(all(feature = "mysql", not(feature = "postgres")))]
use crate::pool::DbPool;
use async_trait::async_trait;
use uuid::Uuid;

use super::entity::{InboxMessage, InboxStatus};

/// Error type for inbox repository operations
#[derive(Debug, thiserror::Error)]
pub enum InboxError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Message not found: {0}")]
    NotFound(Uuid),
}

/// Inbox repository trait
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
        status: InboxStatus,
        error_message: Option<String>,
    ) -> Result<(), InboxError>;

    /// Mark message as processed
    async fn mark_processed(&self, id: Uuid) -> Result<(), InboxError>;

    /// Delete a message
    async fn delete(&self, id: Uuid) -> Result<(), InboxError>;
}

/// SQLx-based inbox repository
pub struct SqlxInboxRepository {
    pool: DbPool,
}

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
impl SqlxInboxRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
impl SqlxInboxRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InboxRepository for SqlxInboxRepository {
    async fn create(&self, message: &InboxMessage) -> Result<(), InboxError> {
        sqlx::query(
            r#"
            INSERT INTO inbox_messages (
                id, topic, partition, offset, key, event_type,
                payload, headers, timestamp, status,
                attempt_count, error_message, received_at, processed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(message.id)
        .bind(&message.topic)
        .bind(message.partition)
        .bind(message.offset)
        .bind(&message.key)
        .bind(&message.event_type)
        .bind(&message.payload)
        .bind(&message.headers)
        .bind(message.timestamp)
        .bind(format!("{:?}", message.status).to_lowercase())
        .bind(message.attempt_count)
        .bind(&message.error_message)
        .bind(message.received_at)
        .bind(message.processed_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<InboxMessage, InboxError> {
        let row = sqlx::query_as::<_, InboxMessageRow>("SELECT * FROM inbox_messages WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(row.into())
    }

    async fn get_by_topic_offset(
        &self,
        topic: &str,
        offset: i64,
    ) -> Result<Option<InboxMessage>, InboxError> {
        let row = sqlx::query_as::<_, InboxMessageRow>(
            "SELECT * FROM inbox_messages WHERE topic = ? AND offset = ?",
        )
        .bind(topic)
        .bind(offset)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into()))
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: InboxStatus,
        error_message: Option<String>,
    ) -> Result<(), InboxError> {
        sqlx::query("UPDATE inbox_messages SET status = ?, error_message = ? WHERE id = ?")
            .bind(format!("{:?}", status).to_lowercase())
            .bind(&error_message)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn mark_processed(&self, id: Uuid) -> Result<(), InboxError> {
        sqlx::query(
            "UPDATE inbox_messages SET status = 'processed', processed_at = NOW() WHERE id = ?",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), InboxError> {
        sqlx::query("DELETE FROM inbox_messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

// Internal row type for SQLx
#[derive(sqlx::FromRow)]
struct InboxMessageRow {
    id: Uuid,
    topic: String,
    partition: i32,
    offset: i64,
    key: Option<String>,
    event_type: Option<String>,
    payload: serde_json::Value,
    headers: Option<serde_json::Value>,
    timestamp: chrono::DateTime<chrono::Utc>,
    status: String,
    attempt_count: i32,
    error_message: Option<String>,
    received_at: chrono::DateTime<chrono::Utc>,
    processed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<InboxMessageRow> for InboxMessage {
    fn from(row: InboxMessageRow) -> Self {
        Self {
            id: row.id,
            topic: row.topic,
            partition: row.partition,
            offset: row.offset,
            key: row.key,
            event_type: row.event_type,
            payload: row.payload,
            headers: row.headers,
            timestamp: row.timestamp,
            status: match row.status.as_str() {
                "received" => InboxStatus::Received,
                "processing" => InboxStatus::Processing,
                "processed" => InboxStatus::Processed,
                "failed" => InboxStatus::Failed,
                _ => InboxStatus::Received,
            },
            attempt_count: row.attempt_count,
            error_message: row.error_message,
            received_at: row.received_at,
            processed_at: row.processed_at,
        }
    }
}
