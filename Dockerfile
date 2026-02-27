# =============================================================================
# rs-broker Multi-Stage Dockerfile
# =============================================================================
# Build optimized production images for the rs-broker service
#
# Usage:
#   docker build -t rs-broker:latest .
#   docker build -t rs-broker:postgres --build-arg DATABASE_FEATURE=postgres .
#   docker build -t rs-broker:mysql --build-arg DATABASE_FEATURE=mysql .
#
# Run:
#   docker run -p 8080:8080 -p 50051:50051 -p 9090:9090 rs-broker:latest
# =============================================================================

ARG RUST_VERSION=1.82
ARG ALPINE_VERSION=3.20

# =============================================================================
# Stage 1: Chef - Cargo Chef for dependency caching
# =============================================================================
FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS chef
WORKDIR /app

RUN apk add --no-cache musl-dev cmake make protoc perl git

RUN cargo install cargo-chef --locked

# =============================================================================
# Stage 2: Planner - Analyze and cache dependencies
# =============================================================================
FROM chef AS planner
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo chef prepare --recipe-path recipe.json

# =============================================================================
# Stage 3: Builder - Compile the application
# =============================================================================
FROM chef AS builder
WORKDIR /app

ARG DATABASE_FEATURE=postgres
ARG CARGO_BUILD_FLAGS="--release"

COPY --from=planner /app/recipe.json recipe.json

RUN cargo chef cook --recipe-path recipe.json --features ${DATABASE_FEATURE} ${CARGO_BUILD_FLAGS}

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY proto ./proto
COPY config ./config

RUN cargo build --features ${DATABASE_FEATURE} ${CARGO_BUILD_FLAGS} \
    --bin rs-broker-server

RUN cp target/$(if [ "$CARGO_BUILD_FLAGS" = "--release" ]; then echo "release"; else echo "debug"; fi)/rs-broker-server /app/rs-broker-server

# =============================================================================
# Stage 4: Distroless Runtime - Minimal production image
# =============================================================================
FROM gcr.io/distroless/cc-debian12:latest AS runtime-distroless

WORKDIR /app

COPY --from=builder /app/rs-broker-server /app/

USER nonroot:nonroot

ENV RUST_LOG=info
ENV RS_BROKER_SERVER__HOST=0.0.0.0

EXPOSE 8080 50051 9090

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/app/rs-broker-server", "--help"] || exit 1

ENTRYPOINT ["/app/rs-broker-server"]

# =============================================================================
# Stage 5: Alpine Runtime - Debug/Development image with shell
# =============================================================================
FROM alpine:${ALPINE_VERSION} AS runtime-alpine

WORKDIR /app

RUN apk add --no-cache ca-certificates tzdata

RUN addgroup -g 1000 rsbroker && \
    adduser -u 1000 -G rsbroker -s /bin/sh -D rsbroker

COPY --from=builder /app/rs-broker-server /app/

RUN chown -R rsbroker:rsbroker /app

USER rsbroker:rsbroker

ENV RUST_LOG=info
ENV RS_BROKER_SERVER__HOST=0.0.0.0

EXPOSE 8080 50051 9090

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/health || exit 1

ENTRYPOINT ["/app/rs-broker-server"]

# =============================================================================
# Default to distroless for production
# =============================================================================
FROM runtime-distroless AS production
FROM runtime-alpine AS development
