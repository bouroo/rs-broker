# rs-broker Makefile
# Standard development targets plus benchmark suite.

# Default target
.DEFAULT_GOAL := all

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
.PHONY: build build-debug

build:
	@echo "Building rs-broker (release)..."
	cargo build --release

build-debug:
	@echo "Building rs-broker (debug)..."
	cargo build

# ---------------------------------------------------------------------------
# Test
# ---------------------------------------------------------------------------
.PHONY: test test-integration

test:
	@echo "Running workspace tests..."
	cargo test --workspace

test-integration:
	@echo "Running rs-broker-server integration tests..."
	cargo test --package rs-broker-server --test integration_test

# ---------------------------------------------------------------------------
# Lint & Format
# ---------------------------------------------------------------------------
.PHONY: lint fmt fmt-check

lint:
	@echo "Running clippy..."
	cargo clippy --workspace -- -D warnings

fmt:
	@echo "Formatting workspace..."
	cargo fmt

fmt-check:
	@echo "Checking formatting..."
	cargo fmt -- --check

# ---------------------------------------------------------------------------
# Check
# ---------------------------------------------------------------------------
.PHONY: check clean

check:
	@echo "Running cargo check..."
	cargo check --workspace

clean:
	@echo "Cleaning cargo artifacts..."
	cargo clean

# ---------------------------------------------------------------------------
# Docker
# ---------------------------------------------------------------------------
.PHONY: docker-build docker-build-dev compose-up compose-down compose-logs

docker-build:
	@echo "Building rs-broker Docker image..."
	docker build -t rs-broker:latest .

docker-build-dev:
	@echo "Building rs-broker development Docker image..."
	docker build -t rs-broker:dev --target development .

compose-up:
	@echo "Starting docker-compose services..."
	docker-compose up -d

compose-down:
	@echo "Stopping docker-compose services..."
	docker-compose down -v

compose-logs:
	docker-compose logs -f

# ---------------------------------------------------------------------------
# Proto & Migrations
# ---------------------------------------------------------------------------
.PHONY: proto migrate

proto:
	@echo "Building rs-broker-proto (regenerates protobuf)..."
	cargo build -p rs-broker-proto

migrate:
	@echo "Run database migrations with:"
	@echo "  cargo run -p rs-broker-db --bin migrate"
	@echo "or apply SQL files from migrations/ in order against your target database."

# ---------------------------------------------------------------------------
# Aggregate
# ---------------------------------------------------------------------------
.PHONY: all

all: check lint fmt-check test
	@echo "All checks passed."

# ---------------------------------------------------------------------------
# Benchmarks
# ---------------------------------------------------------------------------
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

# Alias target
bench: bench-all
