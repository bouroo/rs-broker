//! Kafka consumer client

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Headers, Message};
use tokio::sync::mpsc;

use crate::config::create_consumer_config;
use crate::error::{KafkaError, Result};
use crate::headers::MessageHeaders;
use rs_broker_config::kafka::KafkaConfig as AppKafkaConfig;

/// Incoming Kafka message
pub struct IncomingMessage {
    /// Topic
    pub topic: String,
    /// Partition
    pub partition: i32,
    /// Offset
    pub offset: i64,
    /// Key
    pub key: Option<String>,
    /// Payload
    pub payload: Vec<u8>,
    /// Headers
    pub headers: Option<MessageHeaders>,
    /// Timestamp
    pub timestamp: i64,
}

impl From<BorrowedMessage<'_>> for IncomingMessage {
    fn from(msg: BorrowedMessage<'_>) -> Self {
        let headers = msg.headers().map(|h| {
            let mut headers = MessageHeaders::new();
            // Iterate over headers using the Headers trait
            for header in h.iter() {
                if let Some(value) = header.value {
                    headers.add(header.key, String::from_utf8_lossy(value).to_string());
                }
            }
            headers
        });

        Self {
            topic: msg.topic().to_string(),
            partition: msg.partition(),
            offset: msg.offset(),
            key: msg.key().map(|k| String::from_utf8_lossy(k).to_string()),
            payload: msg.payload().map(|p| p.to_vec()).unwrap_or_default(),
            headers,
            timestamp: msg.timestamp().to_millis().unwrap_or(0),
        }
    }
}

/// Kafka consumer wrapper
pub struct KafkaConsumer {
    consumer: StreamConsumer,
}

impl KafkaConsumer {
    /// Create a new consumer from configuration
    pub fn new(config: &AppKafkaConfig) -> Result<Self> {
        let client_config = create_consumer_config(config);
        Self::new_with_config(client_config)
    }

    /// Create a new consumer with pre-built client config
    pub fn new_with_config(client_config: ClientConfig) -> Result<Self> {
        let consumer = client_config
            .create::<StreamConsumer>()
            .map_err(KafkaError::RdKafka)?;

        Ok(Self { consumer })
    }

    /// Subscribe to topics
    pub fn subscribe(&self, topics: &[&str]) -> Result<()> {
        self.consumer
            .subscribe(topics)
            .map_err(KafkaError::RdKafka)?;
        Ok(())
    }

    /// Get the underlying consumer
    pub fn consumer(&self) -> &StreamConsumer {
        &self.consumer
    }

    /// Create a channel stream for consuming messages
    pub fn stream(&self) -> mpsc::Receiver<Result<IncomingMessage>> {
        let (_tx, rx) = mpsc::channel(100);

        // Note: In a real implementation, this would spawn a task to poll Kafka
        // and send messages to the channel. For now, we return the receiver.

        rx
    }
}
