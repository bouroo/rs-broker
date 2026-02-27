//! DLQ repository

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
use crate::pool::DbPool;
#[cfg(all(feature = "mysql", not(feature = "postgres")))]
use crate::pool::DbPool;
use async_trait::async_trait;
use uuid::Uuid;

use super::entity::DlqMessage;

/// Error type for DLQ repository operations
#[derive(Debug, thiserror::Error)]
pub enum DlqError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Message not found: {0}")]
    NotFound(Uuid),
}

/// DLQ repository trait
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

/// SQLx-based DLQ repository
pub struct SqlxDlqRepository {
    pool: DbPool,
}

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
impl SqlxDlqRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
impl SqlxDlqRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DlqRepository for SqlxDlqRepository {
    async fn create(&self, message: &DlqMessage) -> Result<(), DlqError> {
        sqlx::query(
            r#"
            INSERT INTO dlq_messages (
                id, original_message_id, original_topic, dlq_topic,
                failure_reason, retry_count, payload, headers, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(message.id)
        .bind(message.original_message_id)
        .bind(&message.original_topic)
        .bind(&message.dlq_topic)
        .bind(&message.failure_reason)
        .bind(message.retry_count)
        .bind(&message.payload)
        .bind(&message.headers)
        .bind(message.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<DlqMessage, DlqError> {
        let row = sqlx::query_as::<_, DlqMessageRow>("SELECT * FROM dlq_messages WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(row.into())
    }

    async fn get_all(
        &self,
        topic: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<DlqMessage>, DlqError> {
        let rows = if let Some(t) = topic {
            sqlx::query_as::<_, DlqMessageRow>(
                "SELECT * FROM dlq_messages WHERE original_topic = ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
            )
                .bind(t)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query_as::<_, DlqMessageRow>(
                "SELECT * FROM dlq_messages ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn count(&self, topic: Option<&str>) -> Result<i64, DlqError> {
        let count: (i64,) = if let Some(t) = topic {
            sqlx::query_as("SELECT COUNT(*) FROM dlq_messages WHERE original_topic = ?")
                .bind(t)
                .fetch_one(&self.pool)
                .await?
        } else {
            sqlx::query_as("SELECT COUNT(*) FROM dlq_messages")
                .fetch_one(&self.pool)
                .await?
        };

        Ok(count.0)
    }

    async fn delete(&self, id: Uuid) -> Result<(), DlqError> {
        sqlx::query("DELETE FROM dlq_messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DlqError> {
        sqlx::query("DELETE FROM dlq_messages")
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

// Internal row type for SQLx
#[derive(sqlx::FromRow)]
struct DlqMessageRow {
    id: Uuid,
    original_message_id: Uuid,
    original_topic: String,
    dlq_topic: String,
    failure_reason: String,
    retry_count: i32,
    payload: serde_json::Value,
    headers: Option<serde_json::Value>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<DlqMessageRow> for DlqMessage {
    fn from(row: DlqMessageRow) -> Self {
        Self {
            id: row.id,
            original_message_id: row.original_message_id,
            original_topic: row.original_topic,
            dlq_topic: row.dlq_topic,
            failure_reason: row.failure_reason,
            retry_count: row.retry_count,
            payload: row.payload,
            headers: row.headers,
            created_at: row.created_at,
        }
    }
}
