//! Benchmarks for gRPC Consumer service handlers

use criterion::{black_box, criterion_group, Criterion};
use tokio::runtime::Runtime;
use uuid::Uuid;

use rs_broker_db::{test_utils::setup_test_db, DbPool};
use rs_broker_proto::rsbroker::{
    ListSubscribersRequest, ListSubscribersResponse, RegisterSubscriberRequest,
    RegisterSubscriberResponse, UnregisterSubscriberRequest, UnregisterSubscriberResponse,
    UpdateSubscriberRequest, UpdateSubscriberResponse,
};
use rs_broker_server::grpc::service::RsBrokerService;

// Helper function to create a test service instance
fn create_test_service() -> RsBrokerService {
    let rt = Runtime::new().unwrap();
    let pool = rt.block_on(setup_test_db());
    RsBrokerService::new(pool)
}

// Benchmark subscriber registration
fn bench_subscribe(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    c.bench_function("register_subscriber", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let request = tonic::Request::new(RegisterSubscriberRequest {
                subscriber_id: Uuid::new_v4().to_string(),
                service_name: "test_service".to_string(),
                grpc_endpoint: "http://localhost:50051".to_string(),
                topic_patterns: vec!["test.*".to_string()],
                delivery_config: None,
                metadata: None,
            });
            service.register_subscriber(request)
        })
    });
}

// Benchmark subscriber unregistration
fn bench_unsubscribe(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    // First register a subscriber to have something to unregister
    let subscriber_id = rt.block_on(async {
        let request = tonic::Request::new(RegisterSubscriberRequest {
            subscriber_id: Uuid::new_v4().to_string(),
            service_name: "test_service_unregister".to_string(),
            grpc_endpoint: "http://localhost:50052".to_string(),
            topic_patterns: vec!["test.unregister".to_string()],
            delivery_config: None,
            metadata: None,
        });
        let response = service.register_subscriber(request).await.unwrap();
        response.into_inner().subscriber_id
    });

    c.bench_function("unregister_subscriber", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let request = tonic::Request::new(UnregisterSubscriberRequest {
                subscriber_id: black_box(subscriber_id.clone()),
                metadata: None,
            });
            service.unregister_subscriber(request)
        })
    });
}

// Benchmark listing subscribers
fn bench_list_subscribers(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    // Pre-populate with some subscribers
    rt.block_on(async {
        for i in 0..10 {
            let request = tonic::Request::new(RegisterSubscriberRequest {
                subscriber_id: Uuid::new_v4().to_string(),
                service_name: format!("test_service_{}", i),
                grpc_endpoint: format!("http://localhost:{}", 50050 + i),
                topic_patterns: vec![format!("test.topic.{}", i)],
                delivery_config: None,
                metadata: None,
            });
            let _ = service.register_subscriber(request).await;
        }
    });

    c.bench_function("list_subscribers", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let request = tonic::Request::new(ListSubscribersRequest {
                topic_filter: None,
                active_only: true,
                metadata: None,
            });
            service.list_subscribers(request)
        })
    });
}

// Benchmark updating subscriber
fn bench_update_subscriber(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    // First register a subscriber to update
    let subscriber_id = rt.block_on(async {
        let request = tonic::Request::new(RegisterSubscriberRequest {
            subscriber_id: Uuid::new_v4().to_string(),
            service_name: "test_service_update".to_string(),
            grpc_endpoint: "http://localhost:50053".to_string(),
            topic_patterns: vec!["test.update".to_string()],
            delivery_config: None,
            metadata: None,
        });
        let response = service.register_subscriber(request).await.unwrap();
        response.into_inner().subscriber_id
    });

    c.bench_function("update_subscriber", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let request = tonic::Request::new(UpdateSubscriberRequest {
                subscriber_id: black_box(subscriber_id.clone()),
                grpc_endpoint: "http://localhost:50054".to_string(), // Different endpoint
                topic_patterns: vec!["updated.pattern".to_string()], // Different pattern
                active: true,
                delivery_config: None,
                metadata: None,
            });
            service.update_subscriber(request)
        })
    });
}

criterion_group!(
    consumer_benches,
    bench_subscribe,
    bench_unsubscribe,
    bench_list_subscribers,
    bench_update_subscriber,
);
