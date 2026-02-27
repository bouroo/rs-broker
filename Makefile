# Benchmark targets for rs-broker
# Run with: make bench, make bench-core, make bench-db, etc.

.PHONY: bench bench-all bench-core bench-db bench-kafka bench-server

# Run all benchmarks
bench-all:
	@echo "Running all rs-broker benchmarks..."
	cargo bench --package rs-broker-core
	cargo bench --package rs-broker-db
	cargo bench --package rs-broker-kafka
	cargo bench --package rs-broker-server

# Run core benchmarks
bench-core:
	@echo "Running rs-broker-core benchmarks..."
	cargo bench --package rs-broker-core

# Run database benchmarks
bench-db:
	@echo "Running rs-broker-db benchmarks..."
	cargo bench --package rs-broker-db

# Run Kafka benchmarks
bench-kafka:
	@echo "Running rs-broker-kafka benchmarks..."
	cargo bench --package rs-broker-kafka

# Run server benchmarks
bench-server:
	@echo "Running rs-broker-server benchmarks..."
	cargo bench --package rs-broker-server

# Default target
bench: bench-all
