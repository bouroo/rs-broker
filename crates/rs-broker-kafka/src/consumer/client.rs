//! Kafka consumer client

use std::sync::Arc;

use futures::StreamExt;
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
    consumer: Arc<StreamConsumer>,
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

        Ok(Self {
            consumer: Arc::new(consumer),
        })
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

    /// Spawn a background task that polls Kafka and forwards deserialized
    /// messages through the returned channel.
    ///
    /// The task stops when the receiver is dropped or the consumer stream ends.
    pub fn stream(&self) -> mpsc::Receiver<Result<IncomingMessage>> {
        let (tx, rx) = mpsc::channel(100);
        let consumer = Arc::clone(&self.consumer);

        tokio::spawn(async move {
            let mut stream = consumer.stream();
            while let Some(message) = stream.next().await {
                match message {
                    Ok(borrowed) => {
                        let incoming = IncomingMessage::from(borrowed);
                        if tx.send(Ok(incoming)).await.is_err() {
                            // Receiver dropped — shut down.
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Kafka consumer error: {}", e);
                        if tx.send(Err(KafkaError::RdKafka(e))).await.is_err() {
                            break;
                        }
                    }
                }
            }
            tracing::info!("Kafka consumer stream ended");
        });

        rx
    }
}
