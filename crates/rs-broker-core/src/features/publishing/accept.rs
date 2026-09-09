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
#[derive(Debug, Clone, thiserror::Error)]
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

    /// Validate each request and persist valid messages in one transaction.
    pub async fn apply_batch(
        &self,
        inputs: Vec<PublishRequestInput>,
    ) -> Vec<Result<Uuid, AcceptError>> {
        let mut messages = Vec::with_capacity(inputs.len());
        let mut results = Vec::with_capacity(inputs.len());

        for input in inputs {
            let result = Self::build(input);
            match result {
                Ok(message) => messages.push(message),
                Err(error) => results.push(Err(error)),
            }
        }

        match self.outbox.create_batch(&messages).await {
            Ok(()) => {
                results.extend(messages.iter().map(|message| Ok(message.id)));
            }
            Err(error) => {
                let error = AcceptError::Storage(error.to_string());
                results.extend(std::iter::repeat(error).take(messages.len()).map(Err));
            }
        }
        results
    }

    fn build(mut input: PublishRequestInput) -> Result<OutboxMessage, AcceptError> {
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
                std::mem::take(&mut input.headers).into_iter().collect();
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
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::publishing::ports::OutboxError;
    use crate::features::publishing::MessageStatus;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc as TestArc;
    use std::sync::Mutex;

    struct CountingOutbox {
        create_calls: AtomicUsize,
        create_batch_calls: AtomicUsize,
        messages: Mutex<Vec<OutboxMessage>>,
    }

    impl CountingOutbox {
        fn new() -> Self {
            Self {
                create_calls: AtomicUsize::new(0),
                create_batch_calls: AtomicUsize::new(0),
                messages: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl OutboxRepository for CountingOutbox {
        async fn create(&self, message: &OutboxMessage) -> Result<(), OutboxError> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            self.messages.lock().unwrap().push(message.clone());
            Ok(())
        }

        async fn create_batch(&self, messages: &[OutboxMessage]) -> Result<(), OutboxError> {
            self.create_batch_calls.fetch_add(1, Ordering::SeqCst);
            self.messages
                .lock()
                .unwrap()
                .extend(messages.iter().cloned());
            Ok(())
        }

        async fn get_by_id(&self, id: Uuid) -> Result<OutboxMessage, OutboxError> {
            self.messages
                .lock()
                .unwrap()
                .iter()
                .find(|message| message.id == id)
                .cloned()
                .ok_or(OutboxError::NotFound(id))
        }

        async fn get_pending(&self, _limit: i64) -> Result<Vec<OutboxMessage>, OutboxError> {
            Ok(Vec::new())
        }

        async fn claim_pending(
            &self,
            _limit: i64,
            _lease_secs: i32,
        ) -> Result<Vec<OutboxMessage>, OutboxError> {
            Ok(Vec::new())
        }

        async fn mark_published_batch(&self, _ids: &[Uuid]) -> Result<u64, OutboxError> {
            Ok(0)
        }

        async fn increment_retry(
            &self,
            _id: Uuid,
            _error_message: Option<String>,
        ) -> Result<i32, OutboxError> {
            Ok(0)
        }

        async fn mark_published(&self, _id: Uuid) -> Result<(), OutboxError> {
            Ok(())
        }

        async fn update_status(
            &self,
            _id: Uuid,
            _status: MessageStatus,
            _error_message: Option<String>,
        ) -> Result<(), OutboxError> {
            Ok(())
        }

        async fn delete(&self, _id: Uuid) -> Result<(), OutboxError> {
            Ok(())
        }
    }

    fn input(message_id: Option<String>) -> PublishRequestInput {
        PublishRequestInput {
            message_id,
            aggregate_type: String::from("order"),
            aggregate_id: String::from("1"),
            event_type: String::from("created"),
            payload: Vec::new(),
            headers: Vec::new(),
            topic: String::from("orders"),
            partition_key: Some("key".into()),
        }
    }

    #[tokio::test]
    async fn batch_uses_one_create_batch_and_preserves_invalid_entries() {
        let repository = TestArc::new(CountingOutbox::new());
        let use_case = AcceptMessage::new(repository.clone());
        let inputs = vec![input(None), input(None)];

        let results = use_case.apply_batch(inputs).await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
        assert_ne!(results[0].as_ref().ok(), results[1].as_ref().ok());
        assert_eq!(repository.create_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repository.create_batch_calls.load(Ordering::SeqCst), 1);
        assert_eq!(repository.messages.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn batch_preserves_explicit_message_ids_and_headers() {
        let message_id = Uuid::now_v7();
        let repository = TestArc::new(CountingOutbox::new());
        let use_case = AcceptMessage::new(repository.clone());

        let mut request = input(None);
        request.headers = vec![(String::from("trace"), String::from("1"))];
        let results = use_case
            .apply_batch(vec![input(Some(message_id.to_string())), request])
            .await;

        assert_eq!(results[0].as_ref().unwrap(), &message_id);
        assert_eq!(
            repository.messages.lock().unwrap()[1].headers,
            Some(serde_json::json!({"trace": "1"}))
        );
    }
}
