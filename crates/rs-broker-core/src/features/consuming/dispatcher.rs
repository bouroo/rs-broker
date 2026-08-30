//! Dispatcher for delivering messages to subscribers

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::features::delivery::dispatcher::SubscriberDispatcher;
use crate::features::subscription::ports::SubscriberRepository;
use crate::features::subscription::Subscriber;
use crate::shared::error::Result;
use crate::shared::topic::matches_topic;
use rs_broker_proto::rsbroker::DeliverRequest;

/// Default capacity for the pattern cache.
const DEFAULT_PATTERN_CACHE_CAPACITY: usize = 1000;

/// Bounded FIFO topic -> subscribers cache.
///
/// Insertion order is tracked in a `VecDeque` so that, on overflow, only the
/// oldest single entry is evicted. This avoids the thundering-herd that a
/// full-cache `clear()` would trigger (every previously-cached topic would
/// re-query the database on its next access).
struct SubscriberCache {
    entries: HashMap<String, Vec<Subscriber>>,
    order: VecDeque<String>,
    capacity: usize,
}

impl SubscriberCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&self, topic: &str) -> Option<&Vec<Subscriber>> {
        self.entries.get(topic)
    }

    fn insert(&mut self, topic: String, subscribers: Vec<Subscriber>) {
        // Re-insert of an existing key is a no-op for ordering: the original
        // insertion position is preserved, matching a typical FIFO cache.
        if let std::collections::hash_map::Entry::Occupied(mut e) =
            self.entries.entry(topic.clone())
        {
            e.insert(subscribers);
            return;
        }

        if self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }

        self.order.push_back(topic.clone());
        self.entries.insert(topic, subscribers);
    }

    #[allow(dead_code)]
    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

/// Message dispatcher
pub struct Dispatcher {
    subscriber_repository: Arc<dyn SubscriberRepository>,
    /// Cache for topic pattern matching results
    pattern_cache: Arc<RwLock<SubscriberCache>>, // topic -> vec of matching subscribers
    /// Optional gRPC subscriber dispatcher for real delivery
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    subscriber_dispatcher: Option<Arc<SubscriberDispatcher>>,
}

impl Dispatcher {
    /// Create a new dispatcher
    pub fn new(repository: Arc<dyn SubscriberRepository>) -> Self {
        Self {
            subscriber_repository: repository,
            pattern_cache: Arc::new(RwLock::new(SubscriberCache::new(
                DEFAULT_PATTERN_CACHE_CAPACITY,
            ))),
            subscriber_dispatcher: None,
        }
    }

    /// Create a new dispatcher with a custom subscriber repository (for testing)
    pub fn with_repository(subscriber_repository: Arc<dyn SubscriberRepository>) -> Self {
        Self {
            subscriber_repository,
            pattern_cache: Arc::new(RwLock::new(SubscriberCache::new(
                DEFAULT_PATTERN_CACHE_CAPACITY,
            ))),
            #[cfg(any(feature = "postgres", feature = "mysql"))]
            subscriber_dispatcher: None,
        }
    }

    /// Create a new dispatcher with a SubscriberDispatcher for real gRPC delivery
    pub fn with_subscriber_dispatcher(
        subscriber_repository: Arc<dyn SubscriberRepository>,
        subscriber_dispatcher: Arc<SubscriberDispatcher>,
    ) -> Self {
        Self {
            subscriber_repository,
            pattern_cache: Arc::new(RwLock::new(SubscriberCache::new(
                DEFAULT_PATTERN_CACHE_CAPACITY,
            ))),
            subscriber_dispatcher: Some(subscriber_dispatcher),
        }
    }

    /// Get active subscribers for a topic
    pub async fn get_subscribers(&self, topic: &str) -> Result<Vec<Subscriber>> {
        // Check if we have cached results for this topic
        {
            let cache = self.pattern_cache.read().await;
            if let Some(cached) = cache.get(topic) {
                return Ok(cached.clone());
            }
        }

        // Not in cache, compute the result
        let all = self.subscriber_repository.get_all_active().await?;

        // Filter subscribers by topic pattern
        let matching: Vec<Subscriber> = all
            .into_iter()
            .filter(|s| {
                s.topic_patterns
                    .iter()
                    .any(|pattern| matches_topic(topic, pattern))
            })
            .collect();

        // Cache the result (single-entry FIFO eviction when full)
        {
            let mut cache = self.pattern_cache.write().await;
            cache.insert(topic.to_string(), matching.clone());
        }

        Ok(matching)
    }

    /// Dispatch a message to all matching subscribers
    pub async fn dispatch(&self, topic: &str, payload: &[u8]) -> Result<usize> {
        // If a SubscriberDispatcher is configured, use it for real gRPC delivery
        if let Some(ref sd) = self.subscriber_dispatcher {
            let request = DeliverRequest {
                message_id: uuid::Uuid::now_v7().to_string(),
                topic: topic.to_string(),
                payload: payload.to_vec(),
                headers: Vec::new(),
                timestamp: chrono::Utc::now().timestamp(),
                event_type: String::new(),
                retry_count: 0,
            };
            let results = sd.dispatch_to_all(topic, request).await;
            let success_count = results.iter().filter(|r| r.success).count();
            return Ok(success_count);
        }

        // Fallback: log-only delivery (when no SubscriberDispatcher is set)
        let subscribers = self.get_subscribers(topic).await?;

        let mut success_count = 0;

        for subscriber in subscribers {
            info!("Dispatching to subscriber: {}", subscriber.grpc_endpoint);
            success_count += 1;
        }

        Ok(success_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriber_cache_evicts_oldest_on_overflow() {
        let mut cache = SubscriberCache::new(3);
        cache.insert("a".to_string(), vec![]);
        cache.insert("b".to_string(), vec![]);
        cache.insert("c".to_string(), vec![]);

        // Cache is at capacity; inserting "d" must evict "a".
        cache.insert("d".to_string(), vec![]);

        assert!(cache.get("a").is_none(), "oldest entry should be evicted");
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
        assert!(cache.get("d").is_some());
        assert_eq!(cache.entries.len(), 3, "entries should be at capacity");
    }

    #[test]
    fn subscriber_cache_preserves_capacity_after_many_evictions() {
        let mut cache = SubscriberCache::new(2);
        for i in 0..10 {
            cache.insert(format!("topic-{i}"), vec![]);
        }

        assert_eq!(cache.entries.len(), 2, "entries should stay at capacity");
        assert_eq!(cache.order.len(), 2, "order deque should stay at capacity");
        // Only the two most recent inserts should survive.
        assert!(cache.get("topic-0").is_none());
        assert!(cache.get("topic-7").is_none());
        assert!(cache.get("topic-8").is_some());
        assert!(cache.get("topic-9").is_some());
    }

    #[test]
    fn subscriber_cache_reinsert_does_not_change_order() {
        let mut cache = SubscriberCache::new(3);
        cache.insert("a".to_string(), vec![]);
        cache.insert("b".to_string(), vec![]);

        // Re-inserting an existing key must not bump it to the back.
        cache.insert("a".to_string(), vec![]);
        cache.insert("c".to_string(), vec![]);
        // Cache is full now; inserting "d" must evict the oldest = "a"
        // (because re-insert did NOT move "a" to the back).
        cache.insert("d".to_string(), vec![]);

        assert!(
            cache.get("a").is_none(),
            "re-inserting should not refresh insertion order; 'a' must still be the oldest"
        );
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
        assert!(cache.get("d").is_some());
    }
}
