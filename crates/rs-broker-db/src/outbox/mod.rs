//! Outbox persistence adapter.
//!
//! The outbox domain lives in `rs-broker-core::features::publishing`; this
//! module implements its repository port with sqlx.

pub mod repository;

pub use repository::SqlxOutboxRepository;

pub use rs_broker_core::features::publishing::{
    MessageStatus, OutboxError, OutboxMessage, OutboxRepository,
};
