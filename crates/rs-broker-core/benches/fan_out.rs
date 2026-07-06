// Benchmark: sequential vs bounded-concurrent subscriber fan-out.
//
// We don't exercise real gRPC here (that requires live channels and a server);
// we simulate per-subscriber latency with `tokio::time::sleep`. The point is
// to demonstrate that the `buffer_unordered` fan-out pattern adopted in
// `SubscriberDispatcher::dispatch_to_all` collapses total latency from
// O(N*L) (sequential) to O(L) (concurrent, bounded by the slowest subscriber),
// independent of subscriber count N.
//
// Run with: cargo bench --package rs-broker-core --bench fan_out

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use futures::stream::{self, StreamExt};
use std::time::Duration;
use tokio::runtime::Runtime;

/// Simulates `N` sequential gRPC deliveries, each taking ~1ms.
async fn sequential_fan_out(n: usize) {
    for _ in 0..n {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Simulates `N` concurrent gRPC deliveries, each taking ~1ms, bounded by
/// `concurrency` simultaneous in-flight calls (mirrors `buffer_unordered`
/// inside `dispatch_to_all`).
async fn concurrent_fan_out(n: usize, concurrency: usize) {
    stream::iter(0..n)
        .map(|_| tokio::time::sleep(Duration::from_millis(1)))
        .buffer_unordered(concurrency)
        .collect::<Vec<()>>()
        .await;
}

fn bench_fan_out(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("fan_out");

    for n in [10usize, 50, 100] {
        group.bench_with_input(BenchmarkId::new("sequential", n), &n, |b, &n| {
            b.iter(|| rt.block_on(sequential_fan_out(n)));
        });
        group.bench_with_input(BenchmarkId::new("concurrent_32", n), &n, |b, &n| {
            b.iter(|| rt.block_on(concurrent_fan_out(n, 32)));
        });
    }

    group.finish();
}

criterion_group!(fan_out_benches, bench_fan_out);
criterion_main!(fan_out_benches);
