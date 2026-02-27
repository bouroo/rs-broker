//! Benchmarks for gRPC Message Status service handlers

use criterion::{black_box, criterion_group, BenchmarkId, Criterion};
use tokio::runtime::Runtime;
use uuid::Uuid;

use rs_broker_db::{
    create_pool, outbox::MessageStatus, DbPool, OutboxMessage, SqlxOutboxRepository,
};
use rs_broker_proto::rsbroker::{
    GetMessageStatusRequest, GetMessageStatusResponse, Header, PublishRequest,
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

// Helper function to seed a test message in the database
async fn seed_test_message(pool: DbPool, status: MessageStatus) -> String {
    let repo = SqlxOutboxRepository::new(pool);
    let message_id = Uuid::new_v4();

    let message = OutboxMessage {
        id: message_id,
        aggregate_type: "test_aggregate".to_string(),
        aggregate_id: "test_id".to_string(),
        event_type: "test_event".to_string(),
        payload: serde_json::json!({"key": "value"}),
        headers: None,
        topic: "test_topic".to_string(),
        partition_key: Some("test_partition".to_string()),
        status,
        retry_count: 0,
        error_message: None,
        published_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    repo.create(&message)
        .await
        .expect("Failed to create test message");
    message_id.to_string()
}

// Benchmark single message status lookup
fn bench_get_message_status(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Create a test message first
    let service = create_test_service();
    let message_id = rt.block_on(seed_test_message(
        service.db_pool.clone(),
        MessageStatus::Published,
    ));

    c.bench_function("get_message_status", |b| {
        b.to_async(&rt).iter(|| {
            let service = black_box(&service);
            let request = tonic::Request::new(GetMessageStatusRequest {
                message_id: black_box(message_id.clone()),
                metadata: None,
            });
            service.get_message_status(request)
        })
    });
}

// Benchmark concurrent message status lookups
fn bench_get_message_status_concurrent(c: &mut Criterion) {
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let rt = Runtime::new().unwrap();

    // Create multiple test messages
    let service = create_test_service();
    let message_ids: Vec<String> = rt.block_on(async {
        let mut ids = Vec::new();
        for _ in 0..20 {
            let id = seed_test_message(service.db_pool.clone(), MessageStatus::Published).await;
            ids.push(id);
        }
        ids
    });

    c.bench_function("get_message_status_concurrent", |b| {
        b.to_async(&rt).iter_with_large_drop(|| {
            let service = Arc::new(black_box(service.clone()));
            let message_ids = Arc::new(black_box(message_ids.clone()));
            let semaphore = Arc::new(Semaphore::new(10)); // Limit concurrent requests

            async move {
                let mut handles = Vec::new();

                for i in 0..20 {
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let service_clone = service.clone();
                    let ids_clone = message_ids.clone();
                    let idx = i % ids_clone.len(); // Cycle through available IDs

                    let handle = tokio::spawn(async move {
                        let request = tonic::Request::new(GetMessageStatusRequest {
                            message_id: ids_clone[idx].clone(),
                            metadata: None,
                        });
                        let _ = service_clone.get_message_status(request).await;
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

criterion_group!(
    status_benches,
    bench_get_message_status,
    bench_get_message_status_concurrent,
);
