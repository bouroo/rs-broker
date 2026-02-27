//! Connection Pool Benchmarks
//!
//! Benchmarks for database connection pool operations:
//! - `bench_pool_acquire` - Connection acquisition latency
//! - `bench_pool_concurrent` - Concurrent connection usage
//!
//! Run with: cargo bench --package rs-broker-db --bench pool

use criterion::{BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::time::sleep;

/// Helper to get database URL
fn get_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/postgres".to_string())
}

/// Benchmark: Connection acquisition latency
pub fn bench_pool_acquire(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Create pool once for all iterations
    let pool = rt.block_on(async {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .min_connections(5)
            .acquire_timeout(Duration::from_secs(30))
            .max_lifetime(Duration::from_secs(1800))
            .connect(&database_url)
            .await
            .expect("Failed to create pool")
    });

    let pool = Arc::new(pool);

    // Benchmark single connection acquisition
    c.bench_function("pool_acquire_single", |b| {
        let pool = pool.clone();
        b.iter(|| {
            rt.block_on(async {
                let _conn = pool.acquire().await;
            });
        });
    });

    // Benchmark with different pool sizes
    let mut group = c.benchmark_group("pool_acquire_config");

    for max_conns in [5, 10, 25, 50] {
        group.throughput(Throughput::Elements(1));

        let database_url = database_url.clone();
        group.bench_with_input(
            BenchmarkId::from_parameter(max_conns),
            &max_conns,
            |b, &conns| {
                b.iter(|| {
                    rt.block_on(async {
                        let test_pool = sqlx::postgres::PgPoolOptions::new()
                            .max_connections(conns)
                            .min_connections(1)
                            .connect(&database_url)
                            .await
                            .expect("Failed to create pool");

                        let _conn = test_pool.acquire().await;
                        test_pool.close().await;
                    });
                });
            },
        );
    }

    group.finish();

    rt.block_on(async {
        pool.close().await;
    });
}

/// Benchmark: Concurrent connection usage
pub fn bench_pool_concurrent(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    let mut group = c.benchmark_group("pool_concurrent");

    // Test different concurrency levels
    for num_tasks in [5, 10, 20, 50] {
        group.throughput(Throughput::Elements(num_tasks as u64));

        let database_url = database_url.clone();
        group.bench_with_input(
            BenchmarkId::from_parameter(num_tasks),
            &num_tasks,
            |b, &tasks| {
                b.iter(|| {
                    rt.block_on(async {
                        let pool = sqlx::postgres::PgPoolOptions::new()
                            .max_connections(tasks + 5)
                            .min_connections(2)
                            .connect(&database_url)
                            .await
                            .expect("Failed to create pool");

                        // Spawn concurrent tasks that each acquire and release a connection
                        let handles: Vec<_> = (0..tasks)
                            .map(|_| {
                                let pool = pool.clone();
                                tokio::spawn(async move {
                                    let _conn = pool.acquire().await;
                                    // Simulate some work
                                    sleep(Duration::from_micros(100)).await;
                                })
                            })
                            .collect();

                        // Wait for all tasks to complete
                        for handle in handles {
                            handle.await.ok();
                        }

                        pool.close().await;
                    });
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Pool with pre-warmed connections
pub fn bench_pool_prewarmed(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    let mut group = c.benchmark_group("pool_prewarmed");

    for min_conns in [0, 5, 10] {
        group.throughput(Throughput::Elements(1));

        let database_url = database_url.clone();
        group.bench_with_input(
            BenchmarkId::from_parameter(min_conns),
            &min_conns,
            |b, &min| {
                b.iter(|| {
                    rt.block_on(async {
                        let pool = sqlx::postgres::PgPoolOptions::new()
                            .max_connections(25)
                            .min_connections(min)
                            .connect(&database_url)
                            .await
                            .expect("Failed to create pool");

                        // Warm up phase (if min > 0)
                        if min > 0 {
                            // Wait for connections to be established
                            sleep(Duration::from_millis(100)).await;
                        }

                        // Benchmark acquisition
                        let _conn = pool.acquire().await;

                        pool.close().await;
                    });
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Connection reuse under load
pub fn bench_pool_reuse(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    // Create pool with some connections
    let pool = rt.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .min_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool");

        // Wait for connections to be established
        sleep(Duration::from_millis(500)).await;

        pool
    });

    let pool = Arc::new(pool);

    c.bench_function("pool_reuse_under_load", |b| {
        let pool = pool.clone();
        b.iter(|| {
            rt.block_on(async {
                // Simulate a workload that acquires and releases connections
                for _ in 0..100 {
                    let _conn = pool.acquire().await;
                    // Brief hold time to simulate query execution
                    sleep(Duration::from_micros(10)).await;
                }
            });
        });
    });

    rt.block_on(async {
        pool.close().await;
    });
}

/// Benchmark: Pool saturation behavior
pub fn bench_pool_saturation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let database_url = get_database_url();

    let mut group = c.benchmark_group("pool_saturation");

    // Create pool with limited connections
    let limited_pool = rt.block_on(async {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&database_url)
            .await
            .expect("Failed to create pool")
    });

    let limited_pool = Arc::new(limited_pool);

    // Test with more concurrent requests than available connections
    for overcommit in [5, 10, 20] {
        group.throughput(Throughput::Elements(overcommit as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(overcommit),
            &overcommit,
            |b, &requests| {
                let pool = limited_pool.clone();
                b.iter(|| {
                    rt.block_on(async {
                        let handles: Vec<_> = (0..requests)
                            .map(|_| {
                                let pool = pool.clone();
                                tokio::spawn(async move { pool.acquire().await })
                            })
                            .collect();

                        let mut successes = 0;
                        let mut failures = 0;

                        for handle in handles {
                            match handle.await {
                                Ok(Ok(_conn)) => {
                                    successes += 1;
                                }
                                _ => {
                                    failures += 1;
                                }
                            }
                        }

                        (successes, failures)
                    });
                });
            },
        );
    }

    group.finish();

    rt.block_on(async {
        limited_pool.close().await;
    });
}

/// Run all pool benchmarks
pub fn run_benchmarks(c: &mut Criterion) {
    bench_pool_acquire(c);
    bench_pool_concurrent(c);
    bench_pool_prewarmed(c);
    bench_pool_reuse(c);
    bench_pool_saturation(c);
}
