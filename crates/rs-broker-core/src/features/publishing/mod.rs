//! Publishing feature: the outbox write path.
//!
//! Domain: the outbox message and its status ladder, plus the retry policy
//! value. Ports: the outbox repository (implemented by sqlx in `rs-broker-db`)
//! and the message sink (implemented by the Kafka producer in
//! `rs-broker-kafka`). Use cases: the outbox manager and the background
//! drain-to-Kafka worker.

pub mod accept;
pub mod domain;
pub mod manager;
pub mod ports;
pub mod publisher;
pub mod retry;

pub use accept::{AcceptError, AcceptMessage, PublishRequestInput};
pub use domain::{MessageStatus, OutboxMessage};
pub use manager::OutboxManager;
pub use ports::{MessageSink, OutboundMessage, OutboxError, OutboxRepository, SinkError};
pub use publisher::OutboxPublisher;
pub use retry::RetryStrategy;
