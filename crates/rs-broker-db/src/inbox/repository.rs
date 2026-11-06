//! Inbox repository

#[cfg(any(feature = "postgres", feature = "mysql"))]
use crate::pool::DbPool;
use async_trait::async_trait;
use uuid::Uuid;

pub use rs_broker_core::features::consuming::ports::{InboxError, InboxRepository};
use rs_broker_core::features::consuming::{InboxMessage, InboxStatus};

/// SQLx-based inbox repository
pub struct SqlxInboxRepository {
    pool: DbPool,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl SqlxInboxRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
#[async_trait]
impl InboxRepository for SqlxInboxRepository {
    async fn create(&self, message: &InboxMessage) -> Result<(), InboxError> {
        sqlx::query(
            r#"
            INSERT INTO inbox_messages (
                id, message_id, topic, partition, "offset", key, event_type,
                payload, headers, timestamp, status,
                attempt_count, error_message, received_at, processed_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
        )
        .bind(message.id)
        .bind(message.id)
        .bind(&message.topic)
        .bind(message.partition)
        .bind(message.offset)
        .bind(&message.key)
        .bind(&message.event_type)
        .bind(&message.payload)
        .bind(&message.headers)
        .bind(message.timestamp)
        .bind(message.status.to_string())
        .bind(message.attempt_count)
        .bind(&message.error_message)
        .bind(message.received_at)
        .bind(message.processed_at)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    /// Store a batch of inbox messages with a single round trip.
    ///
    /// The consumer's per-message cost was dominated by one database round
    /// trip per message; this pushes every column as a Postgres array and
    /// unnests it, so a batch costs one statement.
    async fn create_batch(&self, messages: &[InboxMessage]) -> Result<(), InboxError> {
        if messages.is_empty() {
            return Ok(());
        }

        let mut ids = Vec::with_capacity(messages.len());
        let mut topics = Vec::with_capacity(messages.len());
        let mut partitions = Vec::with_capacity(messages.len());
        let mut offsets = Vec::with_capacity(messages.len());
        let mut keys = Vec::with_capacity(messages.len());
        let mut event_types = Vec::with_capacity(messages.len());
        let mut payloads = Vec::with_capacity(messages.len());
        let mut headers = Vec::with_capacity(messages.len());
        let mut timestamps = Vec::with_capacity(messages.len());
        let mut statuses = Vec::with_capacity(messages.len());
        let mut attempt_counts = Vec::with_capacity(messages.len());
        let mut error_messages = Vec::with_capacity(messages.len());
        let mut received_at = Vec::with_capacity(messages.len());
        let mut processed_at = Vec::with_capacity(messages.len());

        for message in messages {
            ids.push(message.id);
            topics.push(message.topic.clone());
            partitions.push(message.partition);
            offsets.push(message.offset);
            keys.push(message.key.clone());
            event_types.push(message.event_type.clone());
            payloads.push(message.payload.clone());
            headers.push(message.headers.clone());
            timestamps.push(message.timestamp);
            statuses.push(message.status.to_string());
            attempt_counts.push(message.attempt_count);
            error_messages.push(message.error_message.clone());
            received_at.push(message.received_at);
            processed_at.push(message.processed_at);
        }

        sqlx::query(
            r#"
            INSERT INTO inbox_messages (
                id, message_id, topic, partition, "offset", key, event_type,
                payload, headers, timestamp, status,
                attempt_count, error_message, received_at, processed_at
            )
            SELECT
                id, id, topic, partition, "offset", key, event_type,
                payload, headers, timestamp, status,
                attempt_count, error_message, received_at, processed_at
            FROM UNNEST(
                $1::uuid[], $2::text[], $3::int[], $4::bigint[],
                $5::text[], $6::text[], $7::jsonb[], $8::jsonb[],
                $9::timestamptz[], $10::text[], $11::int[], $12::text[],
                $13::timestamptz[], $14::timestamptz[]
            ) AS batch(
                id, topic, partition, "offset", key, event_type,
                payload, headers, timestamp, status,
                attempt_count, error_message, received_at, processed_at
            )
            "#,
        )
        .bind(ids)
        .bind(topics)
        .bind(partitions)
        .bind(offsets)
        .bind(keys)
        .bind(event_types)
        .bind(payloads)
        .bind(headers)
        .bind(timestamps)
        .bind(statuses)
        .bind(attempt_counts)
        .bind(error_messages)
        .bind(received_at)
        .bind(processed_at)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }
    async fn get_by_id(&self, id: Uuid) -> Result<InboxMessage, InboxError> {
        let row =
            sqlx::query_as::<_, InboxMessageRow>("SELECT * FROM inbox_messages WHERE id = $1")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;

        Ok(row.into())
    }

    async fn get_by_topic_offset(
        &self,
        topic: &str,
        offset: i64,
    ) -> Result<Option<InboxMessage>, InboxError> {
        let row = sqlx::query_as::<_, InboxMessageRow>(
            "SELECT * FROM inbox_messages WHERE topic = $1 AND \"offset\" = $2",
        )
        .bind(topic)
        .bind(offset)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;

        Ok(row.map(|r| r.into()))
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: InboxStatus,
        error_message: Option<String>,
    ) -> Result<(), InboxError> {
        sqlx::query(
            "UPDATE inbox_messages SET status = $1, error_message = $2, updated_at = NOW() WHERE id = $3"
        )
        .bind(status.to_string())
        .bind(&error_message)
        .bind(id)
        .execute(&self.pool)
        .await.map_err(storage)?;

        Ok(())
    }

    async fn mark_processed(&self, id: Uuid) -> Result<(), InboxError> {
        sqlx::query(
            "UPDATE inbox_messages SET status = 'processed', processed_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), InboxError> {
        sqlx::query("DELETE FROM inbox_messages WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
#[async_trait]
impl InboxRepository for SqlxInboxRepository {
    async fn create(&self, message: &InboxMessage) -> Result<(), InboxError> {
        sqlx::query(
            r#"
            INSERT INTO inbox_messages (
                id, message_id, topic, partition, `offset`, key, event_type,
                payload, headers, timestamp, status,
                attempt_count, error_message, received_at, processed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(message.id)
        .bind(message.id)
        .bind(&message.topic)
        .bind(message.partition)
        .bind(message.offset)
        .bind(&message.key)
        .bind(&message.event_type)
        .bind(&message.payload)
        .bind(&message.headers)
        .bind(message.timestamp)
        .bind(message.status.to_string())
        .bind(message.attempt_count)
        .bind(&message.error_message)
        .bind(message.received_at)
        .bind(message.processed_at)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    /// Store a batch of inbox messages with a single round trip.
    ///
    /// MySQL has no `UNNEST`, so the rows are written with one multi-row
    /// `INSERT` instead of one statement per message.
    async fn create_batch(&self, messages: &[InboxMessage]) -> Result<(), InboxError> {
        if messages.is_empty() {
            return Ok(());
        }
        // Multi-row INSERT: 15 columns per row, chunked to stay well under
        // the prepared-statement parameter limit.
        const MAX_ROWS: usize = 100;

        for chunk in messages.chunks(MAX_ROWS) {
            let mut sql = String::from(
                "INSERT INTO inbox_messages (id, message_id, topic, partition, `offset`, key, \
                 event_type, payload, headers, timestamp, status, attempt_count, error_message, \
                 received_at, processed_at) VALUES ",
            );
            for (i, _) in chunk.iter().enumerate() {
                if i > 0 {
                    sql.push_str(", ");
                }
                sql.push_str("(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)");
            }

            let mut query = sqlx::query(&sql);
            for message in chunk {
                query = query
                    .bind(message.id)
                    .bind(message.id)
                    .bind(&message.topic)
                    .bind(message.partition)
                    .bind(message.offset)
                    .bind(&message.key)
                    .bind(&message.event_type)
                    .bind(&message.payload)
                    .bind(&message.headers)
                    .bind(message.timestamp)
                    .bind(message.status.to_string())
                    .bind(message.attempt_count)
                    .bind(&message.error_message)
                    .bind(message.received_at)
                    .bind(message.processed_at);
            }

            query.execute(&self.pool).await.map_err(storage)?;
        }
        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<InboxMessage, InboxError> {
        let row = sqlx::query_as::<_, InboxMessageRow>("SELECT * FROM inbox_messages WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage)?;

        Ok(row.into())
    }

    async fn get_by_topic_offset(
        &self,
        topic: &str,
        offset: i64,
    ) -> Result<Option<InboxMessage>, InboxError> {
        let row = sqlx::query_as::<_, InboxMessageRow>(
            "SELECT * FROM inbox_messages WHERE topic = ? AND `offset` = ?",
        )
        .bind(topic)
        .bind(offset)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;

        Ok(row.map(|r| r.into()))
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: InboxStatus,
        error_message: Option<String>,
    ) -> Result<(), InboxError> {
        sqlx::query("UPDATE inbox_messages SET status = ?, error_message = ?, updated_at = NOW() WHERE id = ?")
            .bind(status.to_string())
            .bind(&error_message)
            .bind(id)
            .execute(&self.pool)
            .await.map_err(storage)?;

        Ok(())
    }

    async fn mark_processed(&self, id: Uuid) -> Result<(), InboxError> {
        sqlx::query(
            "UPDATE inbox_messages SET status = 'processed', processed_at = NOW() WHERE id = ?",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), InboxError> {
        sqlx::query("DELETE FROM inbox_messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

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
fn storage(e: sqlx::Error) -> rs_broker_core::features::consuming::ports::InboxError {
    rs_broker_core::features::consuming::ports::InboxError::Database(e.to_string())
}
