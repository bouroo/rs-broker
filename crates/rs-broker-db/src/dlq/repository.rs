//! DLQ repository

#[cfg(any(feature = "postgres", feature = "mysql"))]
use crate::pool::DbPool;
use async_trait::async_trait;
use uuid::Uuid;

pub use rs_broker_core::features::dead_letter::ports::{DlqError, DlqRepository};
use rs_broker_core::features::dead_letter::DlqMessage;

/// SQLx-based DLQ repository
pub struct SqlxDlqRepository {
    pool: DbPool,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl SqlxDlqRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
#[async_trait]
impl DlqRepository for SqlxDlqRepository {
    async fn create(&self, message: &DlqMessage) -> Result<(), DlqError> {
        sqlx::query(
            r#"
            INSERT INTO dlq_messages (
                id, original_message_id, original_topic, dlq_topic,
                failure_reason, retry_count, payload, headers, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
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
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<DlqMessage, DlqError> {
        let row = sqlx::query_as::<_, DlqMessageRow>("SELECT * FROM dlq_messages WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage)?;

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
                "SELECT * FROM dlq_messages WHERE original_topic = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
            )
                .bind(t)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await.map_err(storage)?
        } else {
            sqlx::query_as::<_, DlqMessageRow>(
                "SELECT * FROM dlq_messages ORDER BY created_at DESC LIMIT $1 OFFSET $2",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(storage)?
        };

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn count(&self, topic: Option<&str>) -> Result<i64, DlqError> {
        let count: (i64,) = if let Some(t) = topic {
            sqlx::query_as("SELECT COUNT(*) FROM dlq_messages WHERE original_topic = $1")
                .bind(t)
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?
        } else {
            sqlx::query_as("SELECT COUNT(*) FROM dlq_messages")
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?
        };

        Ok(count.0)
    }

    async fn delete(&self, id: Uuid) -> Result<(), DlqError> {
        sqlx::query("DELETE FROM dlq_messages WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DlqError> {
        sqlx::query("DELETE FROM dlq_messages")
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
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
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<DlqMessage, DlqError> {
        let row = sqlx::query_as::<_, DlqMessageRow>("SELECT * FROM dlq_messages WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage)?;

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
                .await.map_err(storage)?
        } else {
            sqlx::query_as::<_, DlqMessageRow>(
                "SELECT * FROM dlq_messages ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(storage)?
        };

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn count(&self, topic: Option<&str>) -> Result<i64, DlqError> {
        let count: (i64,) = if let Some(t) = topic {
            sqlx::query_as("SELECT COUNT(*) FROM dlq_messages WHERE original_topic = ?")
                .bind(t)
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?
        } else {
            sqlx::query_as("SELECT COUNT(*) FROM dlq_messages")
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?
        };

        Ok(count.0)
    }

    async fn delete(&self, id: Uuid) -> Result<(), DlqError> {
        sqlx::query("DELETE FROM dlq_messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DlqError> {
        sqlx::query("DELETE FROM dlq_messages")
            .execute(&self.pool)
            .await
            .map_err(storage)?;

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
fn storage(e: sqlx::Error) -> rs_broker_core::features::dead_letter::ports::DlqError {
    rs_broker_core::features::dead_letter::ports::DlqError::Database(e.to_string())
}
