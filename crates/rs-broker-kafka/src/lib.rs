//! rs-broker-kafka - Kafka integration for rs-broker
//!
//! This crate provides Kafka producer and consumer abstractions for the rs-broker
//! message broker.

pub mod config;
pub mod consumer;
pub mod error;
pub mod headers;
pub mod producer;

pub use config::create_consumer;
pub use config::create_consumer_config;
pub use config::create_producer;
pub use config::create_producer_config;
pub use consumer::KafkaConsumer;
pub use error::{KafkaError, Result};
pub use headers::MessageHeaders;
pub use producer::client::ProducerMessage;
pub use producer::KafkaProducer;
