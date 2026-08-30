//! Subscriber persistence adapter.
//!
//! The subscriber domain lives in `rs-broker-core::features::subscription`;
//! this module implements its repository port with sqlx.

pub mod repository;

pub use repository::SqlxSubscriberRepository;

pub use rs_broker_core::features::subscription::{
    Subscriber, SubscriberError, SubscriberRepository,
};
