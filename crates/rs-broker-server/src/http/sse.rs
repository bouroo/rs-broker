//! SSE handler: `GET /api/v1/events/stream` — the REST equivalent of the
//! gRPC `SubscribeEvents` server-stream. Subscribes to the same broadcast
//! channel the gRPC stream uses and applies the same unified topic matcher
//! (`rs_broker_core::matches_any`), so both transports agree on delivery.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::StreamExt as _;
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;

use crate::grpc::service::RsBrokerService;

/// Query parameters for the SSE endpoint.
#[derive(Debug, Deserialize)]
pub struct SseQuery {
    /// Required: identifies the consumer (mirrors the gRPC validation).
    pub subscriber_id: String,
    /// Comma-separated topic patterns; MQTT-style wildcards (`*` exactly one
    /// segment, `#` zero or more). An empty value matches nothing.
    pub patterns: Option<String>,
    /// Accepted for parity with the gRPC request but ignored: the stream is a
    /// live broadcast from subscription time onward (same as gRPC).
    #[allow(dead_code)]
    pub position: Option<String>,
}

impl SseQuery {
    /// Parse the comma-separated pattern list.
    pub fn pattern_list(&self) -> Vec<String> {
        self.patterns
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// SSE stream of `DeliverEvent`s for one subscriber.
///
/// Event protocol:
/// - `event: deliver` with `data: <DeliverEvent JSON>` for each matching event
/// - `event: lagged` with `data: {"missed": N}` when the client falls behind
///   the broadcast buffer (the gRPC path logs and drops instead)
/// - keep-alive comment pings every 15s (`KeepAlive::default()`)
pub async fn events_stream(
    State(state): State<Arc<RsBrokerService>>,
    Query(query): Query<SseQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, crate::http::error::ApiError> {
    let receiver = state.subscribe_events_inner(&query.subscriber_id)?;
    let patterns = query.pattern_list();

    let sse_stream = BroadcastStream::new(receiver).filter_map(move |result| {
        let patterns = patterns.clone();
        async move {
            match result {
                Ok(event) => {
                    if rs_broker_core::matches_any(&event.topic, &patterns) {
                        let dto = crate::http::dto::DeliverEventDto::from(event);
                        match serde_json::to_string(&dto) {
                            Ok(json) => Some(Ok(Event::default().event("deliver").data(json))),
                            Err(e) => {
                                tracing::error!("failed to serialize DeliverEvent: {}", e);
                                None
                            }
                        }
                    } else {
                        None
                    }
                }
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "sse subscriber lagged behind broadcast");
                    Some(Ok(Event::default()
                        .event("lagged")
                        .data(format!("{{\"missed\": {}}}", skipped))))
                }
            }
        }
    });

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()))
}
