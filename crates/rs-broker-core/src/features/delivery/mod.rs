//! Delivery feature: fan-out of consumed messages to registered subscribers,
//! with per-subscriber circuit breakers.
//!
//! The transport mechanics (tonic channels, proto codec) live outward:
//! `SubscriberDispatcher` today drives the callback client directly and moves
//! fully behind ports in a following step. Registry discovery flows through
//! the subscription feature's repository port.

pub mod channel_pool;
pub mod dispatcher;

pub use dispatcher::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerState, DeliveryResult,
    SubscriberDispatcher, SubscriberEndpoint,
};
