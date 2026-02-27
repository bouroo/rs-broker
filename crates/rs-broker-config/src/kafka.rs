//! Kafka configuration

use serde::Deserialize;

/// Kafka configuration
#[derive(Debug, Clone, Deserialize)]
pub struct KafkaConfig {
    /// Bootstrap servers
    #[serde(default = "default_bootstrap_servers")]
    pub bootstrap_servers: String,

    /// Client ID
    #[serde(default = "default_client_id")]
    pub client_id: String,

    /// Producer configuration
    #[serde(default)]
    pub producer: ProducerConfig,

    /// Consumer configuration
    #[serde(default)]
    pub consumer: ConsumerConfig,
}

fn default_bootstrap_servers() -> String {
    "localhost:9092".to_string()
}

fn default_client_id() -> String {
    "rs-broker".to_string()
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            bootstrap_servers: default_bootstrap_servers(),
            client_id: default_client_id(),
            producer: ProducerConfig::default(),
            consumer: ConsumerConfig::default(),
        }
    }
}

/// Kafka producer configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ProducerConfig {
    /// ACKS mode: 0, 1, or -1
    #[serde(default = "default_acks")]
    pub acks: i32,

    /// Linger.ms
    #[serde(default = "default_linger_ms")]
    pub linger_ms: i32,

    /// Batch size
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Request timeout
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: i32,
}

fn default_acks() -> i32 {
    1
}

fn default_linger_ms() -> i32 {
    5
}

fn default_batch_size() -> usize {
    16384
}

fn default_request_timeout_ms() -> i32 {
    30000
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            acks: default_acks(),
            linger_ms: default_linger_ms(),
            batch_size: default_batch_size(),
            request_timeout_ms: default_request_timeout_ms(),
        }
    }
}

/// Kafka consumer configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ConsumerConfig {
    /// Group ID
    #[serde(default = "default_group_id")]
    pub group_id: String,

    /// Auto offset reset
    #[serde(default = "default_auto_offset_reset")]
    pub auto_offset_reset: String,

    /// Enable auto commit
    #[serde(default = "default_enable_auto_commit")]
    pub enable_auto_commit: bool,

    /// Session timeout
    #[serde(default = "default_session_timeout_ms")]
    pub session_timeout_ms: i32,
}

fn default_group_id() -> String {
    "rs-broker-consumer".to_string()
}

fn default_auto_offset_reset() -> String {
    "earliest".to_string()
}

fn default_enable_auto_commit() -> bool {
    false
}

fn default_session_timeout_ms() -> i32 {
    30000
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            group_id: default_group_id(),
            auto_offset_reset: default_auto_offset_reset(),
            enable_auto_commit: default_enable_auto_commit(),
            session_timeout_ms: default_session_timeout_ms(),
        }
    }
}
