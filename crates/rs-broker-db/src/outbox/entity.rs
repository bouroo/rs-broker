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

impl std::fmt::Display for MessageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            MessageStatus::Pending => "pending",
            MessageStatus::Publishing => "publishing",
            MessageStatus::Published => "published",
            MessageStatus::Retrying => "retrying",
            MessageStatus::Failed => "failed",
            MessageStatus::Dlq => "dlq",
        };
        f.write_str(s)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_message_new_creates_expected_defaults() {
        let payload = serde_json::json!({"order_id": "123", "amount": 99.95});
        let msg = OutboxMessage::new(
            "Order".to_string(),
            "order-123".to_string(),
            "OrderCreated".to_string(),
            payload.clone(),
            "orders.created".to_string(),
        );

        assert_eq!(msg.aggregate_type, "Order");
        assert_eq!(msg.aggregate_id, "order-123");
        assert_eq!(msg.event_type, "OrderCreated");
        assert_eq!(msg.payload, payload);
        assert_eq!(msg.topic, "orders.created");
        assert_eq!(msg.status, MessageStatus::Pending);
        assert_eq!(msg.retry_count, 0);
        assert!(msg.headers.is_none());
        assert!(msg.partition_key.is_none());
        assert!(msg.error_message.is_none());
        assert!(msg.published_at.is_none());
        assert_eq!(msg.created_at, msg.updated_at);
    }

    #[test]
    fn outbox_message_new_generates_unique_ids() {
        let a = OutboxMessage::new(
            "A".into(),
            "1".into(),
            "Evt".into(),
            serde_json::json!({}),
            "t".into(),
        );
        let b = OutboxMessage::new(
            "A".into(),
            "1".into(),
            "Evt".into(),
            serde_json::json!({}),
            "t".into(),
        );
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn message_status_display_round_trips() {
        for status in [
            MessageStatus::Pending,
            MessageStatus::Publishing,
            MessageStatus::Published,
            MessageStatus::Retrying,
            MessageStatus::Failed,
            MessageStatus::Dlq,
        ] {
            let s = status.to_string();
            let parsed: MessageStatus = serde_json::from_value(serde_json::Value::String(s.clone()))
                .unwrap_or_else(|_| panic!("failed to parse status string: {s}"));
            assert_eq!(parsed, status, "round-trip failed for {s}");
        }
    }

    #[test]
    fn message_status_default_is_pending() {
        assert_eq!(MessageStatus::default(), MessageStatus::Pending);
    }

    #[test]
    fn message_status_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_value(MessageStatus::Pending).unwrap(),
            serde_json::json!("pending")
        );
        assert_eq!(
            serde_json::to_value(MessageStatus::Publishing).unwrap(),
            serde_json::json!("publishing")
        );
        assert_eq!(
            serde_json::to_value(MessageStatus::Published).unwrap(),
            serde_json::json!("published")
        );
        assert_eq!(
            serde_json::to_value(MessageStatus::Retrying).unwrap(),
            serde_json::json!("retrying")
        );
        assert_eq!(
            serde_json::to_value(MessageStatus::Failed).unwrap(),
            serde_json::json!("failed")
        );
        assert_eq!(
            serde_json::to_value(MessageStatus::Dlq).unwrap(),
            serde_json::json!("dlq")
        );
    }
}
