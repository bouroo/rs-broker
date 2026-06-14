//! DLQ message entity types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// DLQ message entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlqMessage {
    /// Unique message ID
    pub id: Uuid,

    /// Original outbox message ID
    pub original_message_id: Uuid,

    /// Original topic
    pub original_topic: String,

    /// DLQ topic where message was moved
    pub dlq_topic: String,

    /// Failure reason
    pub failure_reason: String,

    /// Number of retry attempts
    pub retry_count: i32,

    /// Original payload as JSON
    pub payload: serde_json::Value,

    /// Original headers as JSON
    pub headers: Option<serde_json::Value>,

    /// Timestamp when moved to DLQ
    pub created_at: DateTime<Utc>,
}

impl DlqMessage {
    /// Create a new DLQ message
    pub fn new(
        original_message_id: Uuid,
        original_topic: String,
        dlq_topic: String,
        failure_reason: String,
        retry_count: i32,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            original_message_id,
            original_topic,
            dlq_topic,
            failure_reason,
            retry_count,
            payload,
            headers: None,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dlq_message_new_creates_expected_values() {
        let original_id = Uuid::now_v7();
        let payload = serde_json::json!({"order": 42});
        let before = Utc::now();

        let msg = DlqMessage::new(
            original_id,
            "orders.created".to_string(),
            "orders.created.dlq".to_string(),
            "max retries exceeded".to_string(),
            5,
            payload.clone(),
        );

        let after = Utc::now();

        assert_ne!(msg.id, Uuid::nil());
        assert_ne!(msg.id, original_id);
        assert_eq!(msg.original_message_id, original_id);
        assert_eq!(msg.original_topic, "orders.created");
        assert_eq!(msg.dlq_topic, "orders.created.dlq");
        assert_eq!(msg.failure_reason, "max retries exceeded");
        assert_eq!(msg.retry_count, 5);
        assert_eq!(msg.payload, payload);
        assert!(msg.headers.is_none());
        assert!(
            msg.created_at >= before && msg.created_at <= after,
            "created_at should be between before and after timestamps"
        );
    }

    #[test]
    fn dlq_message_round_trips_serde() {
        let original_id = Uuid::now_v7();
        let msg = DlqMessage::new(
            original_id,
            "src".to_string(),
            "dlq".to_string(),
            "reason".to_string(),
            3,
            serde_json::json!({"k": "v"}),
        );

        let serialized = serde_json::to_string(&msg).unwrap();
        let deserialized: DlqMessage = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.id, msg.id);
        assert_eq!(deserialized.original_message_id, msg.original_message_id);
        assert_eq!(deserialized.original_topic, msg.original_topic);
        assert_eq!(deserialized.dlq_topic, msg.dlq_topic);
        assert_eq!(deserialized.failure_reason, msg.failure_reason);
        assert_eq!(deserialized.retry_count, msg.retry_count);
        assert_eq!(deserialized.payload, msg.payload);
    }
}
