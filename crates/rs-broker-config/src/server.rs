//! Server configuration

use serde::Deserialize;

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    #[serde(default = "default_host")]
    pub host: String,

    /// HTTP port
    #[serde(default = "default_http_port")]
    pub http_port: u16,

    /// gRPC port
    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    /// Number of outbox messages fetched per publisher tick
    #[serde(default = "default_publisher_batch_size")]
    pub publisher_batch_size: i64,

    /// Milliseconds between publisher ticks
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

fn default_publisher_batch_size() -> i64 {
    100
}

fn default_publisher_interval_ms() -> u64 {
    100
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            http_port: default_http_port(),
            grpc_port: default_grpc_port(),
            publisher_batch_size: default_publisher_batch_size(),
            publisher_interval_ms: default_publisher_interval_ms(),
        }
    }
}
