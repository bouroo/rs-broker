//! Inbox Repository Benchmarks
//!
//! Benchmarks for critical inbox repository operations:
//! - `bench_inbox_create` - Single message insertion
//! - `bench_inbox_get_by_topic_offset` - Deduplication lookup
//! - `bench_inbox_concurrent_inserts` - Concurrent insertion performance
//!
//! Run with: cargo bench --package rs-broker-db --bench inbox

use criterion::{BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use tokio::runtime::Runtime;

use rs_broker_db::{InboxMessage, InboxRepository, SqlxInboxRepository};

/// Helper to get database URL
fn get_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/postgres".to_string())
}

/// Create a sample inbox message for benchmarking
fn create_sample_message(topic: &str, partition: i32, offset: i64) -> InboxMessage {
    InboxMessage::new(
        topic.to_string(),
        partition,
        offset,
        serde_json::json!({
            "event_type": "OrderCreated",
            "data": {
                "order_id": format!("order-{}", offset),
                "amount": 100.0
            }
        }),
    )
}

/// Benchmark: Single message insertion
pub fn bench_inbox_create(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    c.bench_function("inbox_create_single", |b| {
        let pool = rt.block_on(async {
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await
                .expect("Failed to connect to database")
        });

        b.iter(|| {
            let offset = uuid::Uuid::new_v4().as_u128() as i64;
            let message = create_sample_message("orders.events", 0, offset);

            rt.block_on(async {
                let repo = SqlxInboxRepository::new(pool.clone());
                let result = repo.create(&message).await;

                // Cleanup
                sqlx::query("DELETE FROM inbox_messages WHERE id = $1")
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

/// Benchmark: Get by topic and offset (deduplication lookup)
pub fn bench_inbox_get_by_topic_offset(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Setup: Insert test messages for deduplication checks
    let test_topics = vec!["orders.events", "payments.events", "shipments.events"];

    rt.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        // Clean up first
        sqlx::query("DELETE FROM inbox_messages")
            .execute(&pool)
            .await
            .ok();

        // Insert test messages for each topic
        let repo = SqlxInboxRepository::new(pool.clone());
        for topic in &test_topics {
            for partition in 0..3 {
                for offset in 0..1000 {
                    let message = create_sample_message(topic, partition, offset as i64);
                    repo.create(&message).await.ok();
                }
            }
        }

        pool.close().await;
    });

    // Create persistent pool for benchmarks
    let pool = rt.block_on(async {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database")
    });

    let repo = Arc::new(SqlxInboxRepository::new(pool.clone()));

    let mut group = c.benchmark_group("inbox_get_by_topic_offset");

    // Test different topic/offset combinations
    let test_cases = vec![
        ("orders.events", 0, 0),
        ("orders.events", 0, 500),
        ("orders.events", 2, 999),
        ("payments.events", 1, 100),
        ("shipments.events", 0, 50),
    ];

    for (topic, partition, offset) in test_cases {
        group.throughput(Throughput::Elements(1));
        let topic_str = topic.to_string();
        let repo = repo.clone();

        group.bench_function(
            BenchmarkId::new(
                "dedup_lookup",
                format!("{}_{}_{}", topic, partition, offset),
            ),
            |b| {
                let repo = SqlxInboxRepository::new(pool.clone());
                b.iter(|| {
                    rt.block_on(async {
                        repo.get_by_topic_offset(&topic_str, offset as i64)
                            .await
                            .ok();
                    });
                });
            },
        );
    }

    group.finish();

    // Cleanup
    rt.block_on(async {
        sqlx::query("DELETE FROM inbox_messages")
            .execute(&pool)
            .await
            .ok();
        pool.close().await;
    });
}

/// Benchmark: Concurrent insertions
pub fn bench_inbox_concurrent_inserts(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    let mut group = c.benchmark_group("inbox_concurrent_inserts");

    for num_tasks in [5, 10, 20, 50] {
        group.throughput(Throughput::Elements(num_tasks as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(num_tasks),
            &num_tasks,
            |b, &tasks| {
                b.iter(|| {
                    rt.block_on(async {
                        let pool = sqlx::postgres::PgPoolOptions::new()
                            .max_connections(tasks + 5)
                            .connect(&database_url)
                            .await
                            .expect("Failed to connect to database");

                        // Run concurrent inserts
                        let handles: Vec<_> = (0..tasks)
                            .map(|i| {
                                let pool = pool.clone();
                                tokio::spawn(async move {
                                    let offset =
                                        (uuid::Uuid::new_v4().as_u128() + i as u128) as i64;
                                    let message = create_sample_message("orders.events", 0, offset);
                                    let repo = SqlxInboxRepository::new(pool);
                                    repo.create(&message).await.ok();
                                })
                            })
                            .collect();

                        for handle in handles {
                            handle.await.ok();
                        }

                        // Cleanup
                        sqlx::query("DELETE FROM inbox_messages")
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

/// Run all inbox benchmarks
pub fn run_benchmarks(c: &mut Criterion) {
    bench_inbox_create(c);
    bench_inbox_get_by_topic_offset(c);
    bench_inbox_concurrent_inserts(c);
}
