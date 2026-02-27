//! Kafka producer benchmarks
//!
//! Benchmarks for producer operations including send, batch send, compression, and configuration options.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{BaseProducer, Producer};
use std::time::Duration;

/// Get Kafka bootstrap servers
fn get_bootstrap_servers() -> String {
    std::env::var("KAFKA_BOOTSTRAP_SERVERS").unwrap_or_else(|_| "localhost:9092".to_string())
}

/// Create a topic name
fn create_topic_name(prefix: &str) -> String {
    format!("{}_{}", prefix, std::process::id())
}

/// Create a producer with the given configuration
fn create_producer(bootstrap_servers: &str, additional_config: &[(&str, &str)]) -> BaseProducer {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", bootstrap_servers)
        .set("client.id", "benchmark-producer")
        .set("acks", "1")
        .set("linger.ms", "5")
        .set("batch.size", "16384")
        .set("request.timeout.ms", "30000")
        .set("message.timeout.ms", "30000");

    for (key, value) in additional_config {
        config.set(key.to_string(), value.to_string());
    }

    config.create::<BaseProducer>().unwrap()
}

/// Generate a test message payload
fn generate_payload(size: usize) -> Vec<u8> {
    vec![b'x'; size]
}

/// Benchmark: Single message send latency
fn bench_producer_send(c: &mut Criterion) {
    let bootstrap_servers = get_bootstrap_servers();
    let topic = create_topic_name("bench-producer-send");

    c.bench_function("producer_send_1k", |b| {
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[]);
            let payload = generate_payload(1000);

            producer
                .send(
                    rdkafka::producer::BaseRecord::to(&topic)
                        .payload(&payload)
                        .key("bench-key"),
                )
                .unwrap();

            producer.flush(Duration::from_secs(5));
            drop(producer);
            black_box(())
        });
    });
}

/// Benchmark: Batch message send throughput
fn bench_producer_send_batch(c: &mut Criterion) {
    let bootstrap_servers = get_bootstrap_servers();
    let topic = create_topic_name("bench-producer-batch");

    c.bench_function("producer_send_batch_100", |b| {
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[]);
            let payload = generate_payload(1000);

            for _ in 0..100 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key("bench-key"),
                    )
                    .unwrap();
            }

            producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });
}

/// Benchmark: Compression comparison
fn bench_producer_compression(c: &mut Criterion) {
    let bootstrap_servers = get_bootstrap_servers();
    let payload = generate_payload(10000);

    c.bench_function("producer_compression_none", |b| {
        let topic = create_topic_name("bench-comp-none");
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[]);

            for _ in 0..100 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key("bench-key"),
                    )
                    .unwrap();
            }

            producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });

    c.bench_function("producer_compression_lz4", |b| {
        let topic = create_topic_name("bench-comp-lz4");
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[("compression.type", "lz4")]);

            for _ in 0..100 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key("bench-key"),
                    )
                    .unwrap();
            }

            producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });
}

/// Benchmark: ACKS configuration
fn bench_producer_acks(c: &mut Criterion) {
    let bootstrap_servers = get_bootstrap_servers();
    let payload = generate_payload(1000);

    c.bench_function("producer_acks_0", |b| {
        let topic = create_topic_name("bench-acks-0");
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[("acks", "0")]);

            for _ in 0..50 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key("bench-key"),
                    )
                    .unwrap();
            }

            producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });

    c.bench_function("producer_acks_1", |b| {
        let topic = create_topic_name("bench-acks-1");
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[("acks", "1")]);

            for _ in 0..50 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key("bench-key"),
                    )
                    .unwrap();
            }

            producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });
}

/// Benchmark: Linger.ms configuration
fn bench_producer_linger(c: &mut Criterion) {
    let bootstrap_servers = get_bootstrap_servers();
    let payload = generate_payload(1000);

    c.bench_function("producer_linger_0", |b| {
        let topic = create_topic_name("bench-linger-0");
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[("linger.ms", "0")]);

            for _ in 0..100 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key("bench-key"),
                    )
                    .unwrap();
            }

            producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });

    c.bench_function("producer_linger_20", |b| {
        let topic = create_topic_name("bench-linger-20");
        b.iter(|| {
            let producer = create_producer(&bootstrap_servers, &[("linger.ms", "20")]);

            for _ in 0..100 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key("bench-key"),
                    )
                    .unwrap();
            }

            producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });
}

criterion_group!(
    name = producer_benches;
    config = Criterion::default().sample_size(10);
    targets =
        bench_producer_send,
        bench_producer_send_batch,
        bench_producer_compression,
        bench_producer_acks,
        bench_producer_linger,
);

criterion_main!(producer_benches);
