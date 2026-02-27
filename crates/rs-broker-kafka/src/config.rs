//! Kafka configuration utilities

use rdkafka::config::ClientConfig;
use rs_broker_config::kafka::KafkaConfig as AppKafkaConfig;

/// Create Kafka producer configuration
pub fn create_producer_config(config: &AppKafkaConfig) -> ClientConfig {
    let mut client_config = ClientConfig::new();

    client_config
        .set("bootstrap.servers", &config.bootstrap_servers)
        .set("client.id", &config.client_id)
        .set("acks", config.producer.acks.to_string())
        .set("linger.ms", config.producer.linger_ms.to_string())
        .set("batch.size", config.producer.batch_size.to_string())
        .set(
            "request.timeout.ms",
            config.producer.request_timeout_ms.to_string(),
        )
        .set("message.timeout.ms", "30000");

    client_config
}

/// Create Kafka consumer configuration
pub fn create_consumer_config(config: &AppKafkaConfig) -> ClientConfig {
    let mut client_config = ClientConfig::new();

    client_config
        .set("bootstrap.servers", &config.bootstrap_servers)
        .set("client.id", &config.client_id)
        .set("group.id", &config.consumer.group_id)
        .set("auto.offset.reset", &config.consumer.auto_offset_reset)
        .set(
            "enable.auto.commit",
            config.consumer.enable_auto_commit.to_string(),
        )
        .set(
            "session.timeout.ms",
            config.consumer.session_timeout_ms.to_string(),
        );

    client_config
}

use crate::{error::Result, KafkaConsumer, KafkaProducer};

/// Create a new Kafka producer instance
pub fn create_producer(config: AppKafkaConfig) -> Result<KafkaProducer> {
    let producer_config = create_producer_config(&config);
    KafkaProducer::new_with_config(producer_config)
}

/// Create a new Kafka consumer instance
pub fn create_consumer(config: AppKafkaConfig) -> Result<KafkaConsumer> {
    let consumer_config = create_consumer_config(&config);
    KafkaConsumer::new_with_config(consumer_config)
}
