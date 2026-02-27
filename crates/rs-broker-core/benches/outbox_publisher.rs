// Benchmark suite for rs-broker-core outbox publisher operations
// Run with: cargo bench --package rs-broker-core

use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rs_broker_db::outbox::entity::{MessageStatus, OutboxMessage};
use rs_broker_db::outbox::repository::{OutboxError, OutboxRepository};
use serde_json::json;
use std::sync::Arc;
use tokio::runtime::Runtime;
use uuid::Uuid;

/// Mock Kafka producer for testing
struct MockKafkaProducer;

impl MockKafkaProducer {
    fn new() -> Self {
        Self
    }

    fn send(
        &self,
        _message: rs_broker_kafka::ProducerMessage,
    ) -> Result<(), rs_broker_kafka::KafkaError> {
        Ok(())
    }
}

impl Clone for MockKafkaProducer {
    fn clone(&self) -> Self {
        MockKafkaProducer
    }
}

/// Mock repository for testing
struct MockOutboxRepository {
    messages: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<Uuid, OutboxMessage>>>,
}

impl MockOutboxRepository {
    fn new() -> Self {
        Self {
            messages: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    fn add_pending_messages(&self, count: usize) {
        let mut guard = self.messages.lock().unwrap();
        for _ in 0..count {
            let message = OutboxMessage {
                id: Uuid::new_v4(),
                aggregate_type: "user".to_string(),
                aggregate_id: Uuid::new_v4().to_string(),
                event_type: "user.created".to_string(),
                payload: json!({"id": Uuid::new_v4().to_string(), "name": "John"}),
                headers: None,
                topic: "user.events".to_string(),
                partition_key: Some(Uuid::new_v4().to_string()),
                status: MessageStatus::Pending,
                retry_count: 0,
                error_message: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                published_at: None,
            };
            guard.insert(message.id, message);
        }
    }
}

#[async_trait]
impl OutboxRepository for MockOutboxRepository {
    async fn create(&self, message: &OutboxMessage) -> Result<(), OutboxError> {
        let mut guard = self.messages.lock().unwrap();
        guard.insert(message.id, message.clone());
        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<OutboxMessage, OutboxError> {
        let guard = self.messages.lock().unwrap();
        match guard.get(&id) {
            Some(msg) => Ok(msg.clone()),
            None => Err(OutboxError::NotFound(id)),
        }
    }

    async fn get_pending(&self, limit: i64) -> Result<Vec<OutboxMessage>, OutboxError> {
        let guard = self.messages.lock().unwrap();
        let pending: Vec<OutboxMessage> = guard
            .values()
            .filter(|msg| msg.status == MessageStatus::Pending)
            .take(limit as usize)
            .cloned()
            .collect();
        Ok(pending)
    }

    async fn mark_published(&self, id: Uuid) -> Result<(), OutboxError> {
        let mut guard = self.messages.lock().unwrap();
        if let Some(msg) = guard.get_mut(&id) {
            msg.status = MessageStatus::Published;
            msg.published_at = Some(chrono::Utc::now());
            Ok(())
        } else {
            Err(OutboxError::NotFound(id))
        }
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: MessageStatus,
        error_message: Option<String>,
    ) -> Result<(), OutboxError> {
        let mut guard = self.messages.lock().unwrap();
        if let Some(msg) = guard.get_mut(&id) {
            msg.status = status;
            if let Some(err) = error_message {
                msg.error_message = Some(err);
            }
            Ok(())
        } else {
            Err(OutboxError::NotFound(id))
        }
    }

    async fn delete(&self, id: Uuid) -> Result<(), OutboxError> {
        let mut guard = self.messages.lock().unwrap();
        match guard.remove(&id) {
            Some(_) => Ok(()),
            None => Err(OutboxError::NotFound(id)),
        }
    }
}

/// Benchmark single message publishing simulation
fn bench_publisher_single(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("publisher_single", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Simulate the internal publishing logic
                let mock_repo = MockOutboxRepository::new();
                let mock_producer = Arc::new(MockKafkaProducer::new());

                // Add a pending message to the mock repository
                mock_repo.add_pending_messages(1);

                // Get pending messages
                let pending = mock_repo.get_pending(1).await.unwrap();

                // Process each message
                for message in pending {
                    let payload = serde_json::to_vec(&message.payload).unwrap();

                    let producer_msg = rs_broker_kafka::ProducerMessage {
                        topic: message.topic.clone(),
                        key: message.partition_key.clone(),
                        payload,
                        partition: None,
                        headers: None,
                    };

                    if let Err(_e) = mock_producer.send(producer_msg) {
                        mock_repo
                            .update_status(
                                message.id,
                                MessageStatus::Failed,
                                Some("Send failed".to_string()),
                            )
                            .await
                            .ok();
                    } else {
                        mock_repo.mark_published(message.id).await.ok();
                    }
                }

                black_box(())
            });
        });
    });
}

/// Benchmark batch message publishing simulation
fn bench_publisher_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("publisher_batch");

    for count in [10, 50, 100] {
        group.bench_with_input(BenchmarkId::new("messages", count), &count, |b, &count| {
            let rt = Runtime::new().unwrap();

            b.iter(|| {
                rt.block_on(async {
                    // Simulate the internal publishing logic
                    let mock_repo = MockOutboxRepository::new();
                    let mock_producer = Arc::new(MockKafkaProducer::new());

                    // Add pending messages to the mock repository
                    mock_repo.add_pending_messages(count as usize);

                    // Get pending messages
                    let pending = mock_repo.get_pending(count).await.unwrap();

                    // Process each message
                    for message in pending {
                        let payload = serde_json::to_vec(&message.payload).unwrap();

                        let producer_msg = rs_broker_kafka::ProducerMessage {
                            topic: message.topic.clone(),
                            key: message.partition_key.clone(),
                            payload,
                            partition: None,
                            headers: None,
                        };

                        if let Err(_e) = mock_producer.send(producer_msg) {
                            mock_repo
                                .update_status(
                                    message.id,
                                    MessageStatus::Failed,
                                    Some("Send failed".to_string()),
                                )
                                .await
                                .ok();
                        } else {
                            mock_repo.mark_published(message.id).await.ok();
                        }
                    }

                    black_box(())
                });
            });
        });
    }

    group.finish();
}

/// Benchmark concurrent publishing simulation with worker pool
fn bench_publisher_concurrent(c: &mut Criterion) {
    use futures_util::future::join_all;

    let rt = Runtime::new().unwrap();

    c.bench_function("publisher_concurrent", |b| {
        b.iter_with_setup(
            || {
                // Create multiple sets of mock repos and producers for concurrent work
                (0..10)
                    .map(|_| {
                        (
                            MockOutboxRepository::new(),
                            Arc::new(MockKafkaProducer::new()),
                        )
                    })
                    .collect::<Vec<_>>()
            },
            |mock_pairs| {
                rt.block_on(async {
                    let tasks = mock_pairs.into_iter().map(|(repo, producer)| {
                        // Add pending messages to each repository
                        repo.add_pending_messages(5);

                        async move {
                            // Get pending messages
                            let pending = repo.get_pending(5).await.unwrap();

                            // Process each message
                            for message in pending {
                                let payload = serde_json::to_vec(&message.payload).unwrap();

                                let producer_msg = rs_broker_kafka::ProducerMessage {
                                    topic: message.topic.clone(),
                                    key: message.partition_key.clone(),
                                    payload,
                                    partition: None,
                                    headers: None,
                                };

                                if let Err(_e) = producer.send(producer_msg) {
                                    repo.update_status(
                                        message.id,
                                        MessageStatus::Failed,
                                        Some("Send failed".to_string()),
                                    )
                                    .await
                                    .ok();
                                } else {
                                    repo.mark_published(message.id).await.ok();
                                }
                            }

                            Ok::<(), rs_broker_core::error::Error>(())
                        }
                    });

                    let results = join_all(tasks).await;
                    black_box(results.len())
                })
            },
        );
    });
}

/// Benchmark publisher with JSON serialization overhead
fn bench_publisher_with_serialization(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("publisher_with_serialization", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Simulate the internal publishing logic with large payloads
                let mock_repo = MockOutboxRepository::new();
                let mock_producer = Arc::new(MockKafkaProducer::new());

                // Add a pending message with a large payload to the mock repository
                {
                    let mut guard = mock_repo.messages.lock().unwrap();
                    let large_payload = json!({
                        "id": Uuid::new_v4().to_string(),
                        "name": "John",
                        "data": (0..1000).map(|i| format!("item_{}", i)).collect::<Vec<_>>(),
                        "nested": {
                            "level1": {
                                "level2": {
                                    "level3": (0..100).map(|i| format!("deep_item_{}", i)).collect::<Vec<_>>()
                                }
                            }
                        }
                    });

                    let message = OutboxMessage {
                        id: Uuid::new_v4(),
                        aggregate_type: "user".to_string(),
                        aggregate_id: Uuid::new_v4().to_string(),
                        event_type: "user.created".to_string(),
                        payload: large_payload,
                        headers: None,
                        topic: "user.events".to_string(),
                        partition_key: Some(Uuid::new_v4().to_string()),
                        status: MessageStatus::Pending,
                        retry_count: 0,
                        error_message: None,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                        published_at: None,
                    };
                    guard.insert(message.id, message);
                }

                // Get pending messages
                let pending = mock_repo.get_pending(1).await.unwrap();

                // Process each message (this is where serialization happens)
                for message in pending {
                    let payload = serde_json::to_vec(&message.payload).unwrap(); // This is the serialization

                    let producer_msg = rs_broker_kafka::ProducerMessage {
                        topic: message.topic.clone(),
                        key: message.partition_key.clone(),
                        payload,
                        partition: None,
                        headers: None,
                    };

                    if let Err(_e) = mock_producer.send(producer_msg) {
                        mock_repo.update_status(
                            message.id,
                            MessageStatus::Failed,
                            Some("Send failed".to_string()),
                        ).await.ok();
                    } else {
                        mock_repo.mark_published(message.id).await.ok();
                    }
                }

                black_box(())
            });
        });
    });
}

/// Benchmark publisher with mock Kafka
fn bench_publisher_mock_kafka(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("publisher_mock_kafka", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Simulate the internal publishing logic
                let mock_repo = MockOutboxRepository::new();
                let mock_producer = Arc::new(MockKafkaProducer::new());

                // Add multiple pending messages to the mock repository
                mock_repo.add_pending_messages(50);

                // Get pending messages
                let pending = mock_repo.get_pending(50).await.unwrap();

                // Process each message
                for message in pending {
                    let payload = serde_json::to_vec(&message.payload).unwrap();

                    let producer_msg = rs_broker_kafka::ProducerMessage {
                        topic: message.topic.clone(),
                        key: message.partition_key.clone(),
                        payload,
                        partition: None,
                        headers: None,
                    };

                    if let Err(_e) = mock_producer.send(producer_msg) {
                        mock_repo
                            .update_status(
                                message.id,
                                MessageStatus::Failed,
                                Some("Send failed".to_string()),
                            )
                            .await
                            .ok();
                    } else {
                        mock_repo.mark_published(message.id).await.ok();
                    }
                }

                black_box(())
            });
        });
    });
}

criterion_group!(
    outbox_publisher_benches,
    bench_publisher_single,
    bench_publisher_batch,
    bench_publisher_concurrent,
    bench_publisher_with_serialization,
    bench_publisher_mock_kafka
);

criterion_main!(outbox_publisher_benches);
