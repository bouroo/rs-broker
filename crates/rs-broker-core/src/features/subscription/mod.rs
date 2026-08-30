//! Subscription feature: registering subscribers and matching topics.

pub mod domain;
pub mod ports;
pub mod registry;

pub use domain::Subscriber;
pub use ports::{SubscriberError, SubscriberRepository};
pub use registry::SubscriberRegistry;
