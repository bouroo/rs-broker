//! rs-broker-db - Database layer for rs-broker
//!
//! This crate provides the database layer with connection pooling and repository
//! implementations for outbox, inbox, subscriber, and DLQ entities.

// Ensure exactly one database backend is enabled
#[cfg(all(feature = "postgres", feature = "mysql"))]
compile_error!("Cannot enable both 'postgres' and 'mysql' features simultaneously");

#[cfg(not(any(feature = "postgres", feature = "mysql")))]
compile_error!("Either 'postgres' or 'mysql' feature must be enabled");

pub mod dlq;
pub mod inbox;
pub mod migration;
pub mod outbox;
pub mod pool;
pub mod subscriber;

// Re-export common types based on enabled feature
#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use pool::{create_pool, DbPool};

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use dlq::repository::{DlqError, SqlxDlqRepository};

pub use dlq::{DlqMessage, DlqRepository};

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use inbox::repository::{InboxError, SqlxInboxRepository};

pub use inbox::{InboxMessage, InboxRepository};

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use migration::run_migrations;

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use outbox::repository::{OutboxError, SqlxOutboxRepository};

pub use outbox::{MessageStatus, OutboxMessage, OutboxRepository};

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use subscriber::repository::{SqlxSubscriberRepository, SubscriberError};

pub use subscriber::{Subscriber, SubscriberRepository};
