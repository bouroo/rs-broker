//! Publish acceptance use case.
//!
//! Extracted from the gRPC adapter so publish validation and outbox insertion
//! are application logic, not controller code. The adapter converts proto →
//! transport-free input and maps [`AcceptError`] to its own error type.

use uuid::Uuid;

use crate::features::publishing::ports::OutboxRepository;
use crate::features::publishing::OutboxMessage;

/// Transport-free publish request for the [`AcceptMessage`] use case.
#[derive(Debug, Clone, Default)]
pub struct PublishRequestInput {
    /// Client-supplied message ID; empty → generate a UUIDv7
    pub message_id: Option<String>,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub event_type: String,
    /// Raw JSON bytes; empty → `{}`
    pub payload: Vec<u8>,
    /// Header key/value pairs
    pub headers: Vec<(String, String)>,
    pub topic: String,
    pub partition_key: Option<String>,
}

/// Failure of the accept path.
#[derive(Debug, thiserror::Error)]
pub enum AcceptError {
    #[error("Invalid payload JSON: {0}")]
    InvalidPayload(String),

    #[error("Invalid message_id: {0}")]
    InvalidMessageId(String),

    /// Storage failure; text preserved from the repository port.
    #[error("Failed to create message: {0}")]
    Storage(String),
}

/// Accept a publish request into the outbox.
///
/// Cheap to clone: shares the repository port via `Arc`.
#[derive(Clone)]
pub struct AcceptMessage {
    outbox: std::sync::Arc<dyn OutboxRepository>,
}

impl AcceptMessage {
    /// Create the use case over the outbox repository port.
    pub fn new(outbox: std::sync::Arc<dyn OutboxRepository>) -> Self {
        Self { outbox }
    }

    /// Validate and persist the message; returns the reported message ID.
    pub async fn apply(&self, input: PublishRequestInput) -> Result<Uuid, AcceptError> {
        let message_id = match input.message_id {
            Some(id) if !id.is_empty() => {
                Uuid::parse_str(&id).map_err(|e| AcceptError::InvalidMessageId(e.to_string()))?
            }
            _ => Uuid::now_v7(),
        };

        let payload: serde_json::Value = if input.payload.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&input.payload)
                .map_err(|e| AcceptError::InvalidPayload(e.to_string()))?
        };

        let headers = if input.headers.is_empty() {
            None
        } else {
            let headers_map: std::collections::HashMap<String, String> =
                input.headers.into_iter().collect();
            serde_json::to_value(headers_map).ok()
        };

        let mut message = OutboxMessage::new(
            input.aggregate_type,
            input.aggregate_id,
            input.event_type,
            payload,
            input.topic,
        );

        message.id = message_id;
        message.headers = headers;
        message.partition_key = input.partition_key;

        self.outbox
            .create(&message)
            .await
            .map_err(|e| AcceptError::Storage(e.to_string()))?;

        Ok(message_id)
    }
}
