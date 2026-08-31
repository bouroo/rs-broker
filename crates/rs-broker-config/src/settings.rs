//! Main settings structure for rs-broker

use serde::Deserialize;
use thiserror::Error;

use super::{DatabaseConfig, GrpcConfig, KafkaConfig, RetryConfig, ServerConfig};

/// Logging configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Log format: `json` or `pretty`.
    #[serde(default = "default_logging_format")]
    pub format: String,
}

fn default_logging_format() -> String {
    "json".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: default_logging_format(),
        }
    }
}

/// Metrics configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MetricsConfig {
    /// Enable Prometheus metrics.
    #[serde(default = "default_metrics_enabled")]
    pub enabled: bool,

    /// Metrics path.
    #[serde(default = "default_metrics_path")]
    pub path: String,

    /// Metrics server port.
    #[serde(default = "default_metrics_port")]
    pub port: u16,
}

fn default_metrics_enabled() -> bool {
    true
}

fn default_metrics_path() -> String {
    "/metrics".to_string()
}

fn default_metrics_port() -> u16 {
    9090
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: default_metrics_enabled(),
            path: default_metrics_path(),
            port: default_metrics_port(),
        }
    }
}

/// Outbox configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct OutboxConfig {
    /// Poll interval in milliseconds.
    #[serde(default = "default_outbox_poll_interval_ms")]
    pub poll_interval_ms: u64,

    /// Batch size.
    #[serde(default = "default_outbox_batch_size")]
    pub batch_size: i64,

    /// Maximum concurrent publish tasks.
    #[serde(default = "default_outbox_max_concurrent")]
    pub max_concurrent: usize,

    /// Retention days for cleanup.
    #[serde(default = "default_outbox_retention_days")]
    pub retention_days: u32,
}

fn default_outbox_poll_interval_ms() -> u64 {
    100
}

fn default_outbox_batch_size() -> i64 {
    100
}

fn default_outbox_max_concurrent() -> usize {
    10
}

fn default_outbox_retention_days() -> u32 {
    7
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: default_outbox_poll_interval_ms(),
            batch_size: default_outbox_batch_size(),
            max_concurrent: default_outbox_max_concurrent(),
            retention_days: default_outbox_retention_days(),
        }
    }
}

/// Inbox configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct InboxConfig {
    /// Maximum concurrent deliveries.
    #[serde(default = "default_inbox_max_concurrent_deliveries")]
    pub max_concurrent_deliveries: usize,

    /// Delivery timeout in milliseconds.
    #[serde(default = "default_inbox_delivery_timeout_ms")]
    pub delivery_timeout_ms: u64,

    /// Retention days for cleanup.
    #[serde(default = "default_inbox_retention_days")]
    pub retention_days: u32,
}

fn default_inbox_max_concurrent_deliveries() -> usize {
    10
}

fn default_inbox_delivery_timeout_ms() -> u64 {
    30000
}

fn default_inbox_retention_days() -> u32 {
    7
}

impl Default for InboxConfig {
    fn default() -> Self {
        Self {
            max_concurrent_deliveries: default_inbox_max_concurrent_deliveries(),
            delivery_timeout_ms: default_inbox_delivery_timeout_ms(),
            retention_days: default_inbox_retention_days(),
        }
    }
}

/// DLQ configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DlqConfig {
    /// Enable dead-letter queue.
    #[serde(default = "default_dlq_enabled")]
    pub enabled: bool,

    /// Topic suffix appended to original topic for DLQ.
    #[serde(default = "default_dlq_topic_suffix")]
    pub topic_suffix: String,

    /// Log DLQ messages.
    #[serde(default = "default_dlq_log_messages")]
    pub log_messages: bool,
}

fn default_dlq_enabled() -> bool {
    true
}

fn default_dlq_topic_suffix() -> String {
    ".dlq".to_string()
}

fn default_dlq_log_messages() -> bool {
    true
}

impl Default for DlqConfig {
    fn default() -> Self {
        Self {
            enabled: default_dlq_enabled(),
            topic_suffix: default_dlq_topic_suffix(),
            log_messages: default_dlq_log_messages(),
        }
    }
}

/// Subscriber configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SubscriberConfig {
    /// Default delivery timeout in milliseconds.
    #[serde(default = "default_subscriber_timeout_ms")]
    pub default_timeout_ms: u64,

    /// Default maximum retries for delivery.
    #[serde(default = "default_subscriber_max_retries")]
    pub default_max_retries: u32,
}

fn default_subscriber_timeout_ms() -> u64 {
    30000
}

fn default_subscriber_max_retries() -> u32 {
    3
}

impl Default for SubscriberConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: default_subscriber_timeout_ms(),
            default_max_retries: default_subscriber_max_retries(),
        }
    }
}

/// Main application settings
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Settings {
    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Database configuration
    #[serde(default)]
    pub database: DatabaseConfig,

    /// Kafka configuration
    #[serde(default)]
    pub kafka: KafkaConfig,

    /// gRPC configuration
    #[serde(default)]
    pub grpc: GrpcConfig,

    /// Retry configuration
    #[serde(default)]
    pub retry: RetryConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Metrics configuration
    #[serde(default)]
    pub metrics: MetricsConfig,

    /// Outbox configuration
    #[serde(default)]
    pub outbox: OutboxConfig,

    /// Inbox configuration
    #[serde(default)]
    pub inbox: InboxConfig,

    /// DLQ configuration
    #[serde(default)]
    pub dlq: DlqConfig,

    /// Subscriber configuration
    #[serde(default)]
    pub subscriber: SubscriberConfig,
}

/// Error type for configuration operations
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl Settings {
    /// Load settings from a configuration directory.
    ///
    /// Sources layer in order (later wins), matching the documented
    /// configuration contract:
    ///
    /// 1. `{path}/default.toml` — required base configuration
    /// 2. `{path}/{RS_BROKER_ENV}.toml` — optional environment overlay
    ///    (`RS_BROKER_ENV`, default `development`)
    /// 3. `RS_BROKER_`-prefixed environment variables using `__` as the path
    ///    separator, e.g. `RS_BROKER_SERVER__HTTP_PORT=8080` or
    ///    `RS_BROKER_KAFKA__CONSUMER__TOPICS=events,orders`
    pub fn load_from_file(path: &str) -> Result<Self, ConfigError> {
        let environment =
            std::env::var("RS_BROKER_ENV").unwrap_or_else(|_| "development".to_string());

        let config = config::Config::builder()
            .add_source(config::File::with_name(&format!("{path}/default")))
            .add_source(config::File::with_name(&format!("{path}/{environment}")).required(false))
            // prefix_separator must stay `_`: with_prefix leaves it unset and
            // config-rs then reuses `separator` for the prefix too, so with
            // separator("__") every RS_BROKER_* key would be silently skipped.
            .add_source(
                config::Environment::with_prefix("RS_BROKER")
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?;

        Ok(config.try_deserialize()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented layering contract: base file from the config dir,
    /// then nested `__` environment overrides. These were both broken
    /// before (the loader looked for a literal `config.toml` and the env
    /// source had no separator, so overrides were silently ignored).
    #[test]
    fn env_overrides_nest_with_double_underscore() {
        let config_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../config");
        std::env::set_var("RS_BROKER_SERVER__HTTP_PORT", "19999");
        std::env::set_var("RS_BROKER_KAFKA__CONSUMER__GROUP_ID", "env-group");
        std::env::set_var("RS_BROKER_KAFKA__CONSUMER__TOPICS", "events,orders");

        let settings = Settings::load_from_file(config_dir).unwrap();
        assert_eq!(settings.server.http_port, 19999);
        assert_eq!(settings.kafka.consumer.group_id, "env-group");
        assert_eq!(settings.kafka.consumer.topics, vec!["events", "orders"]);
    }
}
