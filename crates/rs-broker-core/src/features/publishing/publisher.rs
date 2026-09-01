//! Outbox publisher - Background use case that drains pending outbox messages
//! to the configured [`MessageSink`].

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::features::publishing::{
    ports::{MessageSink, OutboundMessage, OutboxRepository},
    retry::{PublishFailureDecision, RetryStrategy},
    MessageStatus,
};
use crate::shared::error::Result;
use rs_broker_config::RetryConfig;

/// How long a claimed batch (`publishing` rows) stays owned by this publisher
/// before another publisher may reclaim it — the recovery bound for a
/// publisher that died mid-batch.
const CLAIM_LEASE_SECS: i32 = 60;

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
        let pending = repository
            .claim_pending(batch_size, CLAIM_LEASE_SECS)
            .await?;

        let mut published: Vec<Uuid> = Vec::with_capacity(pending.len());

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

                // The retry→DLQ→failed ladder is the entity's rule; the
                // use case only executes it.
                match message.evaluate_publish_failure(retry_strategy) {
                    PublishFailureDecision::Retry { next_delay } => {
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
                        let _ = &next_delay; // decision carries the backoff the repository applies
                        warn!(
                            "Message {} scheduled for retry (attempt {}, next delay {:?})",
                            message.id, new_count, delay
                        );
                    }
                    PublishFailureDecision::DeadLetter { topic: dlq_topic } => {
                        // Retries exhausted.
                        error!(
                            "Message {} exhausted retries (max {}). Moving to DLQ/failed.",
                            message.id, attempt
                        );

                        // Route to DLQ topic.
                        let dlq_payload = serde_json::to_vec(&message.payload)?;
                        let dlq_outbound = OutboundMessage {
                            topic: dlq_topic.clone(),
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
                                MessageStatus::Dlq,
                                Some(format!("exhausted {attempt} retries; last error: {e}")),
                            )
                            .await
                        {
                            error!(
                                "Failed to update status for message {} to dlq: {}",
                                message.id, db_err
                            );
                        }
                    }
                    PublishFailureDecision::Fail => {
                        // Retries exhausted.
                        error!(
                            "Message {} exhausted retries (max {}). Moving to DLQ/failed.",
                            message.id, attempt
                        );

                        if let Err(db_err) = repository
                            .update_status(
                                message.id,
                                MessageStatus::Failed,
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
            } else {
                published.push(message.id);
            }
        }

        if published.is_empty() {
            return Ok(());
        }

        // One round trip marks the whole batch. A short count means another
        // publisher re-claimed lease-expired rows mid-flight; those may be
        // republished, which at-least-once draining tolerates.
        match repository.mark_published_batch(&published).await {
            Ok(marked) if marked == published.len() as u64 => {}
            Ok(marked) => warn!(
                "Marked {} of {} claimed messages as published; the rest were \
                 re-claimed after the lease expired and may be republished",
                marked,
                published.len()
            ),
            Err(e) => error!(
                "Failed to mark {} messages as published: {}. Messages may be republished.",
                published.len(),
                e
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::publishing::ports::{OutboxError, SinkError};
    use crate::features::publishing::OutboxMessage;
    // The parent's crate-local `Result<T>` alias would otherwise swallow the
    // two-argument std Result the OutboxRepository trait signatures use.
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::result::Result;
    use std::sync::Mutex;

    fn test_retry_strategy() -> RetryStrategy {
        RetryStrategy::new(RetryConfig {
            max_retries: 1,
            initial_delay_ms: 1,
            multiplier: 2.0,
            max_delay_ms: 10,
            enable_dlq: true,
            dlq_topic: "orders.dlq".to_string(),
        })
    }

    fn message(topic: &str, status: MessageStatus, retry_count: i32) -> OutboxMessage {
        let now = Utc::now();
        OutboxMessage {
            id: Uuid::now_v7(),
            aggregate_type: "Order".to_string(),
            aggregate_id: "o-1".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: serde_json::json!({"amount": 1}),
            headers: None,
            topic: topic.to_string(),
            partition_key: None,
            status,
            retry_count,
            error_message: None,
            created_at: now,
            updated_at: now,
            published_at: None,
        }
    }

    #[derive(Default)]
    struct FakeRepo {
        messages: Mutex<HashMap<Uuid, OutboxMessage>>,
        claim_calls: Mutex<Vec<i64>>,
        marked_batches: Mutex<Vec<Vec<Uuid>>>,
    }

    impl FakeRepo {
        fn with_messages(messages: Vec<OutboxMessage>) -> Self {
            let repo = Self::default();
            {
                let mut guard = repo.messages.lock().unwrap();
                for m in messages {
                    guard.insert(m.id, m);
                }
            }
            repo
        }

        fn status_of(&self, id: Uuid) -> MessageStatus {
            self.messages.lock().unwrap().get(&id).unwrap().status
        }
    }

    #[async_trait]
    impl OutboxRepository for FakeRepo {
        async fn create(&self, message: &OutboxMessage) -> Result<(), OutboxError> {
            self.messages
                .lock()
                .unwrap()
                .insert(message.id, message.clone());
            Ok(())
        }

        async fn create_batch(&self, _messages: &[OutboxMessage]) -> Result<(), OutboxError> {
            Ok(())
        }

        async fn get_by_id(&self, id: Uuid) -> Result<OutboxMessage, OutboxError> {
            self.messages
                .lock()
                .unwrap()
                .get(&id)
                .cloned()
                .ok_or(OutboxError::NotFound(id))
        }

        async fn get_pending(&self, limit: i64) -> Result<Vec<OutboxMessage>, OutboxError> {
            let guard = self.messages.lock().unwrap();
            Ok(guard
                .values()
                .filter(|m| matches!(m.status, MessageStatus::Pending | MessageStatus::Retrying))
                .take(limit as usize)
                .cloned()
                .collect())
        }

        async fn claim_pending(
            &self,
            limit: i64,
            _lease_secs: i32,
        ) -> Result<Vec<OutboxMessage>, OutboxError> {
            self.claim_calls.lock().unwrap().push(limit);
            let mut guard = self.messages.lock().unwrap();
            let mut claimed = Vec::new();
            for msg in guard.values_mut() {
                if claimed.len() >= limit as usize {
                    break;
                }
                if matches!(msg.status, MessageStatus::Pending | MessageStatus::Retrying) {
                    msg.status = MessageStatus::Publishing;
                    claimed.push(msg.clone());
                }
            }
            Ok(claimed)
        }

        async fn mark_published_batch(&self, ids: &[Uuid]) -> Result<u64, OutboxError> {
            self.marked_batches.lock().unwrap().push(ids.to_vec());
            let mut guard = self.messages.lock().unwrap();
            for id in ids {
                if let Some(msg) = guard.get_mut(id) {
                    msg.status = MessageStatus::Published;
                    msg.published_at = Some(Utc::now());
                }
            }
            Ok(ids.len() as u64)
        }

        async fn mark_published(&self, id: Uuid) -> Result<(), OutboxError> {
            let mut guard = self.messages.lock().unwrap();
            match guard.get_mut(&id) {
                Some(msg) => {
                    msg.status = MessageStatus::Published;
                    msg.published_at = Some(Utc::now());
                    Ok(())
                }
                None => Err(OutboxError::NotFound(id)),
            }
        }

        async fn update_status(
            &self,
            id: Uuid,
            status: MessageStatus,
            error_message: Option<String>,
        ) -> Result<(), OutboxError> {
            let mut guard = self.messages.lock().unwrap();
            match guard.get_mut(&id) {
                Some(msg) => {
                    msg.status = status;
                    if let Some(err) = error_message {
                        msg.error_message = Some(err);
                    }
                    Ok(())
                }
                None => Err(OutboxError::NotFound(id)),
            }
        }

        async fn increment_retry(
            &self,
            id: Uuid,
            error_message: Option<String>,
        ) -> Result<i32, OutboxError> {
            let mut guard = self.messages.lock().unwrap();
            match guard.get_mut(&id) {
                Some(msg) => {
                    msg.retry_count += 1;
                    msg.status = MessageStatus::Retrying;
                    msg.error_message = error_message;
                    Ok(msg.retry_count)
                }
                None => Err(OutboxError::NotFound(id)),
            }
        }

        async fn delete(&self, id: Uuid) -> Result<(), OutboxError> {
            self.messages
                .lock()
                .unwrap()
                .remove(&id)
                .map(|_| ())
                .ok_or(OutboxError::NotFound(id))
        }
    }

    struct FakeSink {
        fail_topics: Vec<String>,
        sent_topics: Mutex<Vec<String>>,
    }

    impl FakeSink {
        fn failing_for(topics: &[&str]) -> Self {
            Self {
                fail_topics: topics.iter().map(|t| t.to_string()).collect(),
                sent_topics: Mutex::new(Vec::new()),
            }
        }
    }

    impl MessageSink for FakeSink {
        fn send(&self, message: OutboundMessage) -> Result<(), SinkError> {
            if self.fail_topics.contains(&message.topic) {
                return Err(SinkError::Transport(format!(
                    "send failed: {}",
                    message.topic
                )));
            }
            self.sent_topics.lock().unwrap().push(message.topic);
            Ok(())
        }
    }

    async fn drain(repo: &FakeRepo, sink: &FakeSink) {
        OutboxPublisher::publish_pending(repo, sink, &test_retry_strategy(), 10)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn claims_batch_sends_and_marks_published_in_one_call() {
        let messages = vec![
            message("orders", MessageStatus::Pending, 0),
            message("orders", MessageStatus::Pending, 0),
            message("orders", MessageStatus::Retrying, 1),
        ];
        let ids: Vec<Uuid> = messages.iter().map(|m| m.id).collect();
        let repo = FakeRepo::with_messages(messages);
        let sink = FakeSink::failing_for(&[]);

        drain(&repo, &sink).await;

        assert_eq!(*repo.claim_calls.lock().unwrap(), vec![10]);
        let batches = repo.marked_batches.lock().unwrap();
        assert_eq!(batches.len(), 1);
        let mut marked = batches[0].clone();
        marked.sort();
        let mut expected = ids.clone();
        expected.sort();
        assert_eq!(marked, expected);
        for id in ids {
            assert_eq!(repo.status_of(id), MessageStatus::Published);
        }
    }

    #[tokio::test]
    async fn send_failure_schedules_retry_and_is_not_batch_marked() {
        let msg = message("orders", MessageStatus::Pending, 0);
        let id = msg.id;
        let repo = FakeRepo::with_messages(vec![msg]);
        let sink = FakeSink::failing_for(&["orders"]);

        drain(&repo, &sink).await;

        assert!(repo.marked_batches.lock().unwrap().is_empty());
        assert_eq!(repo.status_of(id), MessageStatus::Retrying);
        assert_eq!(
            repo.messages.lock().unwrap().get(&id).unwrap().retry_count,
            1
        );
    }

    #[tokio::test]
    async fn exhausted_retries_route_to_dlq() {
        let msg = message("orders", MessageStatus::Retrying, 1); // retry_count == max_retries
        let id = msg.id;
        let repo = FakeRepo::with_messages(vec![msg]);
        let sink = FakeSink::failing_for(&["orders"]);

        drain(&repo, &sink).await;

        assert_eq!(repo.status_of(id), MessageStatus::Dlq);
        let sent = sink.sent_topics.lock().unwrap();
        assert!(sent.contains(&"orders.dlq".to_string()));
    }
}
