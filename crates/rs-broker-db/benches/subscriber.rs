//! Subscriber Repository Benchmarks
//!
//! Benchmarks for critical subscriber repository operations:
//! - `bench_subscriber_get_all_active` - Query active subscribers
//! - `bench_subscriber_pattern_matching` - Topic pattern matching performance
//!
//! Run with: cargo bench --package rs-broker-db --bench subscriber

use criterion::{BenchmarkId, Criterion, Throughput};
use tokio::runtime::Runtime;

use rs_broker_db::{SqlxSubscriberRepository, Subscriber, SubscriberRepository};

/// Helper to get database URL
fn get_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/postgres".to_string())
}

/// Create a sample subscriber for benchmarking
fn create_sample_subscriber(service_name: &str, patterns: Vec<String>) -> Subscriber {
    Subscriber::new(
        service_name.to_string(),
        format!("http://{}:50051", service_name),
        patterns,
    )
}

/// Benchmark: Get all active subscribers
pub fn bench_subscriber_get_all_active(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Setup: Insert test subscribers
    rt.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        // Clean up first
        sqlx::query("DELETE FROM subscribers")
            .execute(&pool)
            .await
            .ok();

        // Insert mix of active and inactive subscribers
        let patterns = vec![
            "orders.*".to_string(),
            "payments.*".to_string(),
            "shipments.*".to_string(),
        ];

        let repo = SqlxSubscriberRepository::new(pool.clone());
        for i in 0..500 {
            let is_active = i % 10 != 0; // 90% active
            let mut sub = create_sample_subscriber(&format!("service-{}", i), patterns.clone());
            sub.active = is_active;
            repo.create(&sub).await.ok();
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

    let repo = SqlxSubscriberRepository::new(pool.clone());

    c.bench_function("subscriber_get_all_active", |b| {
        b.iter(|| {
            rt.block_on(async {
                let repo = SqlxSubscriberRepository::new(pool.clone());
                repo.get_all_active().await.ok();
            });
        });
    });

    // Cleanup
    rt.block_on(async {
        sqlx::query("DELETE FROM subscribers")
            .execute(&pool)
            .await
            .ok();
        pool.close().await;
    });
}

/// Benchmark: Topic pattern matching
///
/// This benchmark simulates matching a message topic against subscriber patterns.
/// Since the database doesn't support pattern matching natively, we fetch active
/// subscribers and do the matching in-memory (which is how the actual implementation works).
pub fn bench_subscriber_pattern_matching(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Setup: Insert subscribers with various topic patterns
    rt.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        // Clean up first
        sqlx::query("DELETE FROM subscribers")
            .execute(&pool)
            .await
            .ok();

        // Create diverse topic patterns
        let all_patterns = vec![
            // Exact matches
            vec!["orders.created".to_string()],
            vec!["orders.updated".to_string()],
            vec!["orders.deleted".to_string()],
            // Wildcard patterns
            vec!["orders.*".to_string()],
            vec!["payments.*".to_string()],
            vec!["shipments.*".to_string()],
            // Multi-pattern
            vec!["orders.*".to_string(), "payments.*".to_string()],
            vec!["*".to_string()],
        ];

        let repo = SqlxSubscriberRepository::new(pool.clone());
        for (i, patterns) in all_patterns.iter().enumerate() {
            for j in 0..50 {
                let mut sub =
                    create_sample_subscriber(&format!("service-{}-{}", i, j), patterns.clone());
                sub.active = true;
                repo.create(&sub).await.ok();
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

    let mut group = c.benchmark_group("subscriber_pattern_matching");

    // Test different message topics
    let test_topics = vec![
        "orders.created",
        "orders.updated",
        "orders.deleted",
        "payments.created",
        "shipments.delivered",
        "unknown.topic",
    ];

    for topic in test_topics {
        group.throughput(Throughput::Elements(1));
        let topic_str = topic.to_string();

        group.bench_function(BenchmarkId::new("match_topic", topic), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let pool = pool.clone();
                    // Fetch all active subscribers (as the real implementation does)
                    let repo = SqlxSubscriberRepository::new(pool);
                    let subscribers = repo.get_all_active().await.ok();

                    if let Some(subs) = subscribers {
                        // Perform pattern matching in-memory
                        let _matched: Vec<_> = subs
                            .iter()
                            .filter(|sub| {
                                sub.topic_patterns
                                    .iter()
                                    .any(|pattern| matches_pattern(&topic_str, pattern))
                            })
                            .collect();
                    }
                });
            });
        });
    }

    group.finish();

    // Cleanup
    rt.block_on(async {
        sqlx::query("DELETE FROM subscribers")
            .execute(&pool)
            .await
            .ok();
        pool.close().await;
    });
}

/// Simple glob-style pattern matching function
///
/// Matches a topic against a pattern where:
/// - `*` matches any sequence of characters
/// - Exact match if no wildcards
fn matches_pattern(topic: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if let Some(stripped) = pattern.strip_prefix("*.") {
        // Handle prefix patterns like "orders.*"
        return topic.starts_with(stripped);
    }

    if let Some(stripped) = pattern.strip_suffix(".*") {
        // Handle suffix patterns like "*events"
        return topic.ends_with(stripped);
    }

    // Exact match
    topic == pattern
}

/// Benchmark: Subscriber CRUD operations
pub fn bench_subscriber_crud(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Benchmark: Create subscriber
    c.bench_function("subscriber_create", |b| {
        let pool = rt.block_on(async {
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await
                .expect("Failed to connect to database")
        });

        b.iter(|| {
            let sub = create_sample_subscriber(
                &format!("crud-{}", uuid::Uuid::new_v4()),
                vec!["test.*".to_string()],
            );
            let id = sub.id;

            rt.block_on(async {
                let repo = SqlxSubscriberRepository::new(pool.clone());
                let result = repo.create(&sub).await;

                // Cleanup
                sqlx::query("DELETE FROM subscribers WHERE id = $1")
                    .bind(id)
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

/// Run all subscriber benchmarks
pub fn run_benchmarks(c: &mut Criterion) {
    bench_subscriber_get_all_active(c);
    bench_subscriber_pattern_matching(c);
    bench_subscriber_crud(c);
}
