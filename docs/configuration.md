# rs-broker Configuration

## Overview

This document defines the configuration schema for rs-broker, supporting mode switching, Kafka settings, database configuration, gRPC settings, and retry policies.

## Configuration Sources

Configuration is loaded in the following order (later sources override earlier):

1. `config/default.toml` - Base configuration
2. `config/{environment}.toml` - Environment-specific config
3. Environment variables with `RS_BROKER_` prefix
4. Command-line arguments

## Configuration Schema

### Complete Configuration Structure

```toml
# config/default.toml

[server]
# Server mode: "producer", "consumer", or "both"
mode = "both"

# HTTP server settings
host = "0.0.0.0"
http_port = 8080

# gRPC server settings
grpc_port = 50051

# Graceful shutdown timeout in seconds
shutdown_timeout_secs = 30

# Request timeout in seconds
request_timeout_secs = 30

[database]
# Database type: "postgres" or "mysql"
type = "postgres"

# Connection settings
host = "localhost"
port = 5432
username = "rsbroker"
password = "${RS_BROKER_DB_PASSWORD}"
database = "rsbroker"

# Connection pool settings
max_connections = 25
min_connections = 5
connect_timeout_secs = 30
idle_timeout_secs = 300
max_lifetime_secs = 1800

# Run migrations on startup
auto_migrate = true

[kafka]
# Kafka broker addresses (comma-separated)
brokers = "localhost:9092"

# Consumer group ID (for consumer mode)
consumer_group_id = "rs-broker-consumer"

# Client ID for logging
client_id = "rs-broker"

# Security settings
security_protocol = "plaintext"  # plaintext, ssl, sasl_plaintext, sasl_ssl
sasl_mechanism = "PLAIN"         # PLAIN, SCRAM-SHA-256, SCRAM-SHA-512
sasl_username = "${RS_BROKER_KAFKA_USERNAME}"
sasl_password = "${RS_BROKER_KAFKA_PASSWORD}"

# SSL settings
ssl_ca_location = ""
ssl_certificate_location = ""
ssl_key_location = ""

# Producer settings
[ kafka.producer]
# Message timeout in milliseconds
message_timeout_ms = 30000
# Request timeout in milliseconds
request_timeout_ms = 30000
# Acks: 0, 1, or -1 (all)
acks = -1
# Enable idempotent producer
enable_idempotence = true
# Compression type: none, gzip, snappy, lz4, zstd
compression_type = "snappy"
# Batch size in bytes
batch_size = 16384
# Linger time in milliseconds
linger_ms = 5

# Consumer settings
[kafka.consumer]
# Auto-commit offsets
enable_auto_commit = false
# Auto-commit interval in milliseconds
auto_commit_interval_ms = 5000
# Session timeout in milliseconds
session_timeout_ms = 30000
# Heartbeat interval in milliseconds
heartbeat_interval_ms = 10000
# Max poll records
max_poll_records = 500
# Auto offset reset: earliest, latest, none
auto_offset_reset = "latest"
# Topics to subscribe (for consumer mode)
topics = ["events", "orders"]

[grpc]
# Enable gRPC reflection
enable_reflection = true

# Max message size in bytes
max_message_size = 4194304  # 4MB

# Keep-alive settings
keepalive_time_secs = 60
keepalive_timeout_secs = 20

# TLS settings
enable_tls = false
cert_path = ""
key_path = ""
ca_path = ""

[retry]
# Default retry policy
max_retries = 5
# Initial delay in milliseconds
initial_delay_ms = 1000
# Multiplier for exponential backoff
multiplier = 2.0
# Maximum delay in milliseconds
max_delay_ms = 60000
# Jitter factor (0.0 to 1.0)
jitter_factor = 0.1

# Retry scheduling interval in seconds
scheduler_interval_secs = 5
# Maximum messages to process per schedule
batch_size = 100

[dlq]
# Enable Dead Letter Queue
enabled = true
# DLQ topic suffix
topic_suffix = ".dlq"
# Log DLQ messages
log_messages = true

[outbox]
# Polling interval in milliseconds
poll_interval_ms = 100
# Batch size for publishing
batch_size = 100
# Maximum concurrent publishes
max_concurrent = 10
# Message retention in days (for cleanup)
retention_days = 7

[inbox]
# Maximum concurrent deliveries
max_concurrent_deliveries = 10
# Delivery timeout in milliseconds
delivery_timeout_ms = 30000
# Message retention in days (for cleanup)
retention_days = 7

[subscriber]
# Default delivery config
default_timeout_ms = 30000
default_max_retries = 3

[logging]
# Log level: trace, debug, info, warn, error
level = "info"
# Log format: json, pretty
format = "json"
# Include span traces
include_spans = true

[metrics]
# Enable Prometheus metrics
enabled = true
# Metrics endpoint path
path = "/metrics"
# Prometheus port
port = 9090

[tracing]
# Enable distributed tracing
enabled = false
# Service name
service_name = "rs-broker"
# OTLP endpoint
otlp_endpoint = "http://localhost:4317"
# Sample ratio (0.0 to 1.0)
sample_ratio = 1.0

[health]
# Health check endpoint path
path = "/health"
# Enable liveness probe
liveness_enabled = true
# Enable readiness probe
readiness_enabled = true
```

## Environment-Specific Configurations

### Development Configuration

```toml
# config/development.toml

[server]
mode = "both"
host = "127.0.0.1"
http_port = 8080
grpc_port = 50051

[database]
type = "postgres"
host = "localhost"
port = 5432
username = "rsbroker"
password = "devpassword"
database = "rsbroker_dev"
auto_migrate = true

[kafka]
brokers = "localhost:9092"
security_protocol = "plaintext"

[retry]
max_retries = 3
initial_delay_ms = 500

[logging]
level = "debug"
format = "pretty"

[metrics]
enabled = false

[tracing]
enabled = false
```

### Production Configuration

```toml
# config/production.toml

[server]
mode = "both"
host = "0.0.0.0"
http_port = 8080
grpc_port = 50051
shutdown_timeout_secs = 60

[database]
type = "postgres"
host = "${RS_BROKER_DB_HOST}"
port = 5432
username = "${RS_BROKER_DB_USERNAME}"
password = "${RS_BROKER_DB_PASSWORD}"
database = "rsbroker"
max_connections = 50
min_connections = 10
auto_migrate = false

[kafka]
brokers = "${RS_BROKER_KAFKA_BROKERS}"
security_protocol = "sasl_ssl"
sasl_mechanism = "SCRAM-SHA-256"
sasl_username = "${RS_BROKER_KAFKA_USERNAME}"
sasl_password = "${RS_BROKER_KAFKA_PASSWORD}"

[kafka.producer]
acks = -1
enable_idempotence = true
compression_type = "zstd"

[kafka.consumer]
enable_auto_commit = false
auto_offset_reset = "latest"

[retry]
max_retries = 5
initial_delay_ms = 1000
multiplier = 2.0
max_delay_ms = 120000

[dlq]
enabled = true
log_messages = true

[logging]
level = "info"
format = "json"

[metrics]
enabled = true
port = 9090

[tracing]
enabled = true
otlp_endpoint = "${RS_BROKER_OTLP_ENDPOINT}"
sample_ratio = 0.1
```

### Test Configuration

```toml
# config/test.toml

[server]
mode = "both"
host = "127.0.0.1"
http_port = 18080
grpc_port = 15051

[database]
type = "postgres"
host = "localhost"
port = 5433
username = "test"
password = "test"
database = "rsbroker_test"
auto_migrate = true

[kafka]
brokers = "localhost:9093"
consumer_group_id = "rs-broker-test"

[retry]
max_retries = 1
initial_delay_ms = 100

[logging]
level = "debug"
format = "pretty"

[metrics]
enabled = false

[tracing]
enabled = false
```

## Environment Variables

All configuration values can be overridden via environment variables using the prefix `RS_BROKER_`:

| Environment Variable | Config Path | Example |
|---------------------|-------------|---------|
| `RS_BROKER_SERVER_MODE` | `server.mode` | `producer` |
| `RS_BROKER_SERVER_HTTP_PORT` | `server.http_port` | `8080` |
| `RS_BROKER_DATABASE_HOST` | `database.host` | `db.example.com` |
| `RS_BROKER_DATABASE_PASSWORD` | `database.password` | `secret123` |
| `RS_BROKER_KAFKA_BROKERS` | `kafka.brokers` | `kafka1:9092,kafka2:9092` |
| `RS_BROKER_KAFKA_USERNAME` | `kafka.sasl_username` | `user` |
| `RS_BROKER_KAFKA_PASSWORD` | `kafka.sasl_password` | `password` |

## Configuration Types (Rust)

```rust
// crates/rs-broker-config/src/settings.rs

use serde::Deserialize;
use config::Config;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub kafka: KafkaConfig,
    pub grpc: GrpcConfig,
    pub retry: RetryConfig,
    pub dlq: DlqConfig,
    pub outbox: OutboxConfig,
    pub inbox: InboxConfig,
    pub subscriber: SubscriberConfig,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
    pub tracing: TracingConfig,
    pub health: HealthConfig,
}

impl Settings {
    pub fn load() -> Result<Self, config::ConfigError> {
        let config = Config::builder()
            .add_source(config::File::with_name("config/default"))
            .add_source(
                config::Environment::with_prefix("RS_BROKER")
                    .separator("__")
                    .try_parsing(true)
            )
            .build()?;
        
        config.try_deserialize()
    }
}
```

### Server Configuration

```rust
// crates/rs-broker-config/src/server.rs

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Server mode: producer, consumer, or both
    pub mode: ServerMode,
    
    /// HTTP server host
    pub host: String,
    
    /// HTTP server port
    pub http_port: u16,
    
    /// gRPC server port
    pub grpc_port: u16,
    
    /// Graceful shutdown timeout in seconds
    pub shutdown_timeout_secs: u64,
    
    /// Request timeout in seconds
    pub request_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServerMode {
    Producer,
    Consumer,
    Both,
}
```

### Database Configuration

```rust
// crates/rs-broker-config/src/database.rs

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Database type: postgres or mysql
    #[serde(rename = "type")]
    pub db_type: DatabaseType,
    
    /// Database host
    pub host: String,
    
    /// Database port
    pub port: u16,
    
    /// Database username
    pub username: String,
    
    /// Database password
    pub password: String,
    
    /// Database name
    pub database: String,
    
    /// Maximum connections in pool
    pub max_connections: u32,
    
    /// Minimum connections in pool
    pub min_connections: u32,
    
    /// Connection timeout in seconds
    pub connect_timeout_secs: u64,
    
    /// Idle connection timeout in seconds
    pub idle_timeout_secs: u64,
    
    /// Maximum connection lifetime in seconds
    pub max_lifetime_secs: u64,
    
    /// Run migrations on startup
    pub auto_migrate: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseType {
    Postgres,
    MySql,
}

impl DatabaseConfig {
    pub fn connection_string(&self) -> String {
        match self.db_type {
            DatabaseType::Postgres => {
                format!(
                    "postgres://{}:{}@{}:{}/{}",
                    self.username, self.password, self.host, self.port, self.database
                )
            }
            DatabaseType::MySql => {
                format!(
                    "mysql://{}:{}@{}:{}/{}",
                    self.username, self.password, self.host, self.port, self.database
                )
            }
        }
    }
}
```

### Kafka Configuration

```rust
// crates/rs-broker-config/src/kafka.rs

#[derive(Debug, Clone, Deserialize)]
pub struct KafkaConfig {
    /// Kafka broker addresses (comma-separated)
    pub brokers: String,
    
    /// Consumer group ID
    pub consumer_group_id: String,
    
    /// Client ID
    pub client_id: String,
    
    /// Security protocol
    pub security_protocol: SecurityProtocol,
    
    /// SASL mechanism
    pub sasl_mechanism: SaslMechanism,
    
    /// SASL username
    pub sasl_username: Option<String>,
    
    /// SASL password
    pub sasl_password: Option<String>,
    
    /// SSL CA location
    pub ssl_ca_location: Option<String>,
    
    /// SSL certificate location
    pub ssl_certificate_location: Option<String>,
    
    /// SSL key location
    pub ssl_key_location: Option<String>,
    
    /// Producer settings
    pub producer: ProducerConfig,
    
    /// Consumer settings
    pub consumer: ConsumerConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityProtocol {
    Plaintext,
    Ssl,
    SaslPlaintext,
    SaslSsl,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum SaslMechanism {
    Plain,
    ScramSha256,
    ScramSha512,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProducerConfig {
    pub message_timeout_ms: u32,
    pub request_timeout_ms: u32,
    pub acks: i16,
    pub enable_idempotence: bool,
    pub compression_type: CompressionType,
    pub batch_size: usize,
    pub linger_ms: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsumerConfig {
    pub enable_auto_commit: bool,
    pub auto_commit_interval_ms: u32,
    pub session_timeout_ms: u32,
    pub heartbeat_interval_ms: u32,
    pub max_poll_records: usize,
    pub auto_offset_reset: AutoOffsetReset,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CompressionType {
    None,
    Gzip,
    Snappy,
    Lz4,
    Zstd,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AutoOffsetReset {
    Earliest,
    Latest,
    None,
}
```

### Retry Configuration

```rust
// crates/rs-broker-config/src/retry.rs

use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    /// Maximum retry attempts
    pub max_retries: u32,
    
    /// Initial delay in milliseconds
    pub initial_delay_ms: u64,
    
    /// Exponential backoff multiplier
    pub multiplier: f64,
    
    /// Maximum delay in milliseconds
    pub max_delay_ms: u64,
    
    /// Jitter factor (0.0 to 1.0)
    pub jitter_factor: f64,
    
    /// Scheduler interval in seconds
    pub scheduler_interval_secs: u64,
    
    /// Batch size for processing
    pub batch_size: usize,
}

impl RetryConfig {
    pub fn calculate_delay(&self, retry_count: u32) -> Duration {
        use rand::Rng;
        
        let base_delay = self.initial_delay_ms as f64 
            * self.multiplier.powi(retry_count as i32);
        let capped_delay = base_delay.min(self.max_delay_ms as f64);
        
        // Apply jitter
        let jitter = if self.jitter_factor > 0.0 {
            let mut rng = rand::thread_rng();
            let jitter_range = capped_delay * self.jitter_factor;
            rng.gen_range(0.0..jitter_range)
        } else {
            0.0
        };
        
        Duration::from_millis((capped_delay + jitter) as u64)
    }
}
```

## Mode-Specific Configuration

### Producer Mode Only

```toml
[server]
mode = "producer"

[kafka.consumer]
# Consumer settings are ignored in producer mode

[outbox]
poll_interval_ms = 100
batch_size = 100

# Inbox settings are ignored
```

### Consumer Mode Only

```toml
[server]
mode = "consumer"

[kafka.producer]
# Producer settings are ignored in consumer mode

[kafka.consumer]
topics = ["orders", "payments", "events"]

[inbox]
max_concurrent_deliveries = 20

# Outbox settings are ignored
```

### Dual Mode (Both)

```toml
[server]
mode = "both"

# All settings are active
[kafka.producer]
acks = -1

[kafka.consumer]
topics = ["orders", "payments"]

[outbox]
poll_interval_ms = 100

[inbox]
max_concurrent_deliveries = 10
```

## Configuration Validation

```rust
impl Settings {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        
        // Validate server config
        if self.server.http_port == self.server.grpc_port {
            errors.push("HTTP and gRPC ports must be different".to_string());
        }
        
        // Validate database config
        if self.database.max_connections < self.database.min_connections {
            errors.push("max_connections must be >= min_connections".to_string());
        }
        
        // Validate retry config
        if self.retry.multiplier < 1.0 {
            errors.push("retry multiplier must be >= 1.0".to_string());
        }
        
        if self.retry.jitter_factor < 0.0 || self.retry.jitter_factor > 1.0 {
            errors.push("jitter_factor must be between 0.0 and 1.0".to_string());
        }
        
        // Validate mode-specific config
        match self.server.mode {
            ServerMode::Consumer => {
                if self.kafka.consumer.topics.is_empty() {
                    errors.push("consumer mode requires at least one topic".to_string());
                }
            }
            _ => {}
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
```

## Docker/Kubernetes Configuration

### ConfigMap Example

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: rs-broker-config
data:
  default.toml: |
    [server]
    mode = "both"
    host = "0.0.0.0"
    http_port = 8080
    grpc_port = 50051
    
    [database]
    type = "postgres"
    host = "postgres-service"
    port = 5432
    database = "rsbroker"
    
    [kafka]
    brokers = "kafka-service:9092"
    security_protocol = "plaintext"
    
    [logging]
    level = "info"
    format = "json"
    
    [metrics]
    enabled = true
    port = 9090
```

### Secret Example

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: rs-broker-secrets
type: Opaque
stringData:
  RS_BROKER__DATABASE__PASSWORD: "supersecret"
  RS_BROKER__KAFKA__SASL_USERNAME: "kafka-user"
  RS_BROKER__KAFKA__SASL_PASSWORD: "kafka-password"
```
