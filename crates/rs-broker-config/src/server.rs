//! Server configuration

use serde::Deserialize;

/// Server operating mode.
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServerMode {
    /// Run both producer and consumer pipelines.
    #[default]
    Both,
    /// Run only the producer (outbox) pipeline.
    Producer,
    /// Run only the consumer (inbox) pipeline.
    Consumer,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Operating mode: producer, consumer, or both.
    #[serde(default)]
    pub mode: ServerMode,

    /// Host to bind to.
    #[serde(default = "default_host")]
    pub host: String,

    /// HTTP port.
    #[serde(default = "default_http_port")]
    pub http_port: u16,

    /// gRPC port.
    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    /// Graceful shutdown timeout in seconds.
    #[serde(default = "default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,

    /// Request timeout in seconds.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Number of outbox messages fetched per publisher tick.
    #[serde(default = "default_publisher_batch_size")]
    pub publisher_batch_size: i64,

    /// Milliseconds between publisher ticks.
    #[serde(default = "default_publisher_interval_ms")]
    pub publisher_interval_ms: u64,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_http_port() -> u16 {
    8080
}

fn default_grpc_port() -> u16 {
    50051
}

fn default_shutdown_timeout_secs() -> u64 {
    30
}

fn default_request_timeout_secs() -> u64 {
    30
}

fn default_publisher_batch_size() -> i64 {
    100
}

fn default_publisher_interval_ms() -> u64 {
    100
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            mode: ServerMode::default(),
            host: default_host(),
            http_port: default_http_port(),
            grpc_port: default_grpc_port(),
            shutdown_timeout_secs: default_shutdown_timeout_secs(),
            request_timeout_secs: default_request_timeout_secs(),
            publisher_batch_size: default_publisher_batch_size(),
            publisher_interval_ms: default_publisher_interval_ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_defaults() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.http_port, 8080);
        assert_eq!(cfg.grpc_port, 50051);
        assert_eq!(cfg.mode, ServerMode::Both);
        assert_eq!(cfg.shutdown_timeout_secs, 30);
    }

    #[test]
    fn server_mode_deserializes_lowercase() {
        let m: ServerMode = serde_json::from_str("\"producer\"").unwrap();
        assert_eq!(m, ServerMode::Producer);

        let m: ServerMode = serde_json::from_str("\"consumer\"").unwrap();
        assert_eq!(m, ServerMode::Consumer);

        let m: ServerMode = serde_json::from_str("\"both\"").unwrap();
        assert_eq!(m, ServerMode::Both);
    }
}
