//! rs-broker-core - Core business logic for rs-broker
//!
//! This crate implements the core business logic for the message broker,
//! including outbox pattern, inbox pattern, retry logic, and subscriber management.

pub mod dlq;
pub mod error;
pub mod inbox;
pub mod outbox;
pub mod subscriber;

pub use dlq::DlqHandler;
pub use error::{Error, Result};
pub use inbox::InboxManager;
#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use outbox::OutboxManager;
pub use subscriber::SubscriberRegistry;
