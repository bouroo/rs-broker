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

    #[error("Consumer error: {0}")]
    Consumer(String),

    #[error("Timeout")]
    Timeout,
}

/// Result type alias
pub type Result<T> = std::result::Result<T, KafkaError>;
