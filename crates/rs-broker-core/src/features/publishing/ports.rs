//! Publishing ports: interfaces the use cases depend on, implemented outward
//! (persistence by `rs-broker-db`, transport by `rs-broker-kafka`).

use async_trait::async_trait;
use uuid::Uuid;

use super::domain::OutboxMessage;

/// Error type for outbox repository operations.
#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    /// Storage failure; the storage driver's message is carried verbatim so
    /// operator-facing error text is unchanged across adapter swaps.
    #[error("Database error: {0}")]
    Database(String),

    #[error("Message not found: {0}")]
    NotFound(Uuid),
}

/// Port: durable storage for outbox messages.
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

    /// Atomically claim up to `limit` pending messages for publishing.
    ///
    /// Claimed messages flip to the `publishing` status, so concurrent
    /// publishers — or replicas of one — observe disjoint sets. Rows abandoned
    /// in `publishing` by a crashed publisher become claimable again once they
    /// have not been touched for `lease_secs`, measured against the database
    /// clock so app/server clock skew cannot cause double claims. Draining is
    /// at-least-once; consumers dedup by `message_id`.
    async fn claim_pending(
        &self,
        limit: i64,
        lease_secs: i32,
    ) -> Result<Vec<OutboxMessage>, OutboxError>;

    /// Mark claimed messages as published in one round trip.
    ///
    /// Returns the number of rows actually marked; a count short of
    /// `ids.len()` means another publisher re-claimed lease-expired rows,
    /// which may be published again.
    async fn mark_published_batch(&self, ids: &[Uuid]) -> Result<u64, OutboxError>;

    /// Update message status
    async fn update_status(
        &self,
        id: Uuid,
        status: crate::features::publishing::domain::MessageStatus,
        error_message: Option<String>,
    ) -> Result<(), OutboxError>;

    /// Increment retry count, set status to `retrying`, and store the error.
    ///
    /// Returns the new retry count.
    async fn increment_retry(
        &self,
        id: Uuid,
        error_message: Option<String>,
    ) -> Result<i32, OutboxError>;

    /// Mark message as published
    async fn mark_published(&self, id: Uuid) -> Result<(), OutboxError>;

    /// Delete a message
    async fn delete(&self, id: Uuid) -> Result<(), OutboxError>;
}

/// Port: outbound message transport for the publishing drain.
///
/// The use case converts domain messages into [`OutboundMessage`]s and hands
/// them to the sink; the adapter (Kafka in `rs-broker-kafka`) owns all
/// transport mechanics.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    /// Target topic
    pub topic: String,
    /// Message key
    pub key: Option<String>,
    /// Message payload (bytes)
    pub payload: Vec<u8>,
    /// Partition (None for auto)
    pub partition: Option<i32>,
}

/// Transport failure reported by the sink adapter. The adapter's original
/// message text is preserved verbatim.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    #[error("{0}")]
    Transport(String),
}

/// Port: where the publishing drain sends messages (Kafka today).
pub trait MessageSink: Send + Sync {
    /// Enqueue a message for delivery. Mirrors the historical synchronous
    /// queuing semantics of the rdkafka `BaseProducer` used by the Kafka
    /// adapter.
    fn send(&self, message: OutboundMessage) -> std::result::Result<(), SinkError>;
}
