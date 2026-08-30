//! Dead-letter feature: parking messages that exhausted retries and
//! reprocessing them back into the publish flow.

pub mod domain;
pub mod handler;
pub mod ports;

pub use domain::DlqMessage;
pub use handler::{DlqHandler, DlqSelector, ReprocessResult};
pub use ports::{DlqError, DlqRepository};
