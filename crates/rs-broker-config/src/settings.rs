//! Main settings structure for rs-broker

use serde::Deserialize;
use thiserror::Error;

use super::{DatabaseConfig, GrpcConfig, KafkaConfig, RetryConfig, ServerConfig};

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
    /// Load settings from a configuration file
    pub fn load_from_file(path: &str) -> Result<Self, ConfigError> {
        let config = config::Config::builder()
            .add_source(config::File::with_name(path))
            .add_source(config::Environment::with_prefix("RS_BROKER"))
            .build()?;

        Ok(config.try_deserialize()?)
    }
}
