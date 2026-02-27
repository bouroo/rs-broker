//! Benchmarks for gRPC Publish service handlers

use criterion::{black_box, criterion_group, BenchmarkId, Criterion, Throughput};
use tokio::runtime::Runtime;
use uuid::Uuid;

use rs_broker_db::{create_pool, DbPool};
use rs_broker_proto::rsbroker::{
    Header, PublishBatchRequest, PublishBatchResponse, PublishRequest, PublishResponse,
};
use rs_broker_server::grpc::service::RsBrokerService;

/// Helper to get database URL
fn get_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/postgres".to_string())
}

// Helper function to create a test service instance
fn create_test_service() -> RsBrokerService {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();
    let pool = rt.block_on(async {
        create_pool(&database_url)
            .await
            .expect("Failed to create database pool")
    });
    RsBrokerService::new(pool)
}

// Helper function to create a sample publish request
fn create_publish_request(payload_size: usize) -> PublishRequest {
    let payload = vec![b'a'; payload_size];
    PublishRequest {
        message_id: Uuid::new_v4().to_string(),
        aggregate_type: "test_aggregate".to_string(),
        aggregate_id: "test_id".to_string(),
        event_type: "test_event".to_string(),
        payload,
        headers: vec![Header {
            key: "test_header".to_string(),
            value: "test_value".to_string(),
        }],
        topic: "test_topic".to_string(),
        partition_key: "test_partition".to_string(),
        retry_config: None,
        metadata: None,
    }
}

// Benchmark single message publish latency
fn bench_publish_single(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    c.bench_function("publish_single", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let request = tonic::Request::new(create_publish_request(1024));
            service.publish(request)
        })
    });
}

// Benchmark batch publish with 10 messages
fn bench_publish_batch_10(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    c.bench_function("publish_batch_10", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let messages: Vec<_> = (0..10).map(|_| create_publish_request(1024)).collect();
            let request = tonic::Request::new(PublishBatchRequest {
                messages,
                metadata: None,
            });
            service.publish_batch(request)
        })
    });
}

// Benchmark batch publish with 100 messages
fn bench_publish_batch_100(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    c.bench_function("publish_batch_100", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let messages: Vec<_> = (0..100).map(|_| create_publish_request(1024)).collect();
            let request = tonic::Request::new(PublishBatchRequest {
                messages,
                metadata: None,
            });
            service.publish_batch(request)
        })
    });
}

// Benchmark concurrent publish requests
fn bench_publish_concurrent(c: &mut Criterion) {
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    c.bench_function("publish_concurrent", |b| {
        b.to_async(&rt).iter_with_large_drop(|| {
            let service = Arc::new(black_box(service.clone()));
            let semaphore = Arc::new(Semaphore::new(10)); // Limit concurrent requests

            async move {
                let mut handles = Vec::new();

                for _ in 0..20 {
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let service_clone = service.clone();

                    let handle = tokio::spawn(async move {
                        let request = tonic::Request::new(create_publish_request(1024));
                        let _ = service_clone.publish(request).await;
                        drop(permit);
                    });

                    handles.push(handle);
                }

                // Wait for all tasks to complete
                for handle in handles {
                    let _ = handle.await;
                }
            }
        })
    });
}

// Benchmark publish with different payload sizes
fn bench_publish_payload_size(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let service = create_test_service();

    let payload_sizes = vec![100, 1024, 10240, 102400]; // 100B, 1KB, 10KB, 100KB

    let mut group = c.benchmark_group("publish_payload_size");
    for size in payload_sizes {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.to_async(&rt).iter(|| {
                let service = black_box(&service);
                let request = tonic::Request::new(create_publish_request(size));
                service.publish(request)
            })
        });
    }
    group.finish();
}

criterion_group!(
    publish_benches,
    bench_publish_single,
    bench_publish_batch_10,
    bench_publish_batch_100,
    bench_publish_concurrent,
    bench_publish_payload_size,
);
