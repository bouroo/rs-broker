//! Outbox repository

#[cfg(any(feature = "postgres", feature = "mysql"))]
use crate::pool::DbPool;
use async_trait::async_trait;
use uuid::Uuid;

pub use rs_broker_core::features::publishing::ports::{OutboxError, OutboxRepository};
use rs_broker_core::features::publishing::{MessageStatus, OutboxMessage};

/// SQLx-based outbox repository
#[derive(Clone)]
pub struct SqlxOutboxRepository {
    pool: DbPool,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl SqlxOutboxRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
#[async_trait]
impl OutboxRepository for SqlxOutboxRepository {
    async fn create(&self, message: &OutboxMessage) -> Result<(), OutboxError> {
        sqlx::query(
            r#"
            INSERT INTO outbox_messages (
                id, message_id, aggregate_type, aggregate_id, event_type,
                payload, headers, topic, partition_key,
                status, retry_count, error_message,
                created_at, updated_at, published_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
        )
        .bind(message.id)
        .bind(message.id)
        .bind(&message.aggregate_type)
        .bind(&message.aggregate_id)
        .bind(&message.event_type)
        .bind(&message.payload)
        .bind(&message.headers)
        .bind(&message.topic)
        .bind(&message.partition_key)
        .bind(message.status.to_string())
        .bind(message.retry_count)
        .bind(&message.error_message)
        .bind(message.created_at)
        .bind(message.updated_at)
        .bind(message.published_at)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn create_batch(&self, messages: &[OutboxMessage]) -> Result<(), OutboxError> {
        if messages.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(storage)?;

        for message in messages {
            sqlx::query(
                r#"
                INSERT INTO outbox_messages (
                    id, message_id, aggregate_type, aggregate_id, event_type,
                    payload, headers, topic, partition_key,
                    status, retry_count, error_message,
                    created_at, updated_at, published_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                "#,
            )
            .bind(message.id)
            .bind(message.id)
            .bind(&message.aggregate_type)
            .bind(&message.aggregate_id)
            .bind(&message.event_type)
            .bind(&message.payload)
            .bind(&message.headers)
            .bind(&message.topic)
            .bind(&message.partition_key)
            .bind(message.status.to_string())
            .bind(message.retry_count)
            .bind(&message.error_message)
            .bind(message.created_at)
            .bind(message.updated_at)
            .bind(message.published_at)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        }

        tx.commit().await.map_err(storage)?;
        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<OutboxMessage, OutboxError> {
        let row =
            sqlx::query_as::<_, OutboxMessageRow>("SELECT * FROM outbox_messages WHERE id = $1")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;

        Ok(row.into())
    }

    async fn get_pending(&self, limit: i64) -> Result<Vec<OutboxMessage>, OutboxError> {
        let rows = sqlx::query_as::<_, OutboxMessageRow>(
            "SELECT * FROM outbox_messages WHERE status IN ('pending', 'retrying') ORDER BY created_at ASC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await.map_err(storage)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn claim_pending(
        &self,
        limit: i64,
        lease_secs: i32,
    ) -> Result<Vec<OutboxMessage>, OutboxError> {
        // Atomic claim: the subquery takes candidate rows with SKIP LOCKED so
        // concurrent publishers never wait on each other, and the outer UPDATE
        // flips them to `publishing` in the same statement — each claimed set
        // is exclusive to this caller. Rows already in `publishing` past the
        // lease were abandoned by a crashed publisher and are safe to reclaim
        // (draining is at-least-once; consumers dedup by message_id). The
        // lease is measured against the database clock: `updated_at` is
        // trigger-maintained on this table, so it is the authoritative
        // last-touch time.
        let rows = sqlx::query_as::<_, OutboxMessageRow>(
            r#"
            UPDATE outbox_messages
            SET status = 'publishing', updated_at = NOW()
            WHERE id IN (
                SELECT id FROM outbox_messages
                WHERE status IN ('pending', 'retrying')
                   OR (status = 'publishing' AND updated_at < NOW() - ($2::int * INTERVAL '1 second'))
                ORDER BY created_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING *
            "#,
        )
        .bind(limit)
        .bind(lease_secs)
        .fetch_all(&self.pool)
        .await
        .map_err(storage)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn mark_published_batch(&self, ids: &[Uuid]) -> Result<u64, OutboxError> {
        if ids.is_empty() {
            return Ok(0);
        }

        // Guarded by `status = 'publishing'` so rows another publisher
        // re-claimed after the lease expired are not marked from here.
        let result = sqlx::query(
            "UPDATE outbox_messages SET status = 'published', published_at = NOW(), updated_at = NOW() WHERE id = ANY($1) AND status = 'publishing'",
        )
        .bind(ids.to_vec())
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(result.rows_affected())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: MessageStatus,
        error_message: Option<String>,
    ) -> Result<(), OutboxError> {
        sqlx::query(
            "UPDATE outbox_messages SET status = $1, error_message = $2, updated_at = NOW() WHERE id = $3"
        )
        .bind(status.to_string())
        .bind(&error_message)
        .bind(id)
        .execute(&self.pool)
        .await.map_err(storage)?;

        Ok(())
    }

    async fn increment_retry(
        &self,
        id: Uuid,
        error_message: Option<String>,
    ) -> Result<i32, OutboxError> {
        let row: (i32,) = sqlx::query_as(
            r#"
            UPDATE outbox_messages
            SET retry_count = retry_count + 1,
                status = 'retrying',
                error_message = $1,
                updated_at = NOW()
            WHERE id = $2
            RETURNING retry_count
            "#,
        )
        .bind(&error_message)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(storage)?;

        Ok(row.0)
    }

    async fn mark_published(&self, id: Uuid) -> Result<(), OutboxError> {
        sqlx::query(
            "UPDATE outbox_messages SET status = 'published', published_at = NOW(), updated_at = NOW() WHERE id = $1"
        )
        .bind(id)
        .execute(&self.pool)
        .await.map_err(storage)?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), OutboxError> {
        sqlx::query("DELETE FROM outbox_messages WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
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
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(message.status.to_string())
        .bind(message.retry_count)
        .bind(&message.error_message)
        .bind(message.created_at)
        .bind(message.updated_at)
        .bind(message.published_at)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn create_batch(&self, messages: &[OutboxMessage]) -> Result<(), OutboxError> {
        if messages.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(storage)?;

        for message in messages {
            sqlx::query(
                r#"
                INSERT INTO outbox_messages (
                    id, aggregate_type, aggregate_id, event_type,
                    payload, headers, topic, partition_key,
                    status, retry_count, error_message,
                    created_at, updated_at, published_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            .bind(message.status.to_string())
            .bind(message.retry_count)
            .bind(&message.error_message)
            .bind(message.created_at)
            .bind(message.updated_at)
            .bind(message.published_at)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        }

        tx.commit().await.map_err(storage)?;
        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<OutboxMessage, OutboxError> {
        let row =
            sqlx::query_as::<_, OutboxMessageRow>("SELECT * FROM outbox_messages WHERE id = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;

        Ok(row.into())
    }

    async fn get_pending(&self, limit: i64) -> Result<Vec<OutboxMessage>, OutboxError> {
        let rows = sqlx::query_as::<_, OutboxMessageRow>(
            "SELECT * FROM outbox_messages WHERE status IN ('pending', 'retrying') ORDER BY created_at ASC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await.map_err(storage)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn claim_pending(
        &self,
        limit: i64,
        _lease_secs: i32,
    ) -> Result<Vec<OutboxMessage>, OutboxError> {
        // MySQL/MariaDB have no portable claim-and-return form (no
        // UPDATE..RETURNING; MariaDB lacks SKIP LOCKED), so claiming stays
        // read-only: draining against MySQL remains correct only with a
        // single publisher replica, as before.
        self.get_pending(limit).await
    }

    async fn mark_published_batch(&self, ids: &[Uuid]) -> Result<u64, OutboxError> {
        if ids.is_empty() {
            return Ok(0);
        }

        let mut marked = 0u64;
        // Placeholder-bound chunks stay well under the prepared-statement
        // parameter limit for any configured batch size.
        for chunk in ids.chunks(1000) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "UPDATE outbox_messages SET status = 'published', published_at = NOW(), updated_at = NOW() WHERE id IN ({placeholders})"
            );
            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(*id);
            }
            marked += query
                .execute(&self.pool)
                .await
                .map_err(storage)?
                .rows_affected();
        }

        Ok(marked)
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
        .bind(status.to_string())
        .bind(&error_message)
        .bind(id)
        .execute(&self.pool)
        .await.map_err(storage)?;

        Ok(())
    }

    async fn increment_retry(
        &self,
        id: Uuid,
        error_message: Option<String>,
    ) -> Result<i32, OutboxError> {
        let row: (i32,) = sqlx::query_as(
            r#"
            UPDATE outbox_messages
            SET retry_count = retry_count + 1,
                status = 'retrying',
                error_message = ?,
                updated_at = NOW()
            WHERE id = ?
            RETURNING retry_count
            "#,
        )
        .bind(&error_message)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(storage)?;

        Ok(row.0)
    }

    async fn mark_published(&self, id: Uuid) -> Result<(), OutboxError> {
        sqlx::query(
            "UPDATE outbox_messages SET status = 'published', published_at = NOW(), updated_at = NOW() WHERE id = ?"
        )
        .bind(id)
        .execute(&self.pool)
        .await.map_err(storage)?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), OutboxError> {
        sqlx::query("DELETE FROM outbox_messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

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
fn storage(e: sqlx::Error) -> rs_broker_core::features::publishing::ports::OutboxError {
    rs_broker_core::features::publishing::ports::OutboxError::Database(e.to_string())
}
