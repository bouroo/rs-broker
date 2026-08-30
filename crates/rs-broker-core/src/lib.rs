//! rs-broker-core - Domain and application layer for rs-broker
//!
//! Feature-bounded clean architecture: each module under [`features`] owns its
//! domain entities, use cases, and ports (repository/transport interfaces).
//! This crate must stay free of infrastructure dependencies — no sqlx, kafka,
//! or tonic types. Adapters implementing the ports live outward in
//! `rs-broker-db`, `rs-broker-kafka`, and `rs-broker-server` (composition root).

/// Framework-free building blocks shared by every feature: the unified topic
/// matcher and the error type.
pub mod shared {
    pub mod error;
    pub mod topic;
}

/// Feature-bounded domain and application layer.
pub mod features;

// ---------------------------------------------------------------------------
// Compat shims: canonical paths live under `features` and `shared`; the
// pre-restructure module paths keep compiling for existing consumers.
// ---------------------------------------------------------------------------

/// Compat shim for the pre-restructure `rs_broker_core::outbox` paths.
pub mod outbox {
    pub use crate::features::publishing::{manager, publisher, retry};
    pub use crate::features::publishing::{OutboxManager, OutboxPublisher, RetryStrategy};
}

/// Compat shim for the pre-restructure `rs_broker_core::inbox` paths.
pub mod inbox {
    pub use crate::features::consuming::{dedup, dispatcher, use_cases};
    pub use crate::features::consuming::{Deduplicator, Dispatcher, InboxManager};
}

/// Compat shim for the pre-restructure `rs_broker_core::subscriber` paths.
pub mod subscriber {
    pub use crate::features::subscription::registry;
    pub use crate::features::subscription::SubscriberRegistry;
}

/// Compat shim for the pre-restructure `rs_broker_core::dlq` paths.
pub mod dlq {
    pub use crate::features::dead_letter::{handler, DlqHandler, DlqSelector, ReprocessResult};
}

/// Compat shim for the pre-restructure `rs_broker_core::grpc_client` paths.
pub mod grpc_client {
    pub use crate::features::delivery::dispatcher::{
        CircuitBreaker, CircuitBreakerConfig, CircuitBreakerState, DeliveryResult,
        SubscriberDispatcher, SubscriberEndpoint,
    };
    pub use crate::features::delivery::{channel_pool, dispatcher};
}

pub use crate::features::{
    consuming::{Dispatcher, InboxManager},
    dead_letter::{DlqHandler, DlqSelector, ReprocessResult},
    publishing::{OutboxManager, OutboxPublisher, RetryStrategy},
    subscription::SubscriberRegistry,
};
pub use crate::shared::error::{Error, Result};
pub use crate::shared::topic::{matches_any, matches_topic};

// Compat module aliases for the pre-restructure paths.
pub use crate::shared::{error, topic};
