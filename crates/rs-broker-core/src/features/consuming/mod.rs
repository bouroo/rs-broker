//! Consuming feature: the inbox ingest path for messages received from Kafka.

pub mod dedup;
pub mod dispatcher;
pub mod domain;
pub mod manager;
pub mod ports;

pub use dedup::Deduplicator;
pub use dispatcher::Dispatcher;
pub use domain::{InboxMessage, InboxStatus};
pub use manager::InboxManager;
pub use ports::{InboxError, InboxRepository};
