//! End-to-end Kafka benchmarks

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::message::Message;
use rdkafka::producer::{BaseProducer, Producer};
use std::time::{Duration, Instant};

fn get_bootstrap_servers() -> String {
    std::env::var("KAFKA_BOOTSTRAP_SERVERS").unwrap_or_else(|_| "localhost:9092".to_string())
}

fn create_topic_name(prefix: &str) -> String {
    format!("{}_{}", prefix, std::process::id())
}

fn create_producer(bootstrap_servers: &str) -> BaseProducer {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", bootstrap_servers)
        .set("client.id", "benchmark-producer")
        .set("acks", "1")
        .create::<BaseProducer>()
        .unwrap()
}

fn create_consumer(bootstrap_servers: &str, group_id: &str) -> BaseConsumer {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", bootstrap_servers)
        .set("client.id", "benchmark-consumer")
        .set("group.id", group_id)
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .create::<BaseConsumer>()
        .unwrap()
}

fn generate_payload(size: usize) -> Vec<u8> {
    vec![b'x'; size]
}

fn bench_e2e_latency(c: &mut Criterion) {
    let bootstrap = get_bootstrap_servers();
    let topic = create_topic_name("e2e-latency");
    let group = create_topic_name("e2e-latency-group");

    c.bench_function("e2e_latency", |b| {
        b.iter(|| {
            let payload = generate_payload(1000);

            let produce_time = Instant::now();
            let producer = create_producer(&bootstrap);
            producer
                .send(
                    rdkafka::producer::BaseRecord::to(&topic)
                        .payload(&payload)
                        .key("bench-key"),
                )
                .unwrap();
            let _ = producer.flush(Duration::from_secs(5));
            drop(producer);

            let consumer = create_consumer(&bootstrap, &group);
            consumer.subscribe(&[&topic]).unwrap();

            let mut consumed = false;
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(10) {
                if let Some(Ok(msg)) = consumer.poll(Duration::from_millis(100)) {
                    if msg.payload().is_some() {
                        let latency = produce_time.elapsed();
                        consumed = true;
                        black_box(latency);
                        break;
                    }
                }
            }
            if !consumed {
                black_box(Duration::from_secs(0));
            }
        })
    });
}

fn bench_e2e_throughput(c: &mut Criterion) {
    let bootstrap = get_bootstrap_servers();
    let topic = create_topic_name("e2e-throughput");
    let group = create_topic_name("e2e-throughput-group");

    c.bench_function("e2e_throughput", |b| {
        b.iter(|| {
            let payload = generate_payload(1000);

            let producer = create_producer(&bootstrap);
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

            let consumer = create_consumer(&bootstrap, &group);
            consumer.subscribe(&[&topic]).unwrap();

            let mut consumed = 0;
            let start = Instant::now();
            while consumed < 100 && start.elapsed() < Duration::from_secs(20) {
                if consumer.poll(Duration::from_millis(100)).is_some() {
                    consumed += 1;
                }
            }

            let elapsed = start.elapsed();
            let throughput = 100 as f64 / elapsed.as_secs_f64();
            black_box(throughput)
        })
    });
}

criterion_group!(e2e_benches, bench_e2e_latency, bench_e2e_throughput);
criterion_main!(e2e_benches);
