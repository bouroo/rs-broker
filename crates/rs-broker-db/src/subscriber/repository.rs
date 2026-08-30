//! Subscriber repository

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
use crate::pool::DbPool;
#[cfg(all(feature = "mysql", not(feature = "postgres")))]
use crate::pool::DbPool;
use async_trait::async_trait;
use uuid::Uuid;

pub use rs_broker_core::features::subscription::ports::{SubscriberError, SubscriberRepository};
use rs_broker_core::features::subscription::Subscriber;

/// SQLx-based subscriber repository
pub struct SqlxSubscriberRepository {
    pool: DbPool,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl SqlxSubscriberRepository {
    /// Create a new repository
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[cfg(all(feature = "postgres", not(feature = "mysql")))]
#[async_trait]
impl SubscriberRepository for SqlxSubscriberRepository {
    async fn create(&self, subscriber: &Subscriber) -> Result<(), SubscriberError> {
        sqlx::query(
            r#"
            INSERT INTO subscribers (
                id, subscriber_id, service_name, grpc_endpoint, topic_patterns,
                active, delivery_config, registered_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(subscriber.id)
        .bind(subscriber.id.to_string())
        .bind(&subscriber.service_name)
        .bind(&subscriber.grpc_endpoint)
        .bind(&subscriber.topic_patterns)
        .bind(subscriber.active)
        .bind(&subscriber.delivery_config)
        .bind(subscriber.registered_at)
        .bind(subscriber.updated_at)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<Subscriber, SubscriberError> {
        let row = sqlx::query_as::<_, SubscriberRow>("SELECT * FROM subscribers WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage)?;

        Ok(row.into())
    }

    async fn get_all_active(&self) -> Result<Vec<Subscriber>, SubscriberError> {
        let rows =
            sqlx::query_as::<_, SubscriberRow>("SELECT * FROM subscribers WHERE active = true")
                .fetch_all(&self.pool)
                .await
                .map_err(storage)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update(&self, subscriber: &Subscriber) -> Result<(), SubscriberError> {
        sqlx::query(
            r#"
            UPDATE subscribers SET
                service_name = $1,
                grpc_endpoint = $2,
                topic_patterns = $3,
                active = $4,
                delivery_config = $5,
                updated_at = NOW()
            WHERE id = $6
            "#,
        )
        .bind(&subscriber.service_name)
        .bind(&subscriber.grpc_endpoint)
        .bind(&subscriber.topic_patterns)
        .bind(subscriber.active)
        .bind(&subscriber.delivery_config)
        .bind(subscriber.id)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), SubscriberError> {
        sqlx::query("DELETE FROM subscribers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }

    async fn deactivate(&self, id: Uuid) -> Result<(), SubscriberError> {
        sqlx::query("UPDATE subscribers SET active = false, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
#[async_trait]
impl SubscriberRepository for SqlxSubscriberRepository {
    async fn create(&self, subscriber: &Subscriber) -> Result<(), SubscriberError> {
        sqlx::query(
            r#"
            INSERT INTO subscribers (
                id, service_name, grpc_endpoint, topic_patterns,
                active, delivery_config, registered_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(subscriber.id)
        .bind(&subscriber.service_name)
        .bind(&subscriber.grpc_endpoint)
        .bind(
            serde_json::to_value(&subscriber.topic_patterns).map_err(|e| {
                tracing::warn!("Failed to serialize topic_patterns: {}", e);
                SubscriberError::Database(sqlx::Error::Decode(Box::new(e)))
            })?,
        )
        .bind(subscriber.active)
        .bind(&subscriber.delivery_config)
        .bind(subscriber.registered_at)
        .bind(subscriber.updated_at)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<Subscriber, SubscriberError> {
        let row = sqlx::query_as::<_, SubscriberRow>("SELECT * FROM subscribers WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage)?;

        Ok(row.into())
    }

    async fn get_all_active(&self) -> Result<Vec<Subscriber>, SubscriberError> {
        let rows =
            sqlx::query_as::<_, SubscriberRow>("SELECT * FROM subscribers WHERE active = true")
                .fetch_all(&self.pool)
                .await
                .map_err(storage)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update(&self, subscriber: &Subscriber) -> Result<(), SubscriberError> {
        sqlx::query(
            r#"
            UPDATE subscribers SET
                service_name = ?,
                grpc_endpoint = ?,
                topic_patterns = ?,
                active = ?,
                delivery_config = ?,
                updated_at = NOW()
            WHERE id = ?
            "#,
        )
        .bind(&subscriber.service_name)
        .bind(&subscriber.grpc_endpoint)
        .bind(
            serde_json::to_value(&subscriber.topic_patterns).map_err(|e| {
                tracing::warn!("Failed to serialize topic_patterns: {}", e);
                SubscriberError::Database(sqlx::Error::Decode(Box::new(e)))
            })?,
        )
        .bind(subscriber.active)
        .bind(&subscriber.delivery_config)
        .bind(subscriber.id)
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), SubscriberError> {
        sqlx::query("DELETE FROM subscribers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }

    async fn deactivate(&self, id: Uuid) -> Result<(), SubscriberError> {
        sqlx::query("UPDATE subscribers SET active = false, updated_at = NOW() WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;

        Ok(())
    }
}

// Internal row type for SQLx
#[derive(sqlx::FromRow)]
struct SubscriberRow {
    id: Uuid,
    service_name: String,
    grpc_endpoint: String,
    #[cfg(all(feature = "postgres", not(feature = "mysql")))]
    topic_patterns: Vec<String>,
    #[cfg(all(feature = "mysql", not(feature = "postgres")))]
    topic_patterns: serde_json::Value,
    active: bool,
    delivery_config: Option<serde_json::Value>,
    registered_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<SubscriberRow> for Subscriber {
    fn from(row: SubscriberRow) -> Self {
        Self {
            id: row.id,
            service_name: row.service_name,
            grpc_endpoint: row.grpc_endpoint,
            #[cfg(all(feature = "postgres", not(feature = "mysql")))]
            topic_patterns: row.topic_patterns,
            #[cfg(all(feature = "mysql", not(feature = "postgres")))]
            topic_patterns: serde_json::from_value(row.topic_patterns).unwrap_or_default(),
            active: row.active,
            delivery_config: row.delivery_config,
            registered_at: row.registered_at,
            updated_at: row.updated_at,
        }
    }
}
fn storage(e: sqlx::Error) -> rs_broker_core::features::subscription::ports::SubscriberError {
    rs_broker_core::features::subscription::ports::SubscriberError::Database(e.to_string())
}
