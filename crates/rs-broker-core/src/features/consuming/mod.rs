//! Consuming feature: the inbox ingest path for messages received from Kafka.

pub mod dedup;
pub mod dispatcher;
pub mod domain;
pub mod ports;
pub mod use_cases;

pub use dedup::Deduplicator;
pub use dispatcher::Dispatcher;
pub use domain::{InboxMessage, InboxStatus};
pub use ports::{InboxError, InboxRepository};
pub use use_cases::InboxManager;
