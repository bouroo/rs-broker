//! Inbox persistence adapter.
//!
//! The inbox domain lives in `rs-broker-core::features::consuming`; this
//! module implements its repository port with sqlx.

pub mod repository;

pub use repository::SqlxInboxRepository;

pub use rs_broker_core::features::consuming::{
    InboxError, InboxMessage, InboxRepository, InboxStatus,
};
