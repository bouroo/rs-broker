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
            id: Uuid::new_v4(),
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
