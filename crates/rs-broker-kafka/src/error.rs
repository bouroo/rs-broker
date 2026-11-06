//! Kafka error types

use thiserror::Error;

/// Kafka error type
#[derive(Debug, Error)]
pub enum KafkaError {
    #[error("Kafka error: {0}")]
    RdKafka(#[from] rdkafka::error::KafkaError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Producer error: {0}")]
    Producer(String),

    /// The producer's local queue is full: the broker is applying
    /// backpressure, not rejecting the message.
    #[error("Producer queue full: {0}")]
    ProducerQueueFull(String),

    #[error("Consumer error: {0}")]
    Consumer(String),

    #[error("Timeout")]
    Timeout,
}

/// Result type alias
pub type Result<T> = std::result::Result<T, KafkaError>;
