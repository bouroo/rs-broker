//! Outbox publisher - Background worker for publishing messages

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::error::Result;
use rs_broker_config::RetryConfig;
use rs_broker_db::outbox::repository::SqlxOutboxRepository;
use rs_broker_db::{DbPool, OutboxRepository};
use rs_broker_kafka::{KafkaProducer, ProducerMessage};

/// Outbox publisher - Background worker that publishes pending messages
pub struct OutboxPublisher {
    repository: std::sync::Arc<dyn OutboxRepository>,
    producer: std::sync::Arc<KafkaProducer>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl OutboxPublisher {
    /// Create a new publisher
    pub fn new(
        pool: DbPool,
        kafka_config: rs_broker_config::KafkaConfig,
        _retry_config: RetryConfig,
    ) -> Result<Self> {
        let repository = SqlxOutboxRepository::new(pool);
        let producer = std::sync::Arc::new(KafkaProducer::new(&kafka_config)?);

        Ok(Self {
            repository: std::sync::Arc::new(repository),
            producer,
            shutdown_tx: None,
        })
    }

    /// Start the publisher background task
    pub fn start(&mut self, batch_size: i64, interval_ms: u64) {
        let (tx, mut rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(tx);

        let repository = self.repository.clone();
        let producer = self.producer.clone(); // Arc clone is cheap

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = Self::publish_pending(
                            repository.as_ref(),
                            &producer,
                            batch_size,
                        ).await {
                            error!("Error publishing: {}", e);
                        }
                    }
                    _ = rx.recv() => {
                        info!("Shutting down publisher");
                        break;
                    }
                }
            }
        });
    }

    async fn publish_pending(
        repository: &dyn OutboxRepository,
        producer: &std::sync::Arc<KafkaProducer>,
        batch_size: i64,
    ) -> Result<()> {
        let pending = repository.get_pending(batch_size).await?;

        for message in pending {
            let payload = serde_json::to_vec(&message.payload)?;

            let producer_msg = ProducerMessage {
                topic: message.topic.clone(),
                key: message.partition_key.clone(),
                payload,
                partition: None,
                headers: None,
            };

            if let Err(e) = producer.send(producer_msg) {
                warn!("Failed to send message {}: {}", message.id, e);
                if let Err(db_err) = repository
                    .update_status(
                        message.id,
                        rs_broker_db::outbox::entity::MessageStatus::Failed,
                        Some(e.to_string()),
                    )
                    .await
                {
                    error!(
                        "Failed to update status for message {}: {} (original error: {})",
                        message.id, db_err, e
                    );
                }
            } else if let Err(e) = repository.mark_published(message.id).await {
                error!(
                    "Failed to mark message {} as published: {}. Message may be republished.",
                    message.id, e
                );
            }
        }

        Ok(())
    }

    /// Stop the publisher
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            tx.send(()).await.ok();
        }
    }
}
