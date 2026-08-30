//! Outbox publisher - Background use case that drains pending outbox messages
//! to the configured [`MessageSink`].

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::features::publishing::{
    ports::{MessageSink, OutboundMessage, OutboxRepository},
    retry::RetryStrategy,
};
use crate::shared::error::Result;
use rs_broker_config::RetryConfig;

/// Outbox publisher - Background worker that publishes pending messages
pub struct OutboxPublisher {
    repository: Arc<dyn OutboxRepository>,
    sink: Arc<dyn MessageSink>,
    retry_strategy: RetryStrategy,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl OutboxPublisher {
    /// Create a new publisher over the given repository port and outbound
    /// message sink.
    pub fn new(
        repository: Arc<dyn OutboxRepository>,
        sink: Arc<dyn MessageSink>,
        retry_config: RetryConfig,
    ) -> Result<Self> {
        let retry_strategy = RetryStrategy::new(retry_config);

        Ok(Self {
            repository,
            sink,
            retry_strategy,
            shutdown_tx: None,
        })
    }

    /// Start the publisher background task
    pub fn start(&mut self, batch_size: i64, interval_ms: u64) {
        let (tx, mut rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(tx);

        let repository = self.repository.clone();
        let sink = self.sink.clone();
        let retry_strategy = self.retry_strategy.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = Self::publish_pending(
                            repository.as_ref(),
                            sink.as_ref(),
                            &retry_strategy,
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
        sink: &dyn MessageSink,
        retry_strategy: &RetryStrategy,
        batch_size: i64,
    ) -> Result<()> {
        let pending = repository.get_pending(batch_size).await?;

        for message in pending {
            let payload = serde_json::to_vec(&message.payload)?;

            let outbound = OutboundMessage {
                topic: message.topic.clone(),
                key: message.partition_key.clone(),
                payload,
                partition: None,
            };

            // Attempt to send to the sink. On failure, apply retry strategy:
            //   - If retries remain, increment retry count and set `retrying`.
            //   - If retries exhausted, set to `failed` or route to DLQ.
            let attempt = message.retry_count as u32;

            if let Err(e) = sink.send(outbound) {
                warn!(
                    "Failed to send message {} (attempt {}): {}",
                    message.id,
                    attempt + 1,
                    e
                );

                if retry_strategy.should_retry(attempt) {
                    // Will be retried on the next tick — status stays `retrying`.
                    let new_count = repository
                        .increment_retry(message.id, Some(e.to_string()))
                        .await
                        .unwrap_or_else(|err| {
                            error!(
                                "Failed to increment retry for message {}: {} (original: {})",
                                message.id, err, e
                            );
                            attempt as i32
                        });

                    let delay = retry_strategy.calculate_delay(new_count as u32);
                    warn!(
                        "Message {} scheduled for retry (attempt {}, next delay {:?})",
                        message.id, new_count, delay
                    );
                } else {
                    // Retries exhausted.
                    error!(
                        "Message {} exhausted retries (max {}). Moving to DLQ/failed.",
                        message.id, attempt
                    );

                    if retry_strategy.is_dlq_enabled() {
                        // Route to DLQ topic.
                        let dlq_topic = retry_strategy.dlq_topic();
                        let dlq_payload = serde_json::to_vec(&message.payload)?;
                        let dlq_outbound = OutboundMessage {
                            topic: dlq_topic.to_string(),
                            key: message.partition_key.clone(),
                            payload: dlq_payload,
                            partition: None,
                        };

                        if let Err(dlq_err) = sink.send(dlq_outbound) {
                            error!(
                                "Failed to send message {} to DLQ topic {}: {}",
                                message.id, dlq_topic, dlq_err
                            );
                        }

                        if let Err(db_err) = repository
                            .update_status(
                                message.id,
                                crate::features::publishing::domain::MessageStatus::Dlq,
                                Some(format!("exhausted {attempt} retries; last error: {e}")),
                            )
                            .await
                        {
                            error!(
                                "Failed to update status for message {} to dlq: {}",
                                message.id, db_err
                            );
                        }
                    } else {
                        if let Err(db_err) = repository
                            .update_status(
                                message.id,
                                crate::features::publishing::domain::MessageStatus::Failed,
                                Some(format!("exhausted {attempt} retries; last error: {e}")),
                            )
                            .await
                        {
                            error!(
                                "Failed to update status for message {} to failed: {}",
                                message.id, db_err
                            );
                        }
                    }
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
