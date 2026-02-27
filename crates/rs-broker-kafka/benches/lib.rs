//! Benchmark suite for rs-broker-kafka
//!
//! This module contains benchmarks for:
//! - Producer operations (send, batch, compression, configs)
//! - Consumer operations (poll, commit, rebalance)
//! - End-to-end latency and throughput
//! - Configuration comparisons
//!
//! Run with: cargo bench --package rs-broker-kafka --bench <name>
//!
//! Note: You need Kafka running to execute benchmarks.
//! Set KAFKA_BOOTSTRAP_SERVERS environment variable if using non-default port.
//!
//! Example benchmarks:
//! - cargo bench --package rs-broker-kafka --bench producer
//! - cargo bench --package rs-broker-kafka --bench consumer
//! - cargo bench --package rs-broker-kafka --bench e2e
//! - cargo bench --package rs-broker-kafka --bench config
