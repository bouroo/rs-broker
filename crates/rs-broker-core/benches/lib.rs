// Benchmark suite for rs-broker-core inbox deduplication and dispatch logic
// Run with: cargo bench --package rs-broker-core

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rs_broker_core::inbox::dedup::Deduplicator;
use rs_broker_db::Subscriber;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use uuid::Uuid;

/// Helper function to simulate subscriber pattern matching (copied from dispatcher)
fn matches_topic(topic: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        // Simple wildcard matching
        let prefix = pattern.trim_end_matches('*');
        topic.starts_with(prefix)
    } else if pattern.contains('>') {
        // MQTT-style wildcard (not implemented)
        false
    } else {
        topic == pattern
    }
}

/// Helper to create a test deduplicator
fn create_test_deduplicator(max_size: usize) -> Arc<Deduplicator> {
    Arc::new(Deduplicator::new(max_size))
}

/// Benchmark single deduplication check
fn bench_dedup_check(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dedup = rt.block_on(async {
        let dedup = create_test_deduplicator(1000);
        // Pre-populate with some entries
        for i in 0..500 {
            dedup.mark_seen(format!("topic1-offset{}", i)).await;
        }
        dedup
    });

    c.bench_function("dedup_check_single", |b| {
        b.iter(|| {
            rt.block_on(async {
                black_box(dedup.is_duplicate("topic1-offset250").await);
            });
        });
    });
}

/// Benchmark batch deduplication checks
fn bench_dedup_check_batch(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dedup = rt.block_on(async {
        let dedup = create_test_deduplicator(10000);
        // Pre-populate with entries
        for i in 0..5000 {
            dedup.mark_seen(format!("topic1-offset{}", i)).await;
        }
        dedup
    });

    c.bench_function("dedup_check_batch", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Check multiple entries in a batch
                for i in 0..100 {
                    black_box(dedup.is_duplicate(&format!("topic1-offset{}", i)).await);
                }
            });
        });
    });
}

/// Benchmark concurrent deduplication checks
fn bench_dedup_concurrent(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dedup = rt.block_on(async {
        let dedup = create_test_deduplicator(10000);
        // Pre-populate with entries
        for i in 0..5000 {
            dedup.mark_seen(format!("topic1-offset{}", i)).await;
        }
        dedup
    });

    c.bench_function("dedup_concurrent", |b| {
        b.iter_with_setup(
            || {
                // Prepare a vector of keys to check
                (0..100)
                    .map(|i| format!("topic1-offset{}", i))
                    .collect::<Vec<String>>()
            },
            |keys| {
                rt.block_on(async {
                    futures_util::future::join_all(keys.into_iter().map(|key| {
                        let dedup_clone = dedup.clone(); // Arc clone
                        async move {
                            black_box(dedup_clone.is_duplicate(&key).await);
                        }
                    }))
                    .await;
                });
            },
        );
    });
}

/// Benchmark deduplication with cache simulation
fn bench_dedup_with_cache(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dedup = rt.block_on(async {
        let dedup = create_test_deduplicator(10000);
        // Pre-populate with entries
        for i in 0..5000 {
            dedup.mark_seen(format!("topic1-offset{}", i)).await;
        }
        dedup
    });

    // Simulate a cache hit ratio scenario
    c.bench_function("dedup_with_cache_simulation", |b| {
        b.iter_with_setup(
            || {
                // Mix of hits (50%) and misses
                (0..100)
                    .map(|i| {
                        if i % 2 == 0 {
                            format!("topic1-offset{}", i / 2) // Will be duplicates (cache hits)
                        } else {
                            format!("new-topic-offset{}", i) // Will be new entries (cache misses)
                        }
                    })
                    .collect::<Vec<String>>()
            },
            |keys| {
                rt.block_on(async {
                    for key in keys {
                        let is_dup = black_box(dedup.is_duplicate(&key).await);
                        if !is_dup {
                            // Mark as seen if it's not a duplicate
                            dedup.mark_seen(key.clone()).await;
                        }
                    }
                });
            },
        );
    });
}

/// Benchmark getting subscribers with exact topic match
fn bench_get_subscribers_exact(c: &mut Criterion) {
    // Create test subscribers
    let subscribers = (0..100)
        .map(|i| {
            Subscriber::new(
                format!("service_{}", i),
                format!("grpc://localhost:{}", 5000 + i),
                vec![format!("topic_{}", i)],
            )
        })
        .collect::<Vec<_>>();

    c.bench_function("get_subscribers_exact", |b| {
        b.iter(|| {
            let topic = "topic_50";
            let matching: Vec<Subscriber> = subscribers
                .iter()
                .filter(|s| {
                    s.topic_patterns
                        .iter()
                        .any(|pattern| matches_topic(topic, pattern))
                })
                .cloned()
                .collect();
            black_box(matching);
        });
    });
}

/// Benchmark getting subscribers with wildcard pattern
fn bench_get_subscribers_wildcard(c: &mut Criterion) {
    // Create test subscribers with wildcard patterns
    let subscribers = (0..100)
        .map(|i| {
            Subscriber::new(
                format!("service_{}", i),
                format!("grpc://localhost:{}", 5000 + i),
                vec![format!("topic_{}*", i / 10)], // Every 10 subscribers share a wildcard
            )
        })
        .collect::<Vec<_>>();

    c.bench_function("get_subscribers_wildcard", |b| {
        b.iter(|| {
            let topic = "topic_5_123";
            let matching: Vec<Subscriber> = subscribers
                .iter()
                .filter(|s| {
                    s.topic_patterns
                        .iter()
                        .any(|pattern| matches_topic(topic, pattern))
                })
                .cloned()
                .collect();
            black_box(matching);
        });
    });
}

/// Benchmark getting subscribers with complex pattern matching
fn bench_get_subscribers_pattern(c: &mut Criterion) {
    // Create test subscribers with various pattern types
    let subscribers = (0..100)
        .map(|i| {
            let patterns = match i % 3 {
                0 => vec![format!("exact_topic_{}", i)],
                1 => vec![format!("prefix_*_{}", i / 10)],
                _ => vec![format!("another_topic"), format!("wildcard_*")],
            };
            Subscriber::new(
                format!("service_{}", i),
                format!("grpc://localhost:{}", 5000 + i),
                patterns,
            )
        })
        .collect::<Vec<_>>();

    c.bench_function("get_subscribers_pattern", |b| {
        b.iter(|| {
            let topic = "prefix_5_something";
            let matching: Vec<Subscriber> = subscribers
                .iter()
                .filter(|s| {
                    s.topic_patterns
                        .iter()
                        .any(|pattern| matches_topic(topic, pattern))
                })
                .cloned()
                .collect();
            black_box(matching);
        });
    });
}

/// Benchmark dispatching to a single subscriber
fn bench_dispatch_single_subscriber(c: &mut Criterion) {
    c.bench_function("dispatch_single_subscriber", |b| {
        b.iter(|| {
            // Simulate dispatching to one subscriber
            let success_count = 1; // Simulated successful dispatch
            black_box(success_count)
        });
    });
}

/// Benchmark dispatching to multiple subscribers
fn bench_dispatch_multiple_subscribers(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch_multiple_subscribers");

    for count in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("subscribers", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut success_count = 0;
                    for _ in 0..count {
                        // Simulate dispatching to each subscriber
                        success_count += 1;
                    }
                    black_box(success_count)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark parallel dispatch performance
fn bench_dispatch_parallel(c: &mut Criterion) {
    use futures_util::future::join_all;

    let rt = Runtime::new().unwrap();

    c.bench_function("dispatch_parallel", |b| {
        b.iter_with_setup(
            || (0..50).collect::<Vec<_>>(), // 50 subscribers to dispatch to
            |subscribers| {
                rt.block_on(async {
                    let tasks = subscribers.into_iter().map(|_| async {
                        // Simulate async dispatch operation
                        tokio::task::yield_now().await;
                        1 // Return success count
                    });

                    let results = join_all(tasks).await;
                    let total_success = results.iter().sum::<i32>();
                    black_box(total_success)
                })
            },
        );
    });
}

/// Benchmark exact topic matching
fn bench_pattern_exact_match(c: &mut Criterion) {
    c.bench_function("pattern_exact_match", |b| {
        b.iter(|| {
            let result = matches_topic(black_box("user.created"), black_box("user.created"));
            black_box(result);
        });
    });
}

/// Benchmark single-level wildcard matching
fn bench_pattern_wildcard_single(c: &mut Criterion) {
    c.bench_function("pattern_wildcard_single", |b| {
        b.iter(|| {
            let result = matches_topic(black_box("user.updated"), black_box("user.?pdated"));
            black_box(result);
        });
    });
}

/// Benchmark multi-level wildcard matching
fn bench_pattern_wildcard_multi(c: &mut Criterion) {
    c.bench_function("pattern_wildcard_multi", |b| {
        b.iter(|| {
            let result = matches_topic(black_box("user.profile.updated"), black_box("user*"));
            black_box(result);
        });
    });
}

/// Benchmark complex pattern combinations
fn bench_pattern_complex(c: &mut Criterion) {
    let patterns = vec![
        "user.created",
        "user.*",
        "order.?",
        "payment.>",
        "notification.*.urgent",
        "system.status.*",
        "log.*.error",
        "event.?.processed",
    ];

    c.bench_function("pattern_complex", |b| {
        b.iter(|| {
            let topic = black_box("user.profile.updated");
            let matches: Vec<bool> = patterns
                .iter()
                .map(|pattern| matches_topic(topic, pattern))
                .collect();
            black_box(matches);
        });
    });
}

/// Benchmark pattern registry lookup with HashMap
fn bench_pattern_registry_lookup(c: &mut Criterion) {
    // Create a registry-like structure with multiple patterns
    let mut registry: HashMap<String, Vec<String>> = HashMap::new();

    // Add patterns for different topics
    for i in 0..100 {
        let topic_base = format!("topic_{}", i % 20); // 20 different base topics
        let pattern = format!("{}_*", topic_base);
        registry
            .entry(pattern)
            .or_insert_with(Vec::new)
            .push(format!("subscriber_{}", i));
    }

    c.bench_function("pattern_registry_lookup", |b| {
        b.iter(|| {
            let topic = black_box("topic_5_message_123");

            // Find matching patterns
            let matching_subscribers: Vec<String> = registry
                .iter()
                .filter(|(pattern, _)| matches_topic(topic, pattern))
                .flat_map(|(_, subscribers)| subscribers.iter().cloned())
                .collect();

            black_box(matching_subscribers);
        });
    });
}

/// Simple in-memory subscriber storage for benchmarking
#[derive(Clone)]
struct InMemorySubscribers {
    subscribers: std::sync::Arc<parking_lot::RwLock<HashMap<Uuid, Subscriber>>>,
}

impl InMemorySubscribers {
    fn new() -> Self {
        Self {
            subscribers: std::sync::Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    async fn add(&self, subscriber: Subscriber) -> Uuid {
        let id = subscriber.id;
        self.subscribers.write().insert(id, subscriber);
        id
    }

    async fn remove(&self, id: Uuid) -> bool {
        self.subscribers.write().remove(&id).is_some()
    }

    async fn get_all_active(&self) -> Vec<Subscriber> {
        self.subscribers.read().values().cloned().collect()
    }
}

/// Benchmark adding subscribers to registry
fn bench_registry_add(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let subscribers = InMemorySubscribers::new();

    c.bench_function("registry_add", |b| {
        b.iter_with_setup(
            || {
                // Create a new subscriber for each iteration
                Subscriber::new(
                    format!("service_{}", Uuid::new_v4()),
                    format!("grpc://localhost:{}", 5000 + (rand::random::<u16>() % 1000)),
                    vec![format!("topic_{}", rand::random::<u32>() % 100)],
                )
            },
            |subscriber| {
                rt.block_on(async {
                    let id = subscribers.add(subscriber).await;
                    black_box(id)
                })
            },
        );
    });
}

/// Benchmark removing subscribers from registry
fn bench_registry_remove(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Pre-populate with subscribers
    let subscribers = rt.block_on(async {
        let subs = InMemorySubscribers::new();
        for _ in 0..1000 {
            let subscriber = Subscriber::new(
                format!("service_{}", Uuid::new_v4()),
                format!("grpc://localhost:{}", 5000 + (rand::random::<u16>() % 1000)),
                vec![format!("topic_{}", rand::random::<u32>() % 100)],
            );
            subs.add(subscriber).await;
        }
        subs
    });

    c.bench_function("registry_remove", |b| {
        b.iter_with_setup(
            || {
                // Get a random subscriber ID to remove
                let all_subs = rt.block_on(subscribers.get_all_active());
                all_subs.first().map(|s| s.id).unwrap_or_else(Uuid::new_v4)
            },
            |id| {
                rt.block_on(async {
                    let result = subscribers.remove(id).await;
                    black_box(result)
                })
            },
        );
    });
}

/// Benchmark getting subscribers by topic
fn bench_registry_get_by_topic(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Pre-populate with subscribers
    let subscribers = rt.block_on(async {
        let subs = InMemorySubscribers::new();
        for i in 0..1000 {
            let subscriber = Subscriber::new(
                format!("service_{}", i),
                format!("grpc://localhost:{}", 5000 + (i % 1000)),
                vec![format!("topic_{}", i % 100), format!("general_*")], // Some with wildcards
            );
            subs.add(subscriber).await;
        }
        subs
    });

    c.bench_function("registry_get_by_topic", |b| {
        b.iter(|| {
            rt.block_on(async {
                let topic = "topic_42";
                let all_subs = subscribers.get_all_active().await;

                // Filter by topic pattern (similar to dispatcher logic)
                let matching: Vec<Subscriber> = all_subs
                    .into_iter()
                    .filter(|s| {
                        s.topic_patterns.iter().any(|pattern| {
                            if pattern.contains('*') {
                                let prefix = pattern.trim_end_matches('*');
                                topic.starts_with(prefix)
                            } else {
                                topic == *pattern
                            }
                        })
                    })
                    .collect();

                black_box(matching)
            })
        });
    });
}

/// Benchmark registry performance with increasing subscriber count
fn bench_registry_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_scale");

    for count in [100, 500, 1000, 5000] {
        group.bench_with_input(
            BenchmarkId::new("subscribers", count),
            &count,
            |b, &count| {
                let rt = Runtime::new().unwrap();

                // Pre-populate with specified number of subscribers
                let subscribers = rt.block_on(async {
                    let subs = InMemorySubscribers::new();
                    for i in 0..count {
                        let subscriber = Subscriber::new(
                            format!("service_{}", i),
                            format!("grpc://localhost:{}", 5000 + (i % 1000)),
                            vec![format!("topic_{}", i % (count / 10))], // Distribute across topics
                        );
                        subs.add(subscriber).await;
                    }
                    subs
                });

                b.iter(|| {
                    rt.block_on(async {
                        // Get all active subscribers
                        let all_subs = subscribers.get_all_active().await;
                        black_box(all_subs.len())
                    })
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    // Deduplication benchmarks
    bench_dedup_check,
    bench_dedup_check_batch,
    bench_dedup_concurrent,
    bench_dedup_with_cache,
    // Dispatcher benchmarks
    bench_get_subscribers_exact,
    bench_get_subscribers_wildcard,
    bench_get_subscribers_pattern,
    bench_dispatch_single_subscriber,
    bench_dispatch_multiple_subscribers,
    bench_dispatch_parallel,
    // Pattern matching benchmarks
    bench_pattern_exact_match,
    bench_pattern_wildcard_single,
    bench_pattern_wildcard_multi,
    bench_pattern_complex,
    bench_pattern_registry_lookup,
    // Registry benchmarks
    bench_registry_add,
    bench_registry_remove,
    bench_registry_get_by_topic,
    bench_registry_scale
);

// Include the new benchmark groups
mod dlq;
mod outbox_manager;
mod outbox_publisher;
mod retry;

criterion_group!(
    inbox_benches,
    // Existing benchmarks
    bench_dedup_check,
    bench_dedup_check_batch,
    bench_dedup_concurrent,
    bench_dedup_with_cache,
    bench_get_subscribers_exact,
    bench_get_subscribers_wildcard,
    bench_get_subscribers_pattern,
    bench_dispatch_single_subscriber,
    bench_dispatch_multiple_subscribers,
    bench_dispatch_parallel,
    bench_pattern_exact_match,
    bench_pattern_wildcard_single,
    bench_pattern_wildcard_multi,
    bench_pattern_complex,
    bench_pattern_registry_lookup,
    bench_registry_add,
    bench_registry_remove,
    bench_registry_get_by_topic,
    bench_registry_scale
);

criterion_main!(
    inbox_benches,
    outbox_manager::outbox_manager_benches,
    outbox_publisher::outbox_publisher_benches,
    retry::retry_benches,
    dlq::dlq_benches
);
