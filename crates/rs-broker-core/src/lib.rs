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
