//! DLQ persistence adapter.
//!
//! The DLQ domain lives in `rs-broker-core::features::dead_letter`; this
//! module implements its repository port with sqlx.

pub mod repository;

pub use repository::SqlxDlqRepository;

pub use rs_broker_core::features::dead_letter::{DlqError, DlqMessage, DlqRepository};
