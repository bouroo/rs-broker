//! Inbox entity types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Inbox message status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InboxStatus {
    #[default]
    Received,
    Processing,
    Processed,
    Failed,
}

impl std::fmt::Display for InboxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            InboxStatus::Received => "received",
            InboxStatus::Processing => "processing",
            InboxStatus::Processed => "processed",
            InboxStatus::Failed => "failed",
        };
        f.write_str(s)
    }
}

/// Inbox message entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    /// Unique message ID
    pub id: Uuid,

    /// Source topic
    pub topic: String,

    /// Partition
    pub partition: i32,

    /// Offset
    pub offset: i64,

    /// Message key
    pub key: Option<String>,

    /// Event type
    pub event_type: Option<String>,

    /// Message payload
    pub payload: serde_json::Value,

    /// Message headers
    pub headers: Option<serde_json::Value>,

    /// Timestamp from Kafka
    pub timestamp: DateTime<Utc>,

    /// Current status
    pub status: InboxStatus,

    /// Number of processing attempts
    pub attempt_count: i32,

    /// Error message if failed
    pub error_message: Option<String>,

    /// Timestamp when received
    pub received_at: DateTime<Utc>,

    /// Timestamp when processed
    pub processed_at: Option<DateTime<Utc>>,
}

impl InboxMessage {
    /// Create a new inbox message
    pub fn new(topic: String, partition: i32, offset: i64, payload: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            topic,
            partition,
            offset,
            key: None,
            event_type: None,
            payload,
            headers: None,
            timestamp: now,
            status: InboxStatus::Received,
            attempt_count: 0,
            error_message: None,
            received_at: now,
            processed_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_message_new_creates_expected_defaults() {
        let payload = serde_json::json!({"event": "ping"});
        let msg = InboxMessage::new("orders.created".to_string(), 3, 4242, payload.clone());

        assert_eq!(msg.topic, "orders.created");
        assert_eq!(msg.partition, 3);
        assert_eq!(msg.offset, 4242);
        assert_eq!(msg.payload, payload);
        assert_eq!(msg.status, InboxStatus::Received);
        assert_eq!(msg.attempt_count, 0);
        assert!(msg.key.is_none());
        assert!(msg.event_type.is_none());
        assert!(msg.headers.is_none());
        assert!(msg.error_message.is_none());
        assert!(msg.processed_at.is_none());
        assert_eq!(msg.received_at, msg.timestamp);
    }

    #[test]
    fn inbox_message_new_generates_unique_ids() {
        let a = InboxMessage::new("t".into(), 0, 0, serde_json::json!({}));
        let b = InboxMessage::new("t".into(), 0, 0, serde_json::json!({}));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn inbox_status_display_round_trips() {
        for status in [
            InboxStatus::Received,
            InboxStatus::Processing,
            InboxStatus::Processed,
            InboxStatus::Failed,
        ] {
            let s = status.to_string();
            let parsed: InboxStatus = serde_json::from_value(serde_json::Value::String(s.clone()))
                .unwrap_or_else(|_| panic!("failed to parse status string: {s}"));
            assert_eq!(parsed, status, "round-trip failed for {s}");
        }
    }

    #[test]
    fn inbox_status_default_is_received() {
        assert_eq!(InboxStatus::default(), InboxStatus::Received);
    }

    #[test]
    fn inbox_status_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_value(InboxStatus::Received).unwrap(),
            serde_json::json!("received")
        );
        assert_eq!(
            serde_json::to_value(InboxStatus::Processing).unwrap(),
            serde_json::json!("processing")
        );
        assert_eq!(
            serde_json::to_value(InboxStatus::Processed).unwrap(),
            serde_json::json!("processed")
        );
        assert_eq!(
            serde_json::to_value(InboxStatus::Failed).unwrap(),
            serde_json::json!("failed")
        );
    }
}
