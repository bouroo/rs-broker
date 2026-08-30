//! Delivery ports: transport-free contracts for notifying subscribers.
//!
//! Implemented outward by the tonic callback client in `rs-broker-server`
//! (interface adapter); the fan-out use case depends only on these types, so
//! the feature carries no proto or tonic dependency.

use async_trait::async_trait;

/// Codec-free description of a delivery to one subscriber. Mirrors the wire
/// shape without depending on the generated proto types.
#[derive(Debug, Clone, Default)]
pub struct DeliverNotification {
    /// Broker-generated message ID
    pub message_id: String,
    /// Topic the message was consumed from
    pub topic: String,
    /// Raw payload bytes
    pub payload: Vec<u8>,
    /// Header key/value pairs
    pub headers: Vec<(String, String)>,
    /// UNIX seconds
    pub timestamp: i64,
    /// Event type from the payload (may be empty)
    pub event_type: String,
    /// Retry count so far
    pub retry_count: i32,
}

/// Result of a delivered notification.
#[derive(Debug, Clone)]
pub struct NotificationOutcome {
    /// Whether the subscriber acknowledged success
    pub success: bool,
    /// Error text from the subscriber (empty if none)
    pub error: String,
    /// Whether the subscriber asks for a retry
    pub retry: bool,
    /// Suggested retry delay
    pub retry_delay_ms: i64,
}

/// Transport-level failure of a notification attempt.
#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    /// The call was rejected by the transport; carries the transport's own
    /// message text.
    #[error("{0}")]
    Transport(String),

    /// The call exceeded the per-delivery timeout.
    #[error("Request timeout")]
    Timeout,
}

/// Port: notify one subscriber endpoint about a message.
#[async_trait]
pub trait SubscriberNotifier: Send + Sync {
    /// Attempt delivery to `endpoint`. Implementations must bound the call's
    /// duration; returning [`NotificationError::Timeout`] marks the attempt
    /// timed out.
    async fn notify(
        &self,
        endpoint: &str,
        notification: DeliverNotification,
    ) -> std::result::Result<NotificationOutcome, NotificationError>;
}
