//! Dispatcher for delivering messages to subscribers

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::error::Result;
use crate::topic::matches_topic;
use rs_broker_db::subscriber::repository::SqlxSubscriberRepository;
use rs_broker_db::{DbPool, Subscriber, SubscriberRepository};

#[cfg(any(feature = "postgres", feature = "mysql"))]
use crate::grpc_client::dispatcher::SubscriberDispatcher;

#[cfg(any(feature = "postgres", feature = "mysql"))]
use rs_broker_proto::rsbroker::DeliverRequest;

/// Message dispatcher
pub struct Dispatcher {
    subscriber_repository: Arc<dyn SubscriberRepository>,
    /// Cache for topic pattern matching results
    pattern_cache: Arc<RwLock<HashMap<String, Vec<Subscriber>>>>, // topic -> vec of matching subscribers
    /// Maximum cache size
    max_cache_size: usize,
    /// Optional gRPC subscriber dispatcher for real delivery
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    subscriber_dispatcher: Option<Arc<SubscriberDispatcher>>,
}

impl Dispatcher {
    /// Create a new dispatcher
    pub fn new(pool: DbPool) -> Self {
        let repository = SqlxSubscriberRepository::new(pool);
        Self {
            subscriber_repository: Arc::new(repository) as Arc<dyn SubscriberRepository>,
            pattern_cache: Arc::new(RwLock::new(HashMap::new())),
            max_cache_size: 1000, // Maximum entries in the pattern cache
            #[cfg(any(feature = "postgres", feature = "mysql"))]
            subscriber_dispatcher: None,
        }
    }

    /// Create a new dispatcher with a custom subscriber repository (for testing)
    pub fn with_repository(subscriber_repository: Arc<dyn SubscriberRepository>) -> Self {
        Self {
            subscriber_repository,
            pattern_cache: Arc::new(RwLock::new(HashMap::new())),
            max_cache_size: 1000,
            #[cfg(any(feature = "postgres", feature = "mysql"))]
            subscriber_dispatcher: None,
        }
    }

    /// Create a new dispatcher with a SubscriberDispatcher for real gRPC delivery
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    pub fn with_subscriber_dispatcher(
        pool: DbPool,
        subscriber_dispatcher: Arc<SubscriberDispatcher>,
    ) -> Self {
        let repository = SqlxSubscriberRepository::new(pool);
        Self {
            subscriber_repository: Arc::new(repository) as Arc<dyn SubscriberRepository>,
            pattern_cache: Arc::new(RwLock::new(HashMap::new())),
            max_cache_size: 1000,
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

        // Cache the result
        {
            let mut cache = self.pattern_cache.write().await;

            // Evict oldest entries if cache is too large
            if cache.len() >= self.max_cache_size {
                // Simple eviction: clear cache when it gets too big
                cache.clear();
            }

            cache.insert(topic.to_string(), matching.clone());
        }

        Ok(matching)
    }

    /// Dispatch a message to all matching subscribers
    pub async fn dispatch(&self, topic: &str, payload: &[u8]) -> Result<usize> {
        // If a SubscriberDispatcher is configured, use it for real gRPC delivery
        #[cfg(any(feature = "postgres", feature = "mysql"))]
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
