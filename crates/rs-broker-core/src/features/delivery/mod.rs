//! Delivery feature: fan-out of consumed messages to registered subscribers,
//! with per-subscriber circuit breakers.
//!
//! The transport mechanics (tonic channels, proto codec) live outward:
//! `delivery::ports::SubscriberNotifier` is implemented by the callback
//! client adapter in `rs-broker-server`. Registry discovery flows through the
//! subscription feature's repository port.

pub mod dispatcher;
pub mod ports;

pub use dispatcher::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerState, DeliveryResult,
    SubscriberDispatcher, SubscriberEndpoint,
};
pub use ports::{DeliverNotification, NotificationError, NotificationOutcome, SubscriberNotifier};
