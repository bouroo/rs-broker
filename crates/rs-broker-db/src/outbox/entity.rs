//! Outbox entity types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Message status in the outbox
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    #[default]
    Pending,
    Publishing,
    Published,
    Retrying,
    Failed,
    Dlq,
}

/// Outbox message entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxMessage {
    /// Unique message ID
    pub id: Uuid,

    /// Aggregate type (e.g., "Order", "Payment")
    pub aggregate_type: String,

    /// Aggregate ID
    pub aggregate_id: String,

    /// Event type (e.g., "OrderCreated", "PaymentReceived")
    pub event_type: String,

    /// JSON payload
    pub payload: serde_json::Value,

    /// Message headers as JSON
    pub headers: Option<serde_json::Value>,

    /// Target Kafka topic
    pub topic: String,

    /// Partition key
    pub partition_key: Option<String>,

    /// Current status
    pub status: MessageStatus,

    /// Number of retry attempts
    pub retry_count: i32,

    /// Error message if failed
    pub error_message: Option<String>,

    /// Timestamp when message was created
    pub created_at: DateTime<Utc>,

    /// Timestamp when status was last updated
    pub updated_at: DateTime<Utc>,

    /// Timestamp when message was published to Kafka
    pub published_at: Option<DateTime<Utc>>,
}

impl OutboxMessage {
    /// Create a new outbox message
    pub fn new(
        aggregate_type: String,
        aggregate_id: String,
        event_type: String,
        payload: serde_json::Value,
        topic: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            aggregate_type,
            aggregate_id,
            event_type,
            payload,
            headers: None,
            topic,
            partition_key: None,
            status: MessageStatus::Pending,
            retry_count: 0,
            error_message: None,
            created_at: now,
            updated_at: now,
            published_at: None,
        }
    }
}
