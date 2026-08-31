//! Serde DTOs mirroring the proto message shapes for the REST API.
//!
//! prost-generated types do not implement serde, so the HTTP layer converts
//! between these DTOs and the proto types with `From`/`Into`. Field names
//! match the proto fields (snake_case). Payload bytes follow the broker's
//! JSON-payload contract: JSON bodies carry `payload` as a JSON value;
//! non-JSON bytes fall back to `payload_base64`.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use rs_broker_proto::rsbroker as proto;

// ---------------------------------------------------------------------------
// Shared value types
// ---------------------------------------------------------------------------

/// Header key-value pair (`proto::Header`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub key: String,
    pub value: String,
}

impl From<proto::Header> for Header {
    fn from(h: proto::Header) -> Self {
        Self {
            key: h.key,
            value: h.value,
        }
    }
}

impl From<Header> for proto::Header {
    fn from(h: Header) -> Self {
        Self {
            key: h.key,
            value: h.value,
        }
    }
}

/// Retry configuration (`proto::RetryConfig`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: i32,
    pub initial_delay_ms: i64,
    pub multiplier: f64,
    pub max_delay_ms: i64,
    pub enable_dlq: bool,
    pub dlq_topic: String,
}

impl From<proto::RetryConfig> for RetryConfig {
    fn from(c: proto::RetryConfig) -> Self {
        Self {
            max_retries: c.max_retries,
            initial_delay_ms: c.initial_delay_ms,
            multiplier: c.multiplier,
            max_delay_ms: c.max_delay_ms,
            enable_dlq: c.enable_dlq,
            dlq_topic: c.dlq_topic,
        }
    }
}

impl From<RetryConfig> for proto::RetryConfig {
    fn from(c: RetryConfig) -> Self {
        Self {
            max_retries: c.max_retries,
            initial_delay_ms: c.initial_delay_ms,
            multiplier: c.multiplier,
            max_delay_ms: c.max_delay_ms,
            enable_dlq: c.enable_dlq,
            dlq_topic: c.dlq_topic,
        }
    }
}

/// Delivery configuration (`proto::DeliveryConfig`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveryConfig {
    pub timeout_ms: i64,
    pub max_concurrent: i32,
    pub retry_config: Option<RetryConfig>,
    pub require_ack: bool,
}

impl From<proto::DeliveryConfig> for DeliveryConfig {
    fn from(c: proto::DeliveryConfig) -> Self {
        Self {
            timeout_ms: c.timeout_ms,
            max_concurrent: c.max_concurrent,
            retry_config: c.retry_config.map(Into::into),
            require_ack: c.require_ack,
        }
    }
}

impl From<DeliveryConfig> for proto::DeliveryConfig {
    fn from(c: DeliveryConfig) -> Self {
        Self {
            timeout_ms: c.timeout_ms,
            max_concurrent: c.max_concurrent,
            retry_config: c.retry_config.map(Into::into),
            require_ack: c.require_ack,
        }
    }
}

/// Request metadata (`proto::RequestMetadata`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestMetadata {
    pub correlation_id: String,
    pub source_service: String,
    pub timestamp: i64,
    pub context: std::collections::HashMap<String, String>,
}

impl From<proto::RequestMetadata> for RequestMetadata {
    fn from(m: proto::RequestMetadata) -> Self {
        Self {
            correlation_id: m.correlation_id,
            source_service: m.source_service,
            timestamp: m.timestamp,
            context: m.context,
        }
    }
}

impl From<RequestMetadata> for proto::RequestMetadata {
    fn from(m: RequestMetadata) -> Self {
        Self {
            correlation_id: m.correlation_id,
            source_service: m.source_service,
            timestamp: m.timestamp,
            context: m.context,
        }
    }
}

// ---------------------------------------------------------------------------
// Payload encoding helpers
// ---------------------------------------------------------------------------

/// Decode the request payload: exactly one of `payload` (JSON value) or
/// `payload_base64` must be present.
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    /// JSON value, serialized verbatim to bytes.
    Json(serde_json::Value),
    /// Raw bytes decoded from base64.
    Base64(Vec<u8>),
}

impl Payload {
    /// Build from the optional DTO fields; `None` when both are absent.
    pub fn from_request_parts(
        payload: Option<serde_json::Value>,
        payload_base64: Option<String>,
    ) -> Result<Option<Self>, String> {
        match (payload, payload_base64) {
            (Some(_), Some(_)) => {
                Err("only one of 'payload' and 'payload_base64' may be set".to_string())
            }
            (Some(v), None) => Ok(Some(Payload::Json(v))),
            (None, Some(b64)) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| format!("invalid base64 in payload_base64: {}", e))?;
                Ok(Some(Payload::Base64(bytes)))
            }
            (None, None) => Ok(None),
        }
    }

    /// Encode bytes for a response: JSON when the bytes parse as JSON,
    /// base64 otherwise.
    pub fn to_response_value(bytes: &[u8]) -> ResponsePayload {
        if bytes.is_empty() {
            return ResponsePayload::Json(serde_json::Value::Null);
        }
        match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(v) => ResponsePayload::Json(v),
            Err(_) => {
                ResponsePayload::Base64(base64::engine::general_purpose::STANDARD.encode(bytes))
            }
        }
    }
}

/// Response-side payload encoding (no "both set" ambiguity on output).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ResponsePayload {
    Json(serde_json::Value),
    Base64(String),
}

// ---------------------------------------------------------------------------
// Publish
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/publish`.
#[derive(Debug, Clone, Deserialize)]
pub struct PublishRequestDto {
    pub message_id: Option<String>,
    pub aggregate_type: Option<String>,
    pub aggregate_id: Option<String>,
    pub event_type: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub payload_base64: Option<String>,
    pub headers: Option<Vec<Header>>,
    /// Optional for parity with proto3 defaults: an empty topic is accepted
    /// by the core use case exactly as over gRPC.
    pub topic: Option<String>,
    pub partition_key: Option<String>,
    pub retry_config: Option<RetryConfig>,
    pub metadata: Option<RequestMetadata>,
}

impl PublishRequestDto {
    /// Convert to the proto request. Defaults follow the gRPC contract:
    /// empty-string scalars where the proto uses default values, generated
    /// idempotency key semantics unchanged.
    pub fn into_proto(self) -> Result<proto::PublishRequest, String> {
        let payload_bytes = match Payload::from_request_parts(self.payload, self.payload_base64)? {
            Some(Payload::Json(v)) => {
                serde_json::to_vec(&v).map_err(|e| format!("failed to serialize payload: {}", e))?
            }
            Some(Payload::Base64(bytes)) => bytes,
            None => Vec::new(),
        };

        Ok(proto::PublishRequest {
            message_id: self.message_id.unwrap_or_default(),
            aggregate_type: self.aggregate_type.unwrap_or_default(),
            aggregate_id: self.aggregate_id.unwrap_or_default(),
            event_type: self.event_type.unwrap_or_default(),
            payload: payload_bytes,
            headers: self
                .headers
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            topic: self.topic.unwrap_or_default(),
            partition_key: self.partition_key.unwrap_or_default(),
            retry_config: self.retry_config.map(Into::into),
            metadata: self.metadata.map(Into::into),
        })
    }
}

/// Response body for publish endpoints.
#[derive(Debug, Serialize)]
pub struct PublishResponseDto {
    pub message_id: String,
    pub status: String,
    pub duplicate: bool,
    pub accepted_at: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// Message status as a string (`proto::MessageStatus`).
pub fn message_status_to_str(status: i32) -> String {
    match proto::MessageStatus::try_from(status) {
        Ok(proto::MessageStatus::Pending) => "pending",
        Ok(proto::MessageStatus::Publishing) => "publishing",
        Ok(proto::MessageStatus::Published) => "published",
        Ok(proto::MessageStatus::Retrying) => "retrying",
        Ok(proto::MessageStatus::Failed) => "failed",
        Ok(proto::MessageStatus::Dlq) => "dlq",
        Ok(proto::MessageStatus::Received) => "received",
        Ok(proto::MessageStatus::Processing) => "processing",
        Ok(proto::MessageStatus::Processed) => "processed",
        Ok(proto::MessageStatus::Delivered) => "delivered",
        _ => "unspecified",
    }
    .to_string()
}

impl From<proto::PublishResponse> for PublishResponseDto {
    fn from(r: proto::PublishResponse) -> Self {
        Self {
            message_id: r.message_id,
            status: message_status_to_str(r.status),
            duplicate: r.duplicate,
            accepted_at: r.accepted_at,
            error: r.error,
        }
    }
}

/// Request body for `POST /api/v1/publish/batch`.
#[derive(Debug, Deserialize)]
pub struct PublishBatchRequestDto {
    pub messages: Vec<PublishRequestDto>,
    pub metadata: Option<RequestMetadata>,
}

/// Response body for `POST /api/v1/publish/batch`.
#[derive(Debug, Serialize)]
pub struct PublishBatchResponseDto {
    pub responses: Vec<PublishResponseDto>,
    pub success_count: i32,
    pub failure_count: i32,
}

impl From<proto::PublishBatchResponse> for PublishBatchResponseDto {
    fn from(r: proto::PublishBatchResponse) -> Self {
        Self {
            responses: r.responses.into_iter().map(Into::into).collect(),
            success_count: r.success_count,
            failure_count: r.failure_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Message status / cancel
// ---------------------------------------------------------------------------

/// Response body for `GET /api/v1/messages/{id}`.
#[derive(Debug, Serialize)]
pub struct GetMessageStatusResponseDto {
    pub message_id: String,
    pub status: String,
    pub retry_count: i32,
    pub last_updated: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_message: String,
    pub published_at: i64,
    pub topic: String,
}

impl From<proto::GetMessageStatusResponse> for GetMessageStatusResponseDto {
    fn from(r: proto::GetMessageStatusResponse) -> Self {
        Self {
            message_id: r.message_id,
            status: message_status_to_str(r.status),
            retry_count: r.retry_count,
            last_updated: r.last_updated,
            error_message: r.error_message,
            published_at: r.published_at,
            topic: r.topic,
        }
    }
}

/// Response body for `DELETE /api/v1/messages/{id}`.
#[derive(Debug, Serialize)]
pub struct CancelMessageResponseDto {
    pub success: bool,
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

impl From<proto::CancelMessageResponse> for CancelMessageResponseDto {
    fn from(r: proto::CancelMessageResponse) -> Self {
        Self {
            success: r.success,
            status: message_status_to_str(r.status),
            error: r.error,
        }
    }
}

// ---------------------------------------------------------------------------
// Subscribers
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/subscribers`.
#[derive(Debug, Deserialize)]
pub struct RegisterSubscriberRequestDto {
    pub service_name: String,
    pub grpc_endpoint: String,
    pub topic_patterns: Vec<String>,
    pub delivery_config: Option<DeliveryConfig>,
    pub metadata: Option<RequestMetadata>,
}

/// Response body for register/unregister/update subscriber endpoints.
#[derive(Debug, Serialize)]
pub struct SubscriberMutationResponseDto {
    pub subscriber_id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

impl From<proto::RegisterSubscriberResponse> for SubscriberMutationResponseDto {
    fn from(r: proto::RegisterSubscriberResponse) -> Self {
        Self {
            subscriber_id: r.subscriber_id,
            success: r.success,
            error: r.error,
        }
    }
}

impl From<proto::UpdateSubscriberResponse> for SubscriberMutationResponseDto {
    fn from(r: proto::UpdateSubscriberResponse) -> Self {
        Self {
            subscriber_id: String::new(),
            success: r.success,
            error: r.error,
        }
    }
}

impl From<proto::UnregisterSubscriberResponse> for SubscriberMutationResponseDto {
    fn from(r: proto::UnregisterSubscriberResponse) -> Self {
        Self {
            subscriber_id: String::new(),
            success: r.success,
            error: r.error,
        }
    }
}

/// Query/body for `PATCH /api/v1/subscribers/{id}`.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateSubscriberRequestDto {
    pub grpc_endpoint: Option<String>,
    pub topic_patterns: Option<Vec<String>>,
    pub active: Option<bool>,
    pub delivery_config: Option<DeliveryConfig>,
    pub metadata: Option<RequestMetadata>,
}

/// Response body for `GET /api/v1/subscribers`.
#[derive(Debug, Serialize)]
pub struct ListSubscribersResponseDto {
    pub subscribers: Vec<SubscriberInfoDto>,
}

/// One registered subscriber (`proto::SubscriberInfo`).
#[derive(Debug, Serialize)]
pub struct SubscriberInfoDto {
    pub subscriber_id: String,
    pub service_name: String,
    pub grpc_endpoint: String,
    pub topic_patterns: Vec<String>,
    pub active: bool,
    pub registered_at: i64,
}

impl From<proto::SubscriberInfo> for SubscriberInfoDto {
    fn from(s: proto::SubscriberInfo) -> Self {
        Self {
            subscriber_id: s.subscriber_id,
            service_name: s.service_name,
            grpc_endpoint: s.grpc_endpoint,
            topic_patterns: s.topic_patterns,
            active: s.active,
            registered_at: s.registered_at,
        }
    }
}

impl From<proto::ListSubscribersResponse> for ListSubscribersResponseDto {
    fn from(r: proto::ListSubscribersResponse) -> Self {
        Self {
            subscribers: r.subscribers.into_iter().map(Into::into).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Response body for `GET /api/v1/health`.
#[derive(Debug, Serialize)]
pub struct HealthResponseDto {
    pub status: String,
    pub components: Vec<ComponentHealthDto>,
    pub metrics: Option<BrokerMetricsDto>,
}

/// Component health (`proto::ComponentHealth`).
#[derive(Debug, Serialize)]
pub struct ComponentHealthDto {
    pub name: String,
    pub status: String,
    pub message: String,
}

/// Broker metrics (`proto::BrokerMetrics`).
#[derive(Debug, Serialize)]
pub struct BrokerMetricsDto {
    pub outbox_pending: i64,
    pub inbox_pending: i64,
    pub published_today: i64,
    pub processed_today: i64,
    pub dlq_count: i64,
    pub active_subscribers: i64,
    pub kafka_connected: bool,
    pub database_connected: bool,
}

fn health_status_to_str(status: i32) -> String {
    match proto::HealthStatus::try_from(status) {
        Ok(proto::HealthStatus::Healthy) => "healthy",
        Ok(proto::HealthStatus::Degraded) => "degraded",
        Ok(proto::HealthStatus::Unhealthy) => "unhealthy",
        _ => "unspecified",
    }
    .to_string()
}

impl From<proto::HealthResponse> for HealthResponseDto {
    fn from(h: proto::HealthResponse) -> Self {
        Self {
            status: health_status_to_str(h.status),
            components: h
                .components
                .into_iter()
                .map(|c| ComponentHealthDto {
                    name: c.name,
                    status: health_status_to_str(c.status),
                    message: c.message,
                })
                .collect(),
            metrics: h.metrics.map(|m| BrokerMetricsDto {
                outbox_pending: m.outbox_pending,
                inbox_pending: m.inbox_pending,
                published_today: m.published_today,
                processed_today: m.processed_today,
                dlq_count: m.dlq_count,
                active_subscribers: m.active_subscribers,
                kafka_connected: m.kafka_connected,
                database_connected: m.database_connected,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// DLQ
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/dlq/reprocess`.
#[derive(Debug, Deserialize)]
pub struct ReprocessDlqRequestDto {
    pub message_id: Option<String>,
    pub topic: Option<String>,
    pub all: Option<bool>,
}

impl ReprocessDlqRequestDto {
    pub fn into_proto(self) -> proto::ReprocessDlqRequest {
        proto::ReprocessDlqRequest {
            message_id: self.message_id.unwrap_or_default(),
            topic: self.topic.unwrap_or_default(),
            all: self.all.unwrap_or(false),
            metadata: None,
        }
    }
}

/// Response body for `POST /api/v1/dlq/reprocess`.
#[derive(Debug, Serialize)]
pub struct ReprocessDlqResponseDto {
    pub reprocessed_count: i32,
    pub failure_count: i32,
    pub errors: Vec<String>,
}

impl From<proto::ReprocessDlqResponse> for ReprocessDlqResponseDto {
    fn from(r: proto::ReprocessDlqResponse) -> Self {
        Self {
            reprocessed_count: r.reprocessed_count,
            failure_count: r.failure_count,
            errors: r.errors,
        }
    }
}

/// Query params for `GET /api/v1/dlq`.
#[derive(Debug, Deserialize)]
pub struct ListDlqQuery {
    pub topic: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// One DLQ message (`proto::DlqMessageInfo`).
#[derive(Debug, Serialize)]
pub struct DlqMessageInfoDto {
    pub message_id: String,
    pub original_topic: String,
    pub dlq_topic: String,
    pub failure_reason: String,
    pub retry_count: i32,
    pub created_at: i64,
}

impl From<proto::DlqMessageInfo> for DlqMessageInfoDto {
    fn from(m: proto::DlqMessageInfo) -> Self {
        Self {
            message_id: m.message_id,
            original_topic: m.original_topic,
            dlq_topic: m.dlq_topic,
            failure_reason: m.failure_reason,
            retry_count: m.retry_count,
            created_at: m.created_at,
        }
    }
}

/// Response body for `GET /api/v1/dlq`.
#[derive(Debug, Serialize)]
pub struct ListDlqMessagesResponseDto {
    pub messages: Vec<DlqMessageInfoDto>,
    pub total_count: i32,
}

impl From<proto::ListDlqMessagesResponse> for ListDlqMessagesResponseDto {
    fn from(r: proto::ListDlqMessagesResponse) -> Self {
        Self {
            messages: r.messages.into_iter().map(Into::into).collect(),
            total_count: r.total_count,
        }
    }
}

// ---------------------------------------------------------------------------
// SSE event
// ---------------------------------------------------------------------------

/// JSON shape of an SSE `deliver` event (`proto::DeliverEvent`).
#[derive(Debug, Serialize)]
pub struct DeliverEventDto {
    pub message_id: String,
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub key: String,
    pub payload: ResponsePayload,
    pub headers: Vec<Header>,
    pub timestamp: i64,
    pub event_type: String,
}

impl From<proto::DeliverEvent> for DeliverEventDto {
    fn from(e: proto::DeliverEvent) -> Self {
        Self {
            message_id: e.message_id,
            topic: e.topic,
            partition: e.partition,
            offset: e.offset,
            key: e.key,
            payload: Payload::to_response_value(&e.payload),
            headers: e.headers.into_iter().map(Into::into).collect(),
            timestamp: e.timestamp,
            event_type: e.event_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_request_dto_minimal() {
        let dto: PublishRequestDto =
            serde_json::from_str(r#"{"topic": "orders.created", "payload": {"id": 7}}"#).unwrap();
        let proto_req = dto.into_proto().unwrap();
        assert_eq!(proto_req.topic, "orders.created");
        assert_eq!(proto_req.message_id, "");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&proto_req.payload).unwrap()["id"],
            7
        );
    }

    /// Parity: an omitted topic mirrors proto3's empty-string default.
    #[test]
    fn publish_request_topic_optional() {
        let dto: PublishRequestDto = serde_json::from_str(r#"{"payload": {"id": 7}}"#).unwrap();
        let proto_req = dto.into_proto().unwrap();
        assert_eq!(proto_req.topic, "");
    }

    #[test]
    fn publish_request_payload_and_base64_mutually_exclusive() {
        let dto: PublishRequestDto = serde_json::from_str(
            r#"{"topic": "t", "payload": {"a": 1}, "payload_base64": "aGk="}"#,
        )
        .unwrap();
        assert!(dto.into_proto().is_err());
    }

    #[test]
    fn publish_request_base64_roundtrip() {
        let dto: PublishRequestDto = serde_json::from_str(
            r#"{"topic": "t", "payload_base64": "aGk="}"#, // "hi"
        )
        .unwrap();
        let proto_req = dto.into_proto().unwrap();
        assert_eq!(proto_req.payload, b"hi");
    }

    #[test]
    fn publish_response_json_payload_passes_through() {
        let bytes = br#"{"hello":"world"}"#;
        match Payload::to_response_value(bytes) {
            ResponsePayload::Json(v) => assert_eq!(v["hello"], "world"),
            other => panic!("expected JSON payload, got {:?}", other),
        }
    }

    #[test]
    fn publish_response_non_json_payload_falls_back_to_base64() {
        match Payload::to_response_value(&[0xff, 0xfe, 0x00]) {
            ResponsePayload::Base64(b64) => {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(&b64)
                    .unwrap();
                assert_eq!(decoded, vec![0xff, 0xfe, 0x00]);
            }
            other => panic!("expected base64 payload, got {:?}", other),
        }
    }

    #[test]
    fn message_status_strings() {
        assert_eq!(
            message_status_to_str(proto::MessageStatus::Pending as i32),
            "pending"
        );
        assert_eq!(
            message_status_to_str(proto::MessageStatus::Published as i32),
            "published"
        );
        assert_eq!(
            message_status_to_str(proto::MessageStatus::Dlq as i32),
            "dlq"
        );
        assert_eq!(message_status_to_str(999), "unspecified");
    }

    #[test]
    fn dlq_query_defaults() {
        let q: ListDlqQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(q.topic, None);
        assert_eq!(q.limit, None);
        assert_eq!(q.offset, None);
    }

    #[test]
    fn deliver_event_dto_serializes_json_payload() {
        let event = proto::DeliverEvent {
            message_id: "m1".to_string(),
            topic: "orders".to_string(),
            partition: 0,
            offset: 42,
            key: "k".to_string(),
            payload: br#"{"x":1}"#.to_vec(),
            headers: vec![proto::Header {
                key: "h".to_string(),
                value: "v".to_string(),
            }],
            timestamp: 1234,
            event_type: "test".to_string(),
        };
        let dto: DeliverEventDto = event.into();
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["topic"], "orders");
        assert_eq!(json["payload"]["x"], 1);
        assert_eq!(json["headers"][0]["key"], "h");
    }
}
