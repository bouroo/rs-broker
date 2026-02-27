//! Retry policy configuration

use serde::Deserialize;

/// Retry configuration
#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Initial delay in milliseconds
    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,

    /// Multiplier for exponential backoff
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,

    /// Maximum delay in milliseconds
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,

    /// Enable DLQ on failure
    #[serde(default = "default_enable_dlq")]
    pub enable_dlq: bool,

    /// Default DLQ topic name
    #[serde(default = "default_dlq_topic")]
    pub dlq_topic: String,
}

fn default_max_retries() -> u32 {
    3
}

fn default_initial_delay_ms() -> u64 {
    100
}

fn default_multiplier() -> f64 {
    2.0
}

fn default_max_delay_ms() -> u64 {
    30000
}

fn default_enable_dlq() -> bool {
    true
}

fn default_dlq_topic() -> String {
    "rs-broker-dlq".to_string()
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            initial_delay_ms: default_initial_delay_ms(),
            multiplier: default_multiplier(),
            max_delay_ms: default_max_delay_ms(),
            enable_dlq: default_enable_dlq(),
            dlq_topic: default_dlq_topic(),
        }
    }
}
