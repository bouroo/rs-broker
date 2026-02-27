// Benchmark suite for rs-broker-core outbox manager operations
// Run with: cargo bench --package rs-broker-core

use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rs_broker_config::RetryConfig;
use rs_broker_core::outbox::manager::OutboxManager;
use rs_broker_core::outbox::retry::RetryStrategy;
use rs_broker_db::outbox::entity::{MessageStatus, OutboxMessage};
use rs_broker_db::outbox::repository::OutboxError;
use serde_json::json;
use tokio::runtime::Runtime;
use uuid::Uuid;

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
}

#[async_trait]
impl rs_broker_db::outbox::repository::OutboxRepository for MockOutboxRepository {
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

/// Helper to create a test outbox manager
fn create_test_outbox_manager() -> OutboxManager {
    let mock_repo = std::sync::Arc::new(MockOutboxRepository::new());
    let retry_config = RetryConfig::default();
    OutboxManager::with_repository(mock_repo, retry_config)
}

/// Benchmark single message creation
fn bench_manager_create_message(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let manager = create_test_outbox_manager();

    c.bench_function("manager_create_message", |b| {
        b.iter(|| {
            rt.block_on(async {
                let id = manager
                    .create_message(
                        "user".to_string(),
                        "123".to_string(),
                        "user.created".to_string(),
                        json!({"id": "123", "name": "John"}),
                        "user.events".to_string(),
                    )
                    .await;
                black_box(id)
            });
        });
    });
}

/// Benchmark batch message creation
fn bench_manager_create_message_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("manager_create_message_batch");

    for count in [10, 50, 100] {
        group.bench_with_input(BenchmarkId::new("messages", count), &count, |b, &count| {
            let rt = Runtime::new().unwrap();
            let manager = create_test_outbox_manager();

            b.iter(|| {
                rt.block_on(async {
                    for _ in 0..count {
                        let _ = manager
                            .create_message(
                                "user".to_string(),
                                Uuid::new_v4().to_string(),
                                "user.created".to_string(),
                                json!({"id": Uuid::new_v4().to_string(), "name": "John"}),
                                "user.events".to_string(),
                            )
                            .await;
                    }
                });
            });
        });
    }

    group.finish();
}

/// Benchmark getting pending messages
fn bench_manager_get_pending(c: &mut Criterion) {
    let mut group = c.benchmark_group("manager_get_pending");

    for limit in [10, 50, 100, 500] {
        group.bench_with_input(BenchmarkId::new("limit", limit), &limit, |b, &limit| {
            let rt = Runtime::new().unwrap();

            // Pre-populate with messages
            let manager = rt.block_on(async {
                let manager = create_test_outbox_manager();

                // Create some pending messages
                for i in 0..limit.max(100) {
                    let _ = manager
                        .create_message(
                            "user".to_string(),
                            format!("id_{}", i),
                            "user.created".to_string(),
                            json!({"id": format!("id_{}", i), "name": format!("User {}", i)}),
                            "user.events".to_string(),
                        )
                        .await;
                }
                manager
            });

            b.iter(|| {
                rt.block_on(async {
                    let pending = manager.get_pending_messages(limit).await;
                    black_box(pending)
                });
            });
        });
    }

    group.finish();
}

/// Benchmark marking message as published
fn bench_manager_mark_published(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Pre-create a message
    let manager = rt.block_on(async {
        let manager = create_test_outbox_manager();
        let id = manager
            .create_message(
                "user".to_string(),
                "123".to_string(),
                "user.created".to_string(),
                json!({"id": "123", "name": "John"}),
                "user.events".to_string(),
            )
            .await
            .unwrap();
        (manager, id)
    });

    c.bench_function("manager_mark_published", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = manager.0.mark_published(manager.1).await;
                black_box(result)
            });
        });
    });
}

/// Benchmark complete flow: create → get → mark
fn bench_manager_full_flow(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("manager_full_flow", |b| {
        b.iter(|| {
            rt.block_on(async {
                let manager = create_test_outbox_manager();

                // Create message
                let id = manager
                    .create_message(
                        "user".to_string(),
                        "123".to_string(),
                        "user.created".to_string(),
                        json!({"id": "123", "name": "John"}),
                        "user.events".to_string(),
                    )
                    .await
                    .unwrap();

                // Get pending messages
                let pending = manager.get_pending_messages(10).await.unwrap();

                // Mark as published
                if !pending.is_empty() {
                    manager.mark_published(pending[0].id).await.unwrap();
                }

                black_box((id, pending.len()))
            });
        });
    });
}

criterion_group!(
    outbox_manager_benches,
    bench_manager_create_message,
    bench_manager_create_message_batch,
    bench_manager_get_pending,
    bench_manager_mark_published,
    bench_manager_full_flow
);

criterion_main!(outbox_manager_benches);
