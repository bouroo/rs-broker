//! HTTP REST + SSE transport: full parity with the gRPC surface, sharing the
//! same `RsBrokerService` state and the same transport-free `*_inner` methods.
//!
//! Mounted at `/api/v1` on the existing HTTP port by `crate::app`.

pub mod dto;
pub mod error;
pub mod routes;
pub mod sse;

use std::sync::Arc;

use axum::routing::{get, patch, post};
use axum::Router;

use crate::grpc::service::RsBrokerService;

/// Build the `/api/v1` router over the shared service state.
pub fn router(state: Arc<RsBrokerService>) -> Router {
    Router::new()
        .route("/publish", post(routes::publish))
        .route("/publish/batch", post(routes::publish_batch))
        .route(
            "/messages/:id",
            get(routes::get_message_status).delete(routes::cancel_message),
        )
        .route(
            "/subscribers",
            post(routes::register_subscriber).get(routes::list_subscribers),
        )
        .route(
            "/subscribers/:id",
            patch(routes::update_subscriber).delete(routes::unregister_subscriber),
        )
        .route("/events/stream", get(sse::events_stream))
        .route("/health", get(routes::get_health))
        .route("/dlq/reprocess", post(routes::reprocess_dlq))
        .route("/dlq", get(routes::list_dlq_messages))
        .with_state(state)
}
