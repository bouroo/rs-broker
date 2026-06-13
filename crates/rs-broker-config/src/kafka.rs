//! Kafka configuration

use serde::Deserialize;

/// Kafka configuration
///
/// The primary broker address field is `bootstrap_servers`. The serde alias
/// `brokers` is accepted so that the documented environment variable
/// `RS_BROKER_KAFKA__BROKERS` maps correctly.
#[derive(Debug, Clone, Deserialize)]
pub struct KafkaConfig {
    /// Bootstrap servers (comma-separated).
    ///
    /// Also accepts the key `brokers` for compatibility with the documented
    /// env var `RS_BROKER_KAFKA__BROKERS`.
    #[serde(default = "default_bootstrap_servers", alias = "brokers")]
    pub bootstrap_servers: String,

    /// Client ID.
    #[serde(default = "default_client_id")]
    pub client_id: String,

    /// Security protocol (plaintext, ssl, sasl_plaintext, sasl_ssl).
    #[serde(default = "default_security_protocol")]
    pub security_protocol: String,

    /// SASL mechanism (PLAIN, SCRAM-SHA-256, SCRAM-SHA-512).
    #[serde(default)]
    pub sasl_mechanism: String,

    /// SASL username.
    #[serde(default)]
    pub sasl_username: String,

    /// SASL password.
    #[serde(default)]
    pub sasl_password: String,

    /// Producer configuration.
    #[serde(default)]
    pub producer: ProducerConfig,

    /// Consumer configuration.
    #[serde(default)]
    pub consumer: ConsumerConfig,
}

fn default_bootstrap_servers() -> String {
    "localhost:9092".to_string()
}

fn default_client_id() -> String {
    "rs-broker".to_string()
}

fn default_security_protocol() -> String {
    "plaintext".to_string()
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            bootstrap_servers: default_bootstrap_servers(),
            client_id: default_client_id(),
            security_protocol: default_security_protocol(),
            sasl_mechanism: String::new(),
            sasl_username: String::new(),
            sasl_password: String::new(),
            producer: ProducerConfig::default(),
            consumer: ConsumerConfig::default(),
        }
    }
}

/// Kafka producer configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ProducerConfig {
    /// ACKS mode: 0, 1, or -1 (all).
    #[serde(default = "default_acks")]
    pub acks: i32,

    /// Linger in milliseconds.
    #[serde(default = "default_linger_ms")]
    pub linger_ms: i32,

    /// Batch size in bytes.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Request timeout in milliseconds.
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: i32,

    /// Message timeout in milliseconds.
    #[serde(default = "default_message_timeout_ms")]
    pub message_timeout_ms: i32,

    /// Enable idempotent producer.
    #[serde(default = "default_enable_idempotence")]
    pub enable_idempotence: bool,

    /// Compression type (none, gzip, snappy, lz4, zstd).
    #[serde(default = "default_compression_type")]
    pub compression_type: String,
}

fn default_acks() -> i32 {
    -1
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

fn default_message_timeout_ms() -> i32 {
    30000
}

fn default_enable_idempotence() -> bool {
    true
}

fn default_compression_type() -> String {
    "snappy".to_string()
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            acks: default_acks(),
            linger_ms: default_linger_ms(),
            batch_size: default_batch_size(),
            request_timeout_ms: default_request_timeout_ms(),
            message_timeout_ms: default_message_timeout_ms(),
            enable_idempotence: default_enable_idempotence(),
            compression_type: default_compression_type(),
        }
    }
}

/// Kafka consumer configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ConsumerConfig {
    /// Consumer group ID.
    ///
    /// Also accepts the key `consumer_group_id` so that the documented env
    /// var `RS_BROKER_KAFKA__CONSUMER_GROUP_ID` maps correctly (the nested
    /// form `RS_BROKER_KAFKA__CONSUMER__GROUP_ID` also works).
    #[serde(default = "default_group_id", alias = "consumer_group_id")]
    pub group_id: String,

    /// Auto offset reset (earliest, latest, none).
    #[serde(default = "default_auto_offset_reset")]
    pub auto_offset_reset: String,

    /// Enable auto commit.
    #[serde(default = "default_enable_auto_commit")]
    pub enable_auto_commit: bool,

    /// Auto commit interval in milliseconds.
    #[serde(default = "default_auto_commit_interval_ms")]
    pub auto_commit_interval_ms: i32,

    /// Session timeout in milliseconds.
    #[serde(default = "default_session_timeout_ms")]
    pub session_timeout_ms: i32,

    /// Heartbeat interval in milliseconds.
    #[serde(default = "default_heartbeat_interval_ms")]
    pub heartbeat_interval_ms: i32,

    /// Maximum number of records per poll.
    #[serde(default = "default_max_poll_records")]
    pub max_poll_records: i32,

    /// Topics the consumer subscribes to at startup.
    ///
    /// Leave empty to disable the consumer pipeline.
    #[serde(default)]
    pub topics: Vec<String>,
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

fn default_auto_commit_interval_ms() -> i32 {
    5000
}

fn default_session_timeout_ms() -> i32 {
    30000
}

fn default_heartbeat_interval_ms() -> i32 {
    10000
}

fn default_max_poll_records() -> i32 {
    500
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            group_id: default_group_id(),
            auto_offset_reset: default_auto_offset_reset(),
            enable_auto_commit: default_enable_auto_commit(),
            auto_commit_interval_ms: default_auto_commit_interval_ms(),
            session_timeout_ms: default_session_timeout_ms(),
            heartbeat_interval_ms: default_heartbeat_interval_ms(),
            max_poll_records: default_max_poll_records(),
            topics: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kafka_config_defaults() {
        let cfg = KafkaConfig::default();
        assert_eq!(cfg.bootstrap_servers, "localhost:9092");
        assert_eq!(cfg.client_id, "rs-broker");
        assert_eq!(cfg.security_protocol, "plaintext");
        assert!(cfg.sasl_mechanism.is_empty());
    }

    #[test]
    fn kafka_config_deserializes_brokers_alias() {
        // Simulates RS_BROKER_KAFKA__BROKERS → kafka.brokers.
        let json = r#"{"brokers": "broker1:9092,broker2:9092"}"#;
        let cfg: KafkaConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.bootstrap_servers, "broker1:9092,broker2:9092");
    }

    #[test]
    fn consumer_config_deserializes_group_id_alias() {
        // Simulates RS_BROKER_KAFKA__CONSUMER_GROUP_ID → kafka.consumer_group_id.
        let json = r#"{"consumer_group_id": "my-group"}"#;
        let cfg: ConsumerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.group_id, "my-group");
    }

    #[test]
    fn producer_config_defaults() {
        let cfg = ProducerConfig::default();
        assert_eq!(cfg.acks, -1);
        assert!(cfg.enable_idempotence);
        assert_eq!(cfg.compression_type, "snappy");
    }
}
