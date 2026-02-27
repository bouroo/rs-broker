//! Integration benchmarks for end-to-end gRPC service flows

use criterion::{black_box, criterion_group, Criterion};
use tokio::runtime::Runtime;
use uuid::Uuid;

use rs_broker_db::{outbox::MessageStatus, test_utils::setup_test_db, DbPool};
use rs_broker_proto::rsbroker::{
    GetMessageStatusRequest, GetMessageStatusResponse, Header, PublishRequest, PublishResponse,
};
use rs_broker_server::grpc::service::RsBrokerService;

// Helper function to create a test service instance
fn create_test_service() -> RsBrokerService {
    let rt = Runtime::new().unwrap();
    let pool = rt.block_on(setup_test_db());
    RsBrokerService::new(pool)
}

// Helper function to create a sample publish request
fn create_publish_request() -> PublishRequest {
    PublishRequest {
        message_id: Uuid::new_v4().to_string(),
        aggregate_type: "integration_test".to_string(),
        aggregate_id: "integration_id".to_string(),
        event_type: "integration_event".to_string(),
        payload: b"test_payload".to_vec(),
        headers: vec![Header {
            key: "integration_header".to_string(),
            value: "integration_value".to_string(),
        }],
        topic: "integration_topic".to_string(),
        partition_key: "integration_partition".to_string(),
        retry_config: None,
        metadata: None,
    }
}

// Benchmark full publish flow (gRPC → DB persistence)
fn bench_full_publish_flow(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    c.bench_function("full_publish_flow", |b| {
        b.to_async(&rt).iter(|| async {
            let service = black_box(&service);

            // Step 1: Publish the message
            let publish_request = tonic::Request::new(create_publish_request());
            let publish_response = service.publish(publish_request).await;

            // Step 2: Get the message status
            if let Ok(publish_resp) = publish_response {
                let message_id = publish_resp.into_inner().message_id;
                let status_request = tonic::Request::new(GetMessageStatusRequest {
                    message_id,
                    metadata: None,
                });
                let _status_response = service.get_message_status(status_request).await;
            }
        })
    });
}

// Benchmark full consume flow simulation (would involve Kafka in real scenario)
fn bench_full_consume_flow(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    // In a real scenario, this would involve Kafka consumers delivering messages
    // For the benchmark, we'll simulate the flow by creating messages and then
    // registering/unregistering subscribers to measure the overhead

    c.bench_function("full_consume_flow", |b| {
        b.to_async(&rt).iter(|| async {
            let service = black_box(&service);

            // Simulate the flow of registering a subscriber, publishing a message,
            // and then the subscriber consuming it (simulated by status checks)

            // Register a subscriber
            let register_request =
                tonic::Request::new(rs_broker_proto::rsbroker::RegisterSubscriberRequest {
                    subscriber_id: Uuid::new_v4().to_string(),
                    service_name: "consumer_simulation".to_string(),
                    grpc_endpoint: "http://localhost:50060".to_string(),
                    topic_patterns: vec!["integration_topic".to_string()],
                    delivery_config: None,
                    metadata: None,
                });
            let _register_response = service.register_subscriber(register_request).await;

            // Publish a message
            let publish_request = tonic::Request::new(create_publish_request());
            let publish_response = service.publish(publish_request).await;

            // Check status (simulating consumption)
            if let Ok(publish_resp) = publish_response {
                let message_id = publish_resp.into_inner().message_id;
                let status_request = tonic::Request::new(GetMessageStatusRequest {
                    message_id,
                    metadata: None,
                });
                let _status_response = service.get_message_status(status_request).await;
            }
        })
    });
}

// Benchmark multiple publish operations in sequence
fn bench_sequential_publish_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    c.bench_function("sequential_publish_operations", |b| {
        b.to_async(&rt).iter(|| async {
            let service = black_box(&service);

            // Perform a sequence of operations: publish, check status, publish again
            for _ in 0..5 {
                // Publish a message
                let publish_request = tonic::Request::new(create_publish_request());
                let publish_response = service.publish(publish_request).await;

                // Check status if publish was successful
                if let Ok(publish_resp) = publish_response {
                    let message_id = publish_resp.into_inner().message_id;
                    let status_request = tonic::Request::new(GetMessageStatusRequest {
                        message_id,
                        metadata: None,
                    });
                    let _status_response = service.get_message_status(status_request).await;
                }
            }
        })
    });
}

criterion_group!(
    integration_benches,
    bench_full_publish_flow,
    bench_full_consume_flow,
    bench_sequential_publish_operations,
);
