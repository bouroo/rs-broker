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
            id: Uuid::new_v4(),
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
