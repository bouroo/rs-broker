//! Configuration comparison benchmarks

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{BaseProducer, Producer};
use std::time::Duration;

fn get_bootstrap_servers() -> String {
    std::env::var("KAFKA_BOOTSTRAP_SERVERS").unwrap_or_else(|_| "localhost:9092".to_string())
}

fn create_topic_name(prefix: &str) -> String {
    format!("{}_{}", prefix, std::process::id())
}

fn generate_payload(size: usize) -> Vec<u8> {
    vec![b'x'; size]
}

fn bench_producer_configs(c: &mut Criterion) {
    let bootstrap = get_bootstrap_servers();
    let payload = generate_payload(1000);

    c.bench_function("producer_config_balanced", |b| {
        let topic = create_topic_name("config-balanced");
        b.iter(|| {
            let mut config = ClientConfig::new();
            config
                .set("bootstrap.servers", &bootstrap)
                .set("client.id", "benchmark-producer")
                .set("acks", "1")
                .set("linger.ms", "5");

            let producer: BaseProducer = config.create().unwrap();

            for i in 0..100 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key(&format!("key-{}", i)),
                    )
                    .unwrap();
            }

            let _ = producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });

    c.bench_function("producer_config_high_throughput", |b| {
        let topic = create_topic_name("config-throughput");
        b.iter(|| {
            let mut config = ClientConfig::new();
            config
                .set("bootstrap.servers", &bootstrap)
                .set("client.id", "benchmark-producer")
                .set("acks", "1")
                .set("linger.ms", "20")
                .set("batch.size", "65536")
                .set("compression.type", "lz4");

            let producer: BaseProducer = config.create().unwrap();

            for i in 0..100 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key(&format!("key-{}", i)),
                    )
                    .unwrap();
            }

            let _ = producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });
}

fn bench_idempotent_producer(c: &mut Criterion) {
    let bootstrap = get_bootstrap_servers();
    let payload = generate_payload(1000);

    c.bench_function("idempotent_false", |b| {
        let topic = create_topic_name("idempotent-false");
        b.iter(|| {
            let mut config = ClientConfig::new();
            config
                .set("bootstrap.servers", &bootstrap)
                .set("client.id", "benchmark-producer")
                .set("acks", "all");

            let producer: BaseProducer = config.create().unwrap();

            for i in 0..50 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key(&format!("key-{}", i)),
                    )
                    .unwrap();
            }

            let _ = producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });

    c.bench_function("idempotent_true", |b| {
        let topic = create_topic_name("idempotent-true");
        b.iter(|| {
            let mut config = ClientConfig::new();
            config
                .set("bootstrap.servers", &bootstrap)
                .set("client.id", "benchmark-producer")
                .set("acks", "all")
                .set("enable.idempotence", "true");

            let producer: BaseProducer = config.create().unwrap();

            for i in 0..50 {
                producer
                    .send(
                        rdkafka::producer::BaseRecord::to(&topic)
                            .payload(&payload)
                            .key(&format!("key-{}", i)),
                    )
                    .unwrap();
            }

            let _ = producer.flush(Duration::from_secs(10));
            drop(producer);
            black_box(())
        });
    });
}

criterion_group!(
    config_benches,
    bench_producer_configs,
    bench_idempotent_producer
);
criterion_main!(config_benches);
