// Benchmark suite for rs-broker-core retry logic operations
// Run with: cargo bench --package rs-broker-core

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rs_broker_config::RetryConfig;
use rs_broker_core::outbox::retry::RetryStrategy;

/// Helper to create a test retry strategy
fn create_test_retry_strategy() -> RetryStrategy {
    let config = RetryConfig {
        max_retries: 5,
        initial_delay_ms: 1000,
        multiplier: 2.0,
        max_delay_ms: 60000,
        enable_dlq: true,
        dlq_topic: "dlq.topic".to_string(),
    };
    RetryStrategy::new(config)
}

/// Benchmark exponential backoff calculation
fn bench_retry_calculate_delay(c: &mut Criterion) {
    let strategy = create_test_retry_strategy();

    let mut group = c.benchmark_group("retry_calculate_delay");

    for attempt in [1, 2, 3, 4, 5] {
        group.bench_with_input(
            BenchmarkId::new("attempt", attempt),
            &attempt,
            |b, &attempt| {
                b.iter(|| {
                    let delay = strategy.calculate_delay(attempt);
                    black_box(delay)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark retry decision logic
fn bench_retry_should_retry(c: &mut Criterion) {
    let strategy = create_test_retry_strategy();

    let mut group = c.benchmark_group("retry_should_retry");

    for attempt in [1, 2, 3, 4, 5, 6] {
        // Include one past max to test boundary
        group.bench_with_input(
            BenchmarkId::new("attempt", attempt),
            &attempt,
            |b, &attempt| {
                b.iter(|| {
                    let should_retry = strategy.should_retry(attempt);
                    black_box(should_retry)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark jitter calculation overhead
fn bench_retry_with_jitter(c: &mut Criterion) {
    use rand::Rng;

    c.bench_function("retry_with_jitter", |b| {
        b.iter(|| {
            let strategy = create_test_retry_strategy();
            let base_delay = strategy.calculate_delay(2);
            let base_duration = base_delay.as_millis() as f64;

            // Add jitter: ±10% variation
            let jitter_factor = rand::thread_rng().gen_range(-0.1..=0.1);
            let jittered_duration = base_duration * (1.0 + jitter_factor);

            let final_delay = std::time::Duration::from_millis(jittered_duration as u64);
            black_box(final_delay)
        });
    });
}

/// Benchmark retry history management
fn bench_retry_history(c: &mut Criterion) {
    use std::collections::HashMap;

    c.bench_function("retry_history", |b| {
        b.iter_with_setup(
            || {
                // Create a map simulating retry histories for multiple messages
                let mut histories = HashMap::new();
                for i in 0..1000 {
                    histories.insert(i, rand::random::<u32>());
                }
                histories
            },
            |mut histories| {
                // Simulate updating retry counts
                for (key, count) in histories.iter_mut() {
                    if *count < 5 {
                        // max retries
                        *count += 1;
                    }
                }
                black_box(histories.len())
            },
        );
    });
}

/// Benchmark max attempts boundary conditions
fn bench_retry_max_attempts(c: &mut Criterion) {
    let strategy = create_test_retry_strategy();

    c.bench_function("retry_max_attempts_boundary", |b| {
        b.iter(|| {
            // Test various attempt numbers around the boundary
            let results = [
                strategy.should_retry(4), // Should be true
                strategy.should_retry(5), // Should be false (max reached)
                strategy.should_retry(6), // Should be false
            ];
            black_box(results)
        });
    });
}

/// Benchmark retry strategy creation
fn bench_retry_strategy_creation(c: &mut Criterion) {
    c.bench_function("retry_strategy_creation", |b| {
        b.iter(|| {
            let config = RetryConfig::default();
            let strategy = RetryStrategy::new(config);
            black_box(strategy)
        });
    });
}

/// Benchmark full retry workflow
fn bench_retry_full_workflow(c: &mut Criterion) {
    c.bench_function("retry_full_workflow", |b| {
        b.iter(|| {
            let strategy = create_test_retry_strategy();

            // Simulate a full retry workflow for a message
            let mut attempt = 1;
            let mut delays = Vec::new();

            while strategy.should_retry(attempt) && attempt <= 10 {
                // Safety cap
                let delay = strategy.calculate_delay(attempt);
                delays.push(delay);
                attempt += 1;
            }

            black_box((delays.len(), delays.last().cloned()))
        });
    });
}

criterion_group!(
    retry_benches,
    bench_retry_calculate_delay,
    bench_retry_should_retry,
    bench_retry_with_jitter,
    bench_retry_history,
    bench_retry_max_attempts,
    bench_retry_strategy_creation,
    bench_retry_full_workflow
);

criterion_main!(retry_benches);
