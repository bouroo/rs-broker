//! Kafka adapters implementing rs-broker-core ports.
//!
//! Adapter → inward dependency: implements the publishing feature's
//! [`MessageSink`] port over the rdkafka producer driver.

use rs_broker_core::features::publishing::ports::{MessageSink, OutboundMessage, SinkError};

use crate::producer::client::ProducerMessage;
use crate::KafkaProducer;

/// Publishing drain backed by the Kafka producer. `send` queues the record
/// with the rdkafka `BaseProducer` (delivery is asynchronous inside librdkafka,
/// matching the historical behaviour of the drain loop).
pub struct KafkaMessageSink {
    producer: std::sync::Arc<KafkaProducer>,
}

impl KafkaMessageSink {
    /// Create a sink over a shared Kafka producer.
    pub fn new(producer: std::sync::Arc<KafkaProducer>) -> Self {
        Self { producer }
    }
}

impl MessageSink for KafkaMessageSink {
    fn send(&self, message: OutboundMessage) -> Result<(), SinkError> {
        let record = ProducerMessage {
            topic: message.topic,
            key: message.key,
            payload: message.payload,
            partition: message.partition,
            headers: None,
        };

        self.producer
            .send(record)
            .map_err(|e| SinkError::Transport(e.to_string()))
    }
}
