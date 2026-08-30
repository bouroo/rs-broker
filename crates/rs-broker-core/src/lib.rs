//! rs-broker-core - Core business logic for rs-broker
//!
//! This crate implements the core business logic for the message broker,
//! including outbox pattern, inbox pattern, retry logic, and subscriber management.

/// Framework-free building blocks shared by every feature: the unified topic
/// matcher and the error type. Nothing under `shared` may depend on
/// infrastructure (sqlx, kafka, tonic).
pub mod shared {
    pub mod error;
    pub mod topic;
}

pub mod dlq;
#[cfg(any(feature = "postgres", feature = "mysql"))]
pub mod grpc_client;
pub mod inbox;
pub mod outbox;
pub mod subscriber;

// Compat shims: canonical paths live under `shared`; the pre-restructure
// module paths keep compiling.
pub use crate::shared::{error, topic};

pub use crate::shared::error::{Error, Result};
pub use crate::shared::topic::{matches_any, matches_topic};
pub use dlq::{DlqHandler, DlqSelector, ReprocessResult};
pub use inbox::InboxManager;
#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use outbox::OutboxManager;
pub use subscriber::SubscriberRegistry;
