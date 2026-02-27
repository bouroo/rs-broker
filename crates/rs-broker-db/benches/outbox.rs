//! Outbox Repository Benchmarks
//!
//! Benchmarks for critical outbox repository operations:
//! - `bench_outbox_create` - Single message insertion
//! - `bench_outbox_get_pending` - Query pending messages with various limits
//! - `bench_outbox_mark_published` - Status update operation
//! - `bench_outbox_batch_insert` - Bulk insertion performance
//!
//! Run with: cargo bench --package rs-broker-db --bench outbox

use criterion::{BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use tokio::runtime::Runtime;

use rs_broker_db::{OutboxMessage, OutboxRepository, SqlxOutboxRepository};

/// Helper to get database URL
fn get_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/postgres".to_string())
}

/// Benchmark: Single message insertion
pub fn bench_outbox_create(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    c.bench_function("outbox_create_single", |b| {
        // Pre-create the pool outside the benchmark
        let pool = rt.block_on(async {
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await
                .expect("Failed to connect to database")
        });

        b.iter(|| {
            let message = OutboxMessage::new(
                "Order".to_string(),
                uuid::Uuid::new_v4().to_string(),
                "OrderCreated".to_string(),
                serde_json::json!({
                    "order_id": uuid::Uuid::new_v4().to_string(),
                    "amount": 100.0,
                    "currency": "USD"
                }),
                "orders.events".to_string(),
            );

            rt.block_on(async {
                let repo = SqlxOutboxRepository::new(pool.clone());
                let result = repo.create(&message).await;

                // Cleanup
                sqlx::query("DELETE FROM outbox_messages WHERE id = $1")
                    .bind(message.id)
                    .execute(&pool)
                    .await
                    .ok();

                result
            });
        });

        rt.block_on(async {
            pool.close().await;
        });
    });
}

/// Benchmark: Get pending messages with various limits
pub fn bench_outbox_get_pending(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Setup: Insert pending messages
    rt.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        // Clean up first
        sqlx::query("DELETE FROM outbox_messages")
            .execute(&pool)
            .await
            .ok();

        // Insert test messages
        let repo = SqlxOutboxRepository::new(pool.clone());
        for i in 0..1000 {
            let message = OutboxMessage::new(
                "Order".to_string(),
                format!("order-{}", i),
                "OrderCreated".to_string(),
                serde_json::json!({"order_id": i}),
                "orders.events".to_string(),
            );
            repo.create(&message).await.ok();
        }

        pool.close().await;
    });

    // Create a persistent pool for benchmarks
    let pool = rt.block_on(async {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database")
    });

    let repo = Arc::new(SqlxOutboxRepository::new(pool.clone()));

    let mut group = c.benchmark_group("outbox_get_pending");

    for limit in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(limit as u64));
        group.bench_with_input(BenchmarkId::from_parameter(limit), &limit, |b, &limit| {
            let repo = repo.clone();
            b.iter(|| {
                rt.block_on(async {
                    repo.get_pending(limit).await.ok();
                });
            });
        });
    }

    group.finish();

    // Cleanup
    rt.block_on(async {
        sqlx::query("DELETE FROM outbox_messages")
            .execute(&pool)
            .await
            .ok();
        pool.close().await;
    });
}

/// Benchmark: Mark message as published (status update)
pub fn bench_outbox_mark_published(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Setup: Insert a pending message
    let test_message_id = rt.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        let message = OutboxMessage::new(
            "Order".to_string(),
            "mark-published-test".to_string(),
            "OrderCreated".to_string(),
            serde_json::json!({"order_id": "test"}),
            "orders.events".to_string(),
        );
        let repo = SqlxOutboxRepository::new(pool.clone());
        repo.create(&message)
            .await
            .expect("Failed to create message");

        pool.close().await;
        message.id
    });

    let pool = rt.block_on(async {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database")
    });

    let repo = SqlxOutboxRepository::new(pool.clone());
    let message_id = test_message_id;

    c.bench_function("outbox_mark_published", |b| {
        let repo = SqlxOutboxRepository::new(pool.clone());
        b.iter(|| {
            rt.block_on(async {
                let result = repo.mark_published(message_id).await;
                // Reset status for next iteration
                sqlx::query("UPDATE outbox_messages SET status = 'pending' WHERE id = $1")
                    .bind(message_id)
                    .execute(&pool)
                    .await
                    .ok();
                result
            });
        });
    });

    // Cleanup
    rt.block_on(async {
        sqlx::query("DELETE FROM outbox_messages WHERE id = $1")
            .bind(test_message_id)
            .execute(&pool)
            .await
            .ok();
        pool.close().await;
    });
}

/// Benchmark: Batch insertion performance
pub fn bench_outbox_batch_insert(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    let mut group = c.benchmark_group("outbox_batch_insert");

    for batch_size in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    rt.block_on(async {
                        let pool = sqlx::postgres::PgPoolOptions::new()
                            .max_connections(10)
                            .connect(&database_url)
                            .await
                            .expect("Failed to connect to database");

                        let repo = SqlxOutboxRepository::new(pool.clone());

                        // Create batch of messages
                        for i in 0..size {
                            let message = OutboxMessage::new(
                                "Order".to_string(),
                                format!("batch-{}-{}", uuid::Uuid::new_v4(), i),
                                "OrderCreated".to_string(),
                                serde_json::json!({"order_id": i}),
                                "orders.events".to_string(),
                            );
                            repo.create(&message).await.ok();
                        }

                        // Cleanup
                        sqlx::query("DELETE FROM outbox_messages")
                            .execute(&pool)
                            .await
                            .ok();
                        pool.close().await;
                    });
                });
            },
        );
    }

    group.finish();
}

/// Run all outbox benchmarks
pub fn run_benchmarks(c: &mut Criterion) {
    bench_outbox_create(c);
    bench_outbox_get_pending(c);
    bench_outbox_mark_published(c);
    bench_outbox_batch_insert(c);
}
