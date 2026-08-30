//! Core error types

use thiserror::Error;

/// Core error type
#[derive(Debug, Error)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Outbox error: {0}")]
    Outbox(#[from] rs_broker_db::outbox::repository::OutboxError),

    #[error("Inbox error: {0}")]
    Inbox(#[from] rs_broker_db::inbox::repository::InboxError),

    #[error("DLQ error: {0}")]
    Dlq(#[from] rs_broker_db::dlq::repository::DlqError),

    #[error("Subscriber error: {0}")]
    Subscriber(#[from] rs_broker_db::subscriber::repository::SubscriberError),

    #[error("Kafka error: {0}")]
    Kafka(#[from] rs_broker_kafka::KafkaError),

    #[error("Configuration error: {0}")]
    Config(#[from] rs_broker_config::settings::ConfigError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Retry exhausted: {0}")]
    RetryExhausted(String),
}

/// Result type alias
pub type Result<T> = std::result::Result<T, Error>;
