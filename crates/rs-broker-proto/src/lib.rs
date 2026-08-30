//! rs-broker-proto - Generated protobuf code for rs-broker
//!
//! This crate contains the generated Rust types from the protobuf definitions
//! in the proto directory.

// tonic service trait signatures return `Result<_, tonic::Status>` whose
// `Status` variant exceeds clippy's `result_large_err` threshold; the size is
// inherent to generated code and harmless for these gRPC traits.
#[allow(clippy::result_large_err)]
pub mod rsbroker {
    include!("rsbroker.rs");
}

pub use rsbroker::{
    rs_broker_callback_client::RsBrokerCallbackClient,
    rs_broker_callback_server::{RsBrokerCallback, RsBrokerCallbackServer},
    rs_broker_client::RsBrokerClient,
    rs_broker_server::{RsBroker, RsBrokerServer},
};
