//! rs-broker-db Benchmark Suite
//!
//! Comprehensive benchmarks for database operations in the rs-broker-db crate.
//!
//! ## Running Benchmarks
//!
//! Run all benchmarks:
//! ```bash
//! cargo bench --package rs-broker-db
//! ```
//!
//! Run specific benchmark groups:
//! ```bash
//! cargo bench --package rs-broker-db --bench outbox
//! cargo bench --package rs-broker-db --bench inbox
//! cargo bench --package rs-broker-db --bench subscriber
//! cargo bench --package rs-broker-db --bench pool
//! ```
//!
//! ## Database Setup
//!
//! Set the DATABASE_URL environment variable:
//! ```bash
//! export DATABASE_URL="postgres://postgres:postgres@localhost/postgres"
//! ```
//!
//! Or use docker-compose to start a PostgreSQL instance:
//! ```bash
//! docker-compose up -d postgres
//! ```

mod inbox;
mod outbox;
mod pool;
mod subscriber;

use criterion::Criterion;

fn main() {
    let mut c = Criterion::default()
        .sample_size(20)
        .measurement_time(std::time::Duration::from_secs(5));

    // Run all benchmark groups
    outbox::run_benchmarks(&mut c);
    inbox::run_benchmarks(&mut c);
    subscriber::run_benchmarks(&mut c);
    pool::run_benchmarks(&mut c);

    c.final_summary();
}
