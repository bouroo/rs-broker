// Benchmark suite for rs-broker-core DLQ handler operations
// Run with: cargo bench --package rs-broker-core

use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rs_broker_core::dlq::handler::DlqHandler;
use rs_broker_db::dlq::entity::DlqMessage;
use rs_broker_db::dlq::repository::DlqError;
use serde_json::json;
use tokio::runtime::Runtime;
use uuid::Uuid;

/// Mock repository for testing
struct MockDlqRepository {
    messages: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<Uuid, DlqMessage>>>,
}

impl MockDlqRepository {
    fn new() -> Self {
        Self {
            messages: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    fn add_dlq_messages(&self, count: usize) {
        let mut guard = self.messages.lock().unwrap();
        for i in 0..count {
            let message = DlqMessage {
                id: Uuid::new_v4(),
                original_message_id: Uuid::new_v4(),
                original_topic: format!("topic_{}", i % 10), // Distribute across 10 topics
                dlq_topic: "dlq.main".to_string(),
                failure_reason: format!("Simulated failure {}", i),
                retry_count: 5, // Max retries reached
                payload: json!({"id": format!("msg_{}", i), "data": format!("payload_{}", i)}),
                headers: None,
                created_at: chrono::Utc::now(),
            };
            guard.insert(message.id, message);
        }
    }
}

#[async_trait]
impl rs_broker_db::dlq::repository::DlqRepository for MockDlqRepository {
    async fn create(&self, message: &DlqMessage) -> Result<(), DlqError> {
        let mut guard = self.messages.lock().unwrap();
        guard.insert(message.id, message.clone());
        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<DlqMessage, DlqError> {
        let guard = self.messages.lock().unwrap();
        match guard.get(&id) {
            Some(msg) => Ok(msg.clone()),
            None => Err(DlqError::NotFound(id)),
        }
    }

    async fn get_all(
        &self,
        topic: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<DlqMessage>, DlqError> {
        let guard = self.messages.lock().unwrap();
        let mut filtered: Vec<DlqMessage> = guard
            .values()
            .filter(|msg| {
                if let Some(t) = topic {
                    msg.original_topic == t
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        // Apply pagination
        filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // Sort by creation date descending
        let start = offset as usize;
        let end = std::cmp::min(start + limit as usize, filtered.len());
        Ok(filtered[start..end].to_vec())
    }

    async fn count(&self, topic: Option<&str>) -> Result<i64, DlqError> {
        let guard = self.messages.lock().unwrap();
        let count = guard
            .values()
            .filter(|msg| {
                if let Some(t) = topic {
                    msg.original_topic == t
                } else {
                    true
                }
            })
            .count();
        Ok(count as i64)
    }

    async fn delete(&self, id: Uuid) -> Result<(), DlqError> {
        let mut guard = self.messages.lock().unwrap();
        match guard.remove(&id) {
            Some(_) => Ok(()),
            None => Err(DlqError::NotFound(id)),
        }
    }

    async fn delete_all(&self) -> Result<(), DlqError> {
        let mut guard = self.messages.lock().unwrap();
        guard.clear();
        Ok(())
    }
}

/// Helper to create a test DLQ handler
fn create_test_dlq_handler() -> DlqHandler {
    let mock_repo = Box::new(MockDlqRepository::new());
    DlqHandler::with_repository(mock_repo)
}

/// Benchmark moving a single message to DLQ
fn bench_dlq_move_message(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let handler = create_test_dlq_handler();

    c.bench_function("dlq_move_message", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = handler
                    .move_to_dlq(
                        Uuid::new_v4(),
                        "user.events".to_string(),
                        "dlq.user".to_string(),
                        "Simulated failure".to_string(),
                        5,
                        json!({"id": "123", "name": "John", "error": "Processing failed"}),
                    )
                    .await;
                black_box(result)
            });
        });
    });
}

/// Benchmark batch DLQ operations
fn bench_dlq_move_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("dlq_move_batch");

    for count in [10, 50, 100] {
        group.bench_with_input(BenchmarkId::new("messages", count), &count, |b, &count| {
            let rt = Runtime::new().unwrap();
            let handler = create_test_dlq_handler();

            b.iter(|| {
                rt.block_on(async {
                    for i in 0..count {
                        let _ = handler
                            .move_to_dlq(
                                Uuid::new_v4(),
                                format!("topic_{}", i % 5),
                                "dlq.main".to_string(),
                                format!("Simulated failure {}", i),
                                5,
                                json!({"id": format!("msg_{}", i), "data": format!("payload_{}", i)}),
                            )
                            .await;
                    }
                });
            });
        });
    }

    group.finish();
}

/// Benchmark getting pending DLQ messages
fn bench_dlq_get_pending(c: &mut Criterion) {
    let mut group = c.benchmark_group("dlq_get_pending");

    for (limit, topic) in [
        (10, None),
        (50, None),
        (100, None),
        (10, Some("user.events")),
        (50, Some("order.events")),
    ] {
        let topic_str = topic.unwrap_or("all");
        group.bench_with_input(
            BenchmarkId::new(format!("{}_{}", topic_str, limit), limit),
            &limit,
            |b, &limit| {
                let rt = Runtime::new().unwrap();

                // Pre-populate with messages
                let handler = rt.block_on(async {
                    let handler = create_test_dlq_handler();

                    // Add messages to the repository
                    let repo = MockDlqRepository::new();
                    repo.add_dlq_messages(200);
                    handler
                });

                b.iter(|| {
                    rt.block_on(async {
                        let topic_filter = topic.map(|s| s.as_ref());
                        let messages = handler.get_messages(topic_filter, limit, 0).await;
                        black_box(messages)
                    });
                });
            },
        );
    }

    group.finish();
}

/// Benchmark DLQ message reprocessing
fn bench_dlq_reprocess(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Pre-populate with messages
    let handler = rt.block_on(async {
        let handler = create_test_dlq_handler();

        // Add messages to the repository
        let repo = MockDlqRepository::new();
        repo.add_dlq_messages(100);
        handler
    });

    c.bench_function("dlq_reprocess", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Get messages from DLQ
                let messages = handler.get_messages(None, 10, 0).await.unwrap();

                // Simulate reprocessing logic
                let results: Vec<_> = messages
                    .iter()
                    .map(|msg| {
                        // Simulate attempting to reprocess the message
                        // In a real scenario, this would involve sending back to the original topic
                        let reprocessing_successful = rand::random::<bool>(); // Simulate success/failure
                        (msg.id, reprocessing_successful)
                    })
                    .collect();

                black_box(results)
            });
        });
    });
}

/// Benchmark DLQ message deletion
fn bench_dlq_delete(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Pre-populate with messages and get an ID to delete
    let (handler, message_id) = rt.block_on(async {
        let handler = create_test_dlq_handler();

        // Add a message and get its ID
        let id = handler
            .move_to_dlq(
                Uuid::new_v4(),
                "user.events".to_string(),
                "dlq.user".to_string(),
                "Simulated failure".to_string(),
                5,
                json!({"id": "123", "name": "John"}),
            )
            .await
            .unwrap();

        (handler, id)
    });

    c.bench_function("dlq_delete", |b| {
        b.iter(|| {
            rt.block_on(async {
                // We need to recreate the message first since we're deleting it
                let temp_handler = create_test_dlq_handler();
                let temp_id = temp_handler
                    .move_to_dlq(
                        Uuid::new_v4(),
                        "user.events".to_string(),
                        "dlq.user".to_string(),
                        "Simulated failure".to_string(),
                        5,
                        json!({"id": "456", "name": "Jane"}),
                    )
                    .await
                    .unwrap();

                let result = temp_handler.delete(temp_id).await;
                black_box(result)
            });
        });
    });
}

/// Benchmark DLQ message counting
fn bench_dlq_count(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Pre-populate with messages
    let handler = rt.block_on(async {
        let handler = create_test_dlq_handler();

        // Add messages to the repository
        let repo = MockDlqRepository::new();
        repo.add_dlq_messages(150);
        handler
    });

    c.bench_function("dlq_count", |b| {
        b.iter(|| {
            rt.block_on(async {
                let count = handler.count(None).await;
                black_box(count)
            });
        });
    });
}

criterion_group!(
    dlq_benches,
    bench_dlq_move_message,
    bench_dlq_move_batch,
    bench_dlq_get_pending,
    bench_dlq_reprocess,
    bench_dlq_delete,
    bench_dlq_count
);

criterion_main!(dlq_benches);
