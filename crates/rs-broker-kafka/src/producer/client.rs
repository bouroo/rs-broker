//! Kafka producer client

use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::producer::{BaseProducer, BaseRecord, Producer};

use crate::config::create_producer_config;
use crate::error::{KafkaError, Result};
use crate::headers::MessageHeaders;
use rs_broker_config::KafkaConfig as AppKafkaConfig;

/// Kafka message to produce
pub struct ProducerMessage {
    /// Target topic
    pub topic: String,
    /// Message key
    pub key: Option<String>,
    /// Message payload (bytes)
    pub payload: Vec<u8>,
    /// Partition (None for auto)
    pub partition: Option<i32>,
    /// Message headers
    pub headers: Option<MessageHeaders>,
}

/// Kafka producer wrapper
pub struct KafkaProducer {
    producer: BaseProducer,
}

impl KafkaProducer {
    /// Create a new producer from configuration
    pub fn new(config: &AppKafkaConfig) -> Result<Self> {
        let client_config = create_producer_config(config);
        Self::new_with_config(client_config)
    }

    /// Create a new producer with pre-built client config
    pub fn new_with_config(client_config: ClientConfig) -> Result<Self> {
        let producer = client_config
            .create::<BaseProducer>()
            .map_err(KafkaError::RdKafka)?;

        Ok(Self { producer })
    }

    /// Send a message
    pub fn send(&self, message: ProducerMessage) -> Result<()> {
        let mut record = BaseRecord::to(&message.topic).payload(&message.payload);

        if let Some(ref key) = message.key {
            record = record.key(key);
        }

        if let Some(partition) = message.partition {
            record = record.partition(partition);
        }

        if let Some(ref headers) = message.headers {
            record = record.headers(headers.to_rdkafka());
        }

        self.producer
            .send(record)
            .map_err(|(e, _)| KafkaError::Producer(e.to_string()))?;

        Ok(())
    }

    /// Flush pending messages
    pub fn flush(&self) {
        let _ = self.producer.flush(Duration::from_secs(1));
    }
}
