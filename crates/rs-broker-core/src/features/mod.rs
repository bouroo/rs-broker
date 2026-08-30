//! Feature-bounded modules: each feature owns its domain entities, use cases,
//! and ports (repository interfaces). Features depend only on `shared` and
//! each other's public domain types — never on infrastructure. The sqlx, kafka,
//! and tonic adapters that implement these ports live outward in `rs-broker-db`,
//! `rs-broker-kafka`, and `rs-broker-server`.

pub mod consuming;
pub mod dead_letter;
pub mod delivery;
pub mod publishing;
pub mod subscription;
