// Benchmark suite for rs-broker-server gRPC service handlers
// Run with: cargo bench --package rs-broker-server

mod consumer_service;
mod integration;
mod publish;
mod status;

use criterion::{criterion_group, criterion_main};

criterion_group!(
    benches,
    publish::publish_benches,
    status::status_benches,
    consumer_service::consumer_benches,
    integration::integration_benches,
);
criterion_main!(benches);
