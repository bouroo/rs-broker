//! Subscriber dispatcher with circuit breaker pattern

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, OnceCell};
use tokio::time::timeout;
use async_trait::async_trait;
use tonic::{transport::Channel, Response, Status};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::channel_pool::{ChannelPool, ChannelPoolConfig};

use rs_broker_db::{Subscriber, DbPool, SqlxSubscriberRepository};
use rs_broker_proto::rsbroker::{
    rs_broker_callback_client::RsBrokerCallbackClient,
    DeliverRequest, DeliverResponse,
};
use crate::error::{Error, Result};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitBreakerState {
    /// Circuit is closed, requests flow normally
    Closed,
    /// Circuit is open, requests fail fast
    Open,
    /// Circuit is half-open, testing if service recovered
    HalfOpen,
}

/// Circuit breaker for downstream gRPC calls
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Current state
    state: CircuitBreakerState,
    /// Number of consecutive failures
    failures: u32,
    /// Number of consecutive successes (for half-open)
    successes: u32,
    /// Time when circuit was opened
    opened_at: Option<Instant>,
    /// Configuration
    config: CircuitBreakerConfig,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold to open circuit
    pub failure_threshold: u32,
    /// Success threshold to close circuit from half-open
    pub success_threshold: u32,
    /// Duration to wait before trying half-open
    pub open_duration: Duration,
    /// Timeout for requests
    pub request_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            open_duration: Duration::from_secs(30),
            request_timeout: Duration::from_secs(10),
        }
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failures: 0,
            successes: 0,
            opened_at: None,
            config,
        }
    }
    
    /// Check if request can proceed
    pub fn can_execute(&mut self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                // Check if we should try half-open
                if let Some(opened_at) = self.opened_at {
                    if opened_at.elapsed() >= self.config.open_duration {
                        self.state = CircuitBreakerState::HalfOpen;
                        self.successes = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }
    
    /// Record a successful call
    pub fn record_success(&mut self) {
        match self.state {
            CircuitBreakerState::Closed => {
                self.failures = 0;
            }
            CircuitBreakerState::HalfOpen => {
                self.successes += 1;
                if self.successes >= self.config.success_threshold {
                    self.state = CircuitBreakerState::Closed;
                    self.failures = 0;
                    self.successes = 0;
                    self.opened_at = None;
                }
            }
            CircuitBreakerState::Open => {
                // Should not happen, but handle gracefully
                self.state = CircuitBreakerState::HalfOpen;
                self.successes = 0;
            }
        }
    }
    
    /// Record a failed call
    pub fn record_failure(&mut self) {
        self.failures += 1;
        
        match self.state {
            CircuitBreakerState::Closed => {
                if self.failures >= self.config.failure_threshold {
                    self.state = CircuitBreakerState::Open;
                    self.opened_at = Some(Instant::now());
                }
            }
            CircuitBreakerState::HalfOpen => {
                // Any failure in half-open goes back to open
                self.state = CircuitBreakerState::Open;
                self.opened_at = Some(Instant::now());
                self.successes = 0;
            }
            CircuitBreakerState::Open => {
                // Already open, just update the timestamp
                self.opened_at = Some(Instant::now());
            }
        }
    }
    
    /// Get current state
    pub fn state(&self) -> CircuitBreakerState {
        self.state
    }
}

/// Subscriber endpoint with circuit breaker
#[derive(Debug)]
pub struct SubscriberEndpoint {
    /// Subscriber info
    pub subscriber: Subscriber,
    /// Circuit breaker
    pub circuit_breaker: CircuitBreaker,
    /// Cached channel (if connected)
    channel: Option<Channel>,
    /// Channel pool for concurrent requests
    channel_pool: Arc<RwLock<Vec<Channel>>>,
    /// Maximum channels in pool
    max_channels: usize,
}

impl SubscriberEndpoint {
    /// Create a new subscriber endpoint
    pub fn new(subscriber: Subscriber) -> Self {
        Self {
            circuit_breaker: CircuitBreaker::new(CircuitBreakerConfig::default()),
            subscriber,
            channel: None,
            channel_pool: Arc::new(RwLock::new(Vec::new())),
            max_channels: 5, // Default max channels in pool
        }
    }
    
    /// Get the endpoint address
    pub fn endpoint(&self) -> &str {
        &self.subscriber.grpc_endpoint
    }
}

/// Delivery result
#[derive(Debug)]
pub struct DeliveryResult {
    /// Subscriber ID
    pub subscriber_id: String,
    /// Whether delivery was successful
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Whether to retry
    pub retry: bool,
    /// Suggested retry delay
    pub retry_delay_ms: i64,
}

/// Subscriber dispatcher for delivering messages to subscribers
pub struct SubscriberDispatcher {
    /// Database pool
    db_pool: DbPool,
    /// Subscriber endpoints cache
    endpoints: Arc<RwLock<HashMap<String, SubscriberEndpoint>>>,
    /// Default timeout for deliveries
    default_timeout: Duration,
    /// Channel pool for gRPC connections
    channel_pool: Arc<ChannelPool>,
}

impl SubscriberDispatcher {
    /// Create a new subscriber dispatcher
    pub fn new(db_pool: DbPool) -> Self {
        Self {
            db_pool,
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            default_timeout: Duration::from_secs(10),
            channel_pool: Arc::new(ChannelPool::new(ChannelPoolConfig::default())),
        }
    }
    
    /// Load subscribers from database
    pub async fn load_subscribers(&self) -> Result<Vec<Subscriber>> {
        let repo = SqlxSubscriberRepository::new(self.db_pool.clone());
        repo.get_all_active().await.map_err(Error::from)
    }
    
    /// Refresh subscriber cache from database
    pub async fn refresh_subscribers(&self) -> Result<()> {
        let subscribers = self.load_subscribers().await?;
        let mut endpoints = self.endpoints.write().await;
        
        endpoints.clear();
        for subscriber in subscribers {
            endpoints.insert(subscriber.id.to_string(), SubscriberEndpoint::new(subscriber));
        }
        
        Ok(())
    }
    
    /// Get subscribers matching a topic
    pub async fn get_matching_subscribers(&self, topic: &str) -> Vec<Subscriber> {
        let endpoints = self.endpoints.read().await;
        
        endpoints
            .values()
            .filter(|e| self.topic_matches(&e.subscriber.topic_patterns, topic))
            .map(|e| e.subscriber.clone())
            .collect()
    }
    
    /// Check if topic matches any of the patterns
    fn topic_matches(&self, patterns: &[String], topic: &str) -> bool {
        for pattern in patterns {
            if self.matches_pattern(pattern, topic) {
                return true;
            }
        }
        false
    }
    
    /// Simple wildcard matching (supports * and ?)
    fn matches_pattern(&self, pattern: &str, topic: &str) -> bool {
        // Exact match
        if pattern == topic {
            return true;
        }
        
        // Wildcard matching
        let pattern_parts: Vec<&str> = pattern.split('.').collect();
        let topic_parts: Vec<&str> = topic.split('.').collect();
        
        self.match_parts(&pattern_parts, &topic_parts)
    }
    
    fn match_parts(&self, pattern: &[&str], topic: &[&str]) -> bool {
        match (pattern.first(), topic.first()) {
            (Some(&"*"), _) => {
                // * matches anything
                true
            }
            (Some(p), Some(t)) if p == t => {
                // Parts match, continue
                self.match_parts(&pattern[1..], &topic[1..])
            }
            (Some(p), Some(_)) if p.contains('*') => {
                // Pattern has wildcard, try to match
                let remaining_pattern = &pattern[1..];
                for i in 0..=topic.len() {
                    if self.match_parts(remaining_pattern, &topic[i..]) {
                        return true;
                    }
                }
                false
            }
            (Some(_), Some(_)) => {
                // Parts don't match
                false
            }
            (None, None) => {
                // Both exhausted
                true
            }
            (None, Some(_)) => {
                // Pattern exhausted but topic has more parts
                false
            }
            (Some(_), None) => {
                // Topic exhausted but pattern has more parts
                false
            }
        }
    }
    
    /// Dispatch a message to a subscriber
    pub async fn dispatch(&self, subscriber: &Subscriber, request: DeliverRequest) -> DeliveryResult {
        // Get or create endpoint
        let endpoint = {
            let mut endpoints = self.endpoints.write().await;
            endpoints
                .entry(subscriber.id.to_string())
                .or_insert_with(|| SubscriberEndpoint::new(subscriber.clone()))
                .clone()
        };
        
        // Check circuit breaker
        if !endpoint.circuit_breaker.can_execute() {
            return DeliveryResult {
                subscriber_id: subscriber.id.to_string(),
                success: false,
                error: Some("Circuit breaker open".to_string()),
                retry: true,
                retry_delay_ms: 1000,
            };
        }
        
        // Try to deliver with timeout
        let result = timeout(
            endpoint.circuit_breaker.config.request_timeout,
            self.deliver_to_endpoint(endpoint.endpoint(), request),
        )
        .await;
        
        // Update circuit breaker and return result
        match result {
            Ok(Ok(response)) => {
                // Success
                DeliveryResult {
                    subscriber_id: subscriber.id.to_string(),
                    success: response.success,
                    error: if response.error.is_empty() { None } else { Some(response.error) },
                    retry: response.retry,
                    retry_delay_ms: response.retry_delay_ms,
                }
            }
            Ok(Err(e)) => {
                // gRPC error
                DeliveryResult {
                    subscriber_id: subscriber.id.to_string(),
                    success: false,
                    error: Some(e.message().to_string()),
                    retry: true,
                    retry_delay_ms: 1000,
                }
            }
            Err(_) => {
                // Timeout
                DeliveryResult {
                    subscriber_id: subscriber.id.to_string(),
                    success: false,
                    error: Some("Request timeout".to_string()),
                    retry: true,
                    retry_delay_ms: 2000,
                }
            }
        }
    }
    
    /// Deliver message to a specific endpoint
    async fn deliver_to_endpoint(
        &self,
        endpoint: &str,
        request: DeliverRequest,
    ) -> Result<DeliverResponse, Status> {
        // Get a channel from the pool
        let channel = self.channel_pool.get_channel(endpoint).await
            .map_err(|e| Status::unavailable(format!("Failed to get channel: {}", e)))?;
        
        let mut client = RsBrokerCallbackClient::new(channel);
        
        let response = client
            .deliver(request)
            .await
            .map(Response::into_inner);
            
        // Return the channel to the pool for reuse
        if let Ok(ch) = client.into_inner().ready().await {
            let _ = self.channel_pool.put_channel(endpoint, ch).await;
        }
        
        response
    }
    
    /// Dispatch to all matching subscribers
    pub async fn dispatch_to_all(
        &self,
        topic: &str,
        request: DeliverRequest,
    ) -> Vec<DeliveryResult> {
        let subscribers = self.get_matching_subscribers(topic).await;
        
        let mut results = Vec::new();
        for subscriber in subscribers {
            let result = self.dispatch(&subscriber, request.clone()).await;
            results.push(result);
        }
        
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_topic_matching() {
        let dispatcher = SubscriberDispatcher::new(rs_broker_db::DbPool::new_mock(""));
        
        // Exact match
        assert!(dispatcher.topic_matches(&["orders.created".to_string()], "orders.created"));
        
        // Wildcard match
        assert!(dispatcher.topic_matches(&["orders.*".to_string()], "orders.created"));
        assert!(dispatcher.topic_matches(&["orders.*".to_string()], "orders.updated"));
        
        // No match
        assert!(!dispatcher.topic_matches(&["orders.created".to_string()], "payments.created"));
    }
}
