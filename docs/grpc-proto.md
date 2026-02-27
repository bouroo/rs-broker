# rs-broker gRPC Proto Definition

This document describes the gRPC service interface for rs-broker. The actual proto file will be located at `proto/rs_broker.proto`.

## Service Overview

The rs-broker gRPC service provides a unified interface for both producer (outbox) and consumer (inbox) operations. It abstracts Kafka complexity from clients while providing reliability features like automatic retries and dead-letter queues.

## Service Definition

```protobuf
syntax = "proto3";

package rsbroker;

option go_package = "github.com/bouroo/rs-broker/proto/rsbroker";
option java_multiple_files = true;
option java_package = "com.bouroo.rsbroker";
```

## Main Service: RsBroker

### PRODUCER MODE - Outbox Pattern Operations

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `Publish` | Publish a single message to outbox | PublishRequest | PublishResponse |
| `PublishBatch` | Publish multiple messages atomically | PublishBatchRequest | PublishBatchResponse |
| `GetMessageStatus` | Query status of published message | GetMessageStatusRequest | GetMessageStatusResponse |
| `CancelMessage` | Cancel a pending message | CancelMessageRequest | CancelMessageResponse |

### CONSUMER MODE - Inbox Pattern Operations

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `RegisterSubscriber` | Register a subscriber service | RegisterSubscriberRequest | RegisterSubscriberResponse |
| `UnregisterSubscriber` | Unregister a subscriber | UnregisterSubscriberRequest | UnregisterSubscriberResponse |
| `UpdateSubscriber` | Update subscriber configuration | UpdateSubscriberRequest | UpdateSubscriberResponse |
| `ListSubscribers` | List all registered subscribers | ListSubscribersRequest | ListSubscribersResponse |

### STREAMING OPERATIONS

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `SubscribeEvents` | Stream for real-time message delivery | SubscribeEventsRequest | stream DeliverEvent |
| `StreamPublish` | Stream for publishing messages | stream PublishRequest | stream PublishResponse |

### ADMIN OPERATIONS

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `GetHealth` | Get broker health and metrics | HealthRequest | HealthResponse |
| `ReprocessDlq` | Reprocess messages from DLQ | ReprocessDlqRequest | ReprocessDlqResponse |
| `ListDlqMessages` | List DLQ messages for monitoring | ListDlqMessagesRequest | ListDlqMessagesResponse |

## Message Types

### Common Types

#### Header
```protobuf
message Header {
    string key = 1;
    string value = 2;
}
```

#### MessageStatus
```protobuf
enum MessageStatus {
    MESSAGE_STATUS_UNSPECIFIED = 0;
    MESSAGE_STATUS_PENDING = 1;
    MESSAGE_STATUS_PUBLISHING = 2;
    MESSAGE_STATUS_PUBLISHED = 3;
    MESSAGE_STATUS_RETRYING = 4;
    MESSAGE_STATUS_FAILED = 5;
    MESSAGE_STATUS_DLQ = 6;
    MESSAGE_STATUS_RECEIVED = 7;
    MESSAGE_STATUS_PROCESSING = 8;
    MESSAGE_STATUS_PROCESSED = 9;
    MESSAGE_STATUS_DELIVERED = 10;
}
```

#### DeliveryStatus
```protobuf
enum DeliveryStatus {
    DELIVERY_STATUS_UNSPECIFIED = 0;
    DELIVERY_STATUS_PENDING = 1;
    DELIVERY_STATUS_DELIVERING = 2;
    DELIVERY_STATUS_DELIVERED = 3;
    DELIVERY_STATUS_RETRYING = 4;
    DELIVERY_STATUS_FAILED = 5;
}
```

### Producer Messages

#### PublishRequest
```protobuf
message PublishRequest {
    string message_id = 1;              // Unique identifier for idempotency
    string aggregate_type = 2;          // Type of aggregate/entity
    string aggregate_id = 3;            // Unique identifier of the aggregate
    string event_type = 4;              // Type/name of the event
    bytes payload = 5;                  // JSON-encoded message payload
    repeated Header headers = 6;        // Optional headers for Kafka message
    string topic = 7;                   // Target Kafka topic
    string partition_key = 8;           // Optional partition key
    RetryConfig retry_config = 9;       // Override default retry configuration
    RequestMetadata metadata = 10;      // Request metadata
}
```

#### PublishResponse
```protobuf
message PublishResponse {
    string message_id = 1;              // Unique message identifier
    MessageStatus status = 2;           // Current status of the message
    bool duplicate = 3;                 // Whether this was a duplicate request
    int64 accepted_at = 4;              // Timestamp when message was accepted
    string error = 5;                   // Error message if request failed
}
```

#### PublishBatchRequest
```protobuf
message PublishBatchRequest {
    repeated PublishRequest messages = 1;
    RequestMetadata metadata = 2;
}
```

#### PublishBatchResponse
```protobuf
message PublishBatchResponse {
    repeated PublishResponse responses = 1;
    int32 success_count = 2;
    int32 failure_count = 3;
}
```

### Consumer Messages

#### RegisterSubscriberRequest
```protobuf
message RegisterSubscriberRequest {
    string subscriber_id = 1;           // Unique identifier for this subscriber
    string service_name = 2;            // Human-readable service name
    string grpc_endpoint = 3;           // gRPC endpoint for message delivery
    repeated string topic_patterns = 4; // Topic patterns with wildcards
    DeliveryConfig delivery_config = 5; // Optional delivery configuration
    RequestMetadata metadata = 6;
}
```

#### SubscriberInfo
```protobuf
message SubscriberInfo {
    string subscriber_id = 1;
    string service_name = 2;
    string grpc_endpoint = 3;
    repeated string topic_patterns = 4;
    bool active = 5;
    int64 registered_at = 6;
}
```

### Streaming Messages

#### SubscribeEventsRequest
```protobuf
message SubscribeEventsRequest {
    string subscriber_id = 1;
    repeated string topic_patterns = 2;
    SubscriptionPosition position = 3;
    RequestMetadata metadata = 4;
}

enum SubscriptionPosition {
    SUBSCRIPTION_POSITION_UNSPECIFIED = 0;
    SUBSCRIPTION_POSITION_LATEST = 1;
    SUBSCRIPTION_POSITION_EARLIEST = 2;
    SUBSCRIPTION_POSITION_TIMESTAMP = 3;
}
```

#### DeliverEvent
```protobuf
message DeliverEvent {
    string message_id = 1;
    string topic = 2;
    int32 partition = 3;
    int64 offset = 4;
    string key = 5;
    bytes payload = 6;
    repeated Header headers = 7;
    int64 timestamp = 8;
    string event_type = 9;
}
```

### Configuration Types

#### RetryConfig
```protobuf
message RetryConfig {
    int32 max_retries = 1;              // Maximum retry attempts
    int64 initial_delay_ms = 2;         // Initial delay in milliseconds
    double multiplier = 3;              // Exponential backoff multiplier
    int64 max_delay_ms = 4;             // Maximum delay in milliseconds
    bool enable_dlq = 5;                // Route to DLQ on failure
    string dlq_topic = 6;               // Custom DLQ topic name
}
```

#### DeliveryConfig
```protobuf
message DeliveryConfig {
    int64 timeout_ms = 1;               // gRPC call timeout
    int32 max_concurrent = 2;           // Maximum concurrent deliveries
    RetryConfig retry_config = 3;       // Retry configuration
    bool require_ack = 4;               // Require acknowledgment
}
```

#### RequestMetadata
```protobuf
message RequestMetadata {
    string correlation_id = 1;          // Correlation ID for tracing
    string source_service = 2;          // ID of calling service
    int64 timestamp = 3;                // Request timestamp
    map<string, string> context = 4;    // Additional context
}
```

### Admin Messages

#### HealthResponse
```protobuf
message HealthResponse {
    HealthStatus status = 1;
    repeated ComponentHealth components = 2;
    BrokerMetrics metrics = 3;
}

enum HealthStatus {
    HEALTH_STATUS_UNSPECIFIED = 0;
    HEALTH_STATUS_HEALTHY = 1;
    HEALTH_STATUS_DEGRADED = 2;
    HEALTH_STATUS_UNHEALTHY = 3;
}
```

#### BrokerMetrics
```protobuf
message BrokerMetrics {
    int64 outbox_pending = 1;
    int64 inbox_pending = 2;
    int64 published_today = 3;
    int64 processed_today = 4;
    int64 dlq_count = 5;
    int64 active_subscribers = 6;
    bool kafka_connected = 7;
    bool database_connected = 8;
}
```

## Callback Service: RsBrokerCallback

Downstream services must implement this service to receive pushed messages.

```protobuf
service RsBrokerCallback {
    rpc Deliver(DeliverRequest) returns (DeliverResponse);
}
```

#### DeliverRequest
```protobuf
message DeliverRequest {
    string message_id = 1;
    string topic = 2;
    bytes payload = 3;
    repeated Header headers = 4;
    int64 timestamp = 5;
    string event_type = 6;
    int32 retry_count = 7;
}
```

#### DeliverResponse
```protobuf
message DeliverResponse {
    bool success = 1;
    string error = 2;
    bool retry = 3;
    int64 retry_delay_ms = 4;
}
```

## Usage Examples

### Publishing a Message

```rust
// Rust client example
let request = PublishRequest {
    message_id: uuid::Uuid::new_v4().to_string(),
    aggregate_type: "Order".to_string(),
    aggregate_id: "order-123".to_string(),
    event_type: "OrderCreated".to_string(),
    payload: order_json.as_bytes().to_vec(),
    topic: "orders.events".to_string(),
    partition_key: Some("order-123".to_string()),
    ..Default::default()
};

let response = client.publish(request).await?;
println!("Message published with ID: {}", response.message_id);
```

### Registering a Subscriber

```rust
let request = RegisterSubscriberRequest {
    subscriber_id: "service-b-orders".to_string(),
    service_name: "service-b".to_string(),
    grpc_endpoint: "http://service-b:50051".to_string(),
    topic_patterns: vec!["orders.*".to_string()],
    ..Default::default()
};

let response = client.register_subscriber(request).await?;
```

### Streaming Events

```rust
let request = SubscribeEventsRequest {
    subscriber_id: "service-b-orders".to_string(),
    topic_patterns: vec!["orders.*".to_string()],
    position: SubscriptionPosition::Latest as i32,
    ..Default::default()
};

let mut stream = client.subscribe_events(request).await?;

while let Some(event) = stream.message().await? {
    // Process event
    process_order_event(event);
}
```

## Error Handling

The service returns standard gRPC status codes:

| Code | Description |
|------|-------------|
| `OK` | Success |
| `INVALID_ARGUMENT` | Invalid request parameters |
| `NOT_FOUND` | Message or subscriber not found |
| `ALREADY_EXISTS` | Duplicate message ID |
| `RESOURCE_EXHAUSTED` | Rate limit exceeded |
| `INTERNAL` | Internal server error |
| `UNAVAILABLE` | Service temporarily unavailable |

## Complete Proto File

The complete proto file will be created at [`proto/rs_broker.proto`](../proto/rs_broker.proto) when implementing the service.
