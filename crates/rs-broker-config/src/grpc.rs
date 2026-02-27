//! gRPC configuration

use serde::Deserialize;

/// gRPC configuration
#[derive(Debug, Clone, Deserialize)]
pub struct GrpcConfig {
    /// Enable gRPC server
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// gRPC host
    #[serde(default = "default_host")]
    pub host: String,

    /// gRPC port
    #[serde(default = "default_port")]
    pub port: u16,

    /// Max concurrent streams
    #[serde(default = "default_max_concurrent_streams")]
    pub max_concurrent_streams: u32,

    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    50051
}

fn default_max_concurrent_streams() -> u32 {
    100
}

fn default_connection_timeout() -> u64 {
    30
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            host: default_host(),
            port: default_port(),
            max_concurrent_streams: default_max_concurrent_streams(),
            connection_timeout: default_connection_timeout(),
        }
    }
}
