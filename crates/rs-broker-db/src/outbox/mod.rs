//! Outbox module

pub mod entity;
pub mod repository;

pub use entity::{MessageStatus, OutboxMessage};
pub use repository::OutboxRepository;
