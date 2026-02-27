//! Kafka consumer benchmarks

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::producer::{BaseProducer, Producer};
use std::time::Duration;

fn get_bootstrap_servers() -> String {
    std::env::var("KAFKA_BOOTSTRAP_SERVERS").unwrap_or_else(|_| "localhost:9092".to_string())
}

fn create_topic_name(prefix: &str) -> String {
    format!("{}_{}", prefix, std::process::id())
}

fn create_consumer(bootstrap_servers: &str, group_id: &str) -> BaseConsumer {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", bootstrap_servers)
        .set("client.id", "benchmark-consumer")
        .set("group.id", group_id)
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .set("session.timeout.ms", "30000")
        .create::<BaseConsumer>()
        .unwrap()
}

fn produce_messages(bootstrap_servers: &str, topic: &str, count: usize) {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", bootstrap_servers)
        .set("client.id", "benchmark-producer-prep")
        .set("acks", "1");

    let producer: rdkafka::producer::BaseProducer = config.create().unwrap();
    let payload = vec![b'x'; 1000];

    for i in 0..count {
        let key = format!("key-{}", i);
        let record = rdkafka::producer::BaseRecord::to(topic)
            .payload(&payload)
            .key(key.as_str());
        let _ = producer.send(record);
    }

    let _ = producer.flush(Duration::from_secs(10));
    drop(producer);
}

fn bench_consumer_poll(c: &mut Criterion) {
    let bootstrap = get_bootstrap_servers();
    let topic = create_topic_name("bench-poll");
    let group = create_topic_name("poll-group");
    produce_messages(&bootstrap, &topic, 1000);

    c.bench_function("consumer_poll", |b| {
        b.iter(|| {
            let consumer = create_consumer(&bootstrap, &group);
            consumer.subscribe(&[&topic]).unwrap();
            consumer.poll(Duration::from_millis(100));
            black_box(())
        })
    });
}

fn bench_consumer_poll_batch(c: &mut Criterion) {
    let bootstrap = get_bootstrap_servers();
    let topic = create_topic_name("bench-batch");
    let group = create_topic_name("batch-group");
    produce_messages(&bootstrap, &topic, 10000);

    c.bench_function("consumer_poll_batch", |b| {
        b.iter(|| {
            let consumer = create_consumer(&bootstrap, &group);
            consumer.subscribe(&[&topic]).unwrap();
            let mut count = 0;
            while count < 100 {
                if consumer.poll(Duration::from_millis(10)).is_some() {
                    count += 1;
                }
            }
            black_box(count)
        })
    });
}

criterion_group!(
    consumer_benches,
    bench_consumer_poll,
    bench_consumer_poll_batch
);
criterion_main!(consumer_benches);
