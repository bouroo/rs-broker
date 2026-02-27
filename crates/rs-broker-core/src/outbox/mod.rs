//! Outbox module

pub mod manager;
pub mod publisher;
pub mod retry;

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub use manager::OutboxManager;
pub use publisher::OutboxPublisher;
pub use retry::RetryStrategy;
