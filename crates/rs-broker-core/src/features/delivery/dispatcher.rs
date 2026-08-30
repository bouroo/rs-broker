//! Subscriber dispatcher with circuit breaker pattern

use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;
use tonic::Status;

use super::channel_pool::{ChannelPool, ChannelPoolConfig};

use crate::features::subscription::ports::SubscriberRepository;
use crate::features::subscription::Subscriber;
use crate::shared::error::{Error, Result};
use crate::shared::topic::matches_any;
use rs_broker_proto::rsbroker::{
    rs_broker_callback_client::RsBrokerCallbackClient, DeliverRequest, DeliverResponse,
};

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
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            open_duration: Duration::from_secs(30),
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
}

impl SubscriberEndpoint {
    /// Create a new subscriber endpoint
    pub fn new(subscriber: Subscriber) -> Self {
        Self {
            circuit_breaker: CircuitBreaker::new(CircuitBreakerConfig::default()),
            subscriber,
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

/// Default upper bound for concurrent in-flight deliveries per fan-out.
///
/// 32 is chosen to give meaningful parallelism for typical workloads (most
/// topics have far fewer than 32 matching subscribers) while keeping the
/// per-process concurrent gRPC call count bounded under bursty fan-out
/// (a 1k-subscriber topic won't open 1k simultaneous connections). With a
/// 10s per-request timeout, worst-case memory is ~32 in-flight requests.
const DEFAULT_FAN_OUT_CONCURRENCY: usize = 32;

/// Subscriber dispatcher for delivering messages to subscribers
pub struct SubscriberDispatcher {
    /// Subscriber repository port
    subscriber_repo: Arc<dyn SubscriberRepository>,
    /// Subscriber endpoints cache
    endpoints: Arc<RwLock<HashMap<String, SubscriberEndpoint>>>,
    /// Timeout for individual delivery requests
    request_timeout: Duration,
    /// Channel pool for gRPC connections
    channel_pool: Arc<ChannelPool>,
    /// Upper bound on concurrent in-flight deliveries per `dispatch_to_all`
    /// call. See [`DEFAULT_FAN_OUT_CONCURRENCY`].
    fan_out_concurrency: usize,
}

impl SubscriberDispatcher {
    /// Create a new subscriber dispatcher
    pub fn new(subscriber_repo: Arc<dyn SubscriberRepository>) -> Self {
        Self {
            subscriber_repo,
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            request_timeout: Duration::from_secs(10),
            channel_pool: Arc::new(ChannelPool::new(ChannelPoolConfig::default())),
            fan_out_concurrency: DEFAULT_FAN_OUT_CONCURRENCY,
        }
    }

    /// Maximum number of concurrent in-flight deliveries per `dispatch_to_all`
    /// call. See [`DEFAULT_FAN_OUT_CONCURRENCY`].
    pub fn fan_out_concurrency(&self) -> usize {
        self.fan_out_concurrency
    }

    /// Load subscribers from database
    pub async fn load_subscribers(&self) -> Result<Vec<Subscriber>> {
        self.subscriber_repo
            .get_all_active()
            .await
            .map_err(Error::from)
    }

    /// Refresh subscriber cache from database
    ///
    /// Merges the database state into the in-memory endpoint map instead of
    /// clearing it: circuit-breaker state survives refreshes, and removed
    /// subscribers drop out. A clear-and-rebuild would reset every breaker
    /// and race in-flight deliveries that record results after the wipe.
    pub async fn refresh_subscribers(&self) -> Result<()> {
        let subscribers = self.load_subscribers().await?;
        self.merge_subscribers(subscribers).await;
        Ok(())
    }

    /// Merge a full active-subscriber snapshot into the endpoint map,
    /// preserving existing circuit breakers for known subscriber IDs.
    async fn merge_subscribers(&self, subscribers: Vec<Subscriber>) {
        let active_ids: std::collections::HashSet<String> =
            subscribers.iter().map(|s| s.id.to_string()).collect();

        let mut endpoints = self.endpoints.write().await;

        // Deactivated/removed subscribers leave the cache; everyone else keeps
        // their breaker history.
        endpoints.retain(|id, _| active_ids.contains(id));

        for subscriber in subscribers {
            match endpoints.get_mut(&subscriber.id.to_string()) {
                Some(endpoint) => endpoint.subscriber = subscriber,
                None => {
                    endpoints.insert(
                        subscriber.id.to_string(),
                        SubscriberEndpoint::new(subscriber),
                    );
                }
            }
        }
    }

    /// Get subscribers matching a topic
    pub async fn get_matching_subscribers(&self, topic: &str) -> Vec<Subscriber> {
        let endpoints = self.endpoints.read().await;

        endpoints
            .values()
            .filter(|e| matches_any(topic, &e.subscriber.topic_patterns))
            .map(|e| e.subscriber.clone())
            .collect()
    }

    /// Dispatch a message to a subscriber
    pub async fn dispatch(
        &self,
        subscriber: &Subscriber,
        request: DeliverRequest,
    ) -> DeliveryResult {
        let subscriber_id = subscriber.id.to_string();

        // Get or create endpoint and check circuit breaker under the write lock
        let endpoint_addr = {
            let mut endpoints = self.endpoints.write().await;
            let endpoint = endpoints
                .entry(subscriber_id.clone())
                .or_insert_with(|| SubscriberEndpoint::new(subscriber.clone()));

            // Check circuit breaker
            if !endpoint.circuit_breaker.can_execute() {
                return DeliveryResult {
                    subscriber_id,
                    success: false,
                    error: Some("Circuit breaker open".to_string()),
                    retry: true,
                    retry_delay_ms: 1000,
                };
            }

            endpoint.endpoint().to_string()
        };

        // Deliver with timeout (lock released during I/O)
        let result = timeout(
            self.request_timeout,
            self.deliver_to_endpoint(&endpoint_addr, request),
        )
        .await;

        // Record result in circuit breaker if the endpoint is still present.
        // Between releasing the write lock for the I/O call and re-acquiring it
        // here, `refresh_subscribers` may have removed this endpoint (e.g. the
        // subscriber was deactivated); we must handle that gracefully instead
        // of panicking.
        {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&subscriber_id) {
                match &result {
                    Ok(Ok(_)) => endpoint.circuit_breaker.record_success(),
                    Ok(Err(_)) | Err(_) => endpoint.circuit_breaker.record_failure(),
                }
            } else {
                tracing::warn!(
                    subscriber_id = %subscriber_id,
                    "endpoint vanished mid-delivery (likely concurrent refresh_subscribers); skipping circuit-breaker recording"
                );
            }
        }

        outcome_to_delivery_result(subscriber_id, result)
    }

    /// Deliver message to a specific endpoint
    //
    // `tonic::Status` as the Err variant exceeds clippy's `result_large_err`
    // threshold; matching the generated proto traits' own exemption.
    #[allow(clippy::result_large_err)]
    async fn deliver_to_endpoint(
        &self,
        endpoint: &str,
        request: DeliverRequest,
    ) -> std::result::Result<DeliverResponse, Status> {
        // Get a channel from the pool
        let channel = self
            .channel_pool
            .get_channel(endpoint)
            .await
            .map_err(|e| Status::unavailable(format!("Failed to get channel: {}", e)))?;

        // Clone for the client; the original is returned to the pool afterwards.
        // Channel::clone is cheap (Arc-based internal pool).
        let mut client = RsBrokerCallbackClient::new(channel.clone());

        let result = client.deliver(request).await.map(|r| r.into_inner());

        // Return the channel for reuse regardless of outcome. Tonic's Channel
        // handles reconnection internally, so even a failed call's channel can
        // be retried on the next delivery.
        let _ = self.channel_pool.put_channel(endpoint, channel).await;

        result
    }

    /// Dispatch to all matching subscribers
    ///
    /// Fan-out is bounded by `fan_out_concurrency` (see
    /// [`DEFAULT_FAN_OUT_CONCURRENCY`]) using `buffer_unordered`, so total
    /// delivery latency is bounded by the slowest matching subscriber rather
    /// than the sum of per-subscriber latencies. Results are returned in
    /// completion order, not submission order; callers identify results by
    /// `DeliveryResult::subscriber_id` rather than Vec position.
    pub async fn dispatch_to_all(
        &self,
        topic: &str,
        request: DeliverRequest,
    ) -> Vec<DeliveryResult> {
        let subscribers = self.get_matching_subscribers(topic).await;

        if subscribers.is_empty() {
            return Vec::new();
        }

        let concurrency = self.fan_out_concurrency.max(1).min(subscribers.len());

        stream::iter(subscribers)
            .map(|subscriber| {
                let request = request.clone();
                let subscriber = subscriber.clone();
                async move { self.dispatch(&subscriber, request).await }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await
    }
}

/// Map a (timed) delivery outcome to a `DeliveryResult` without touching any state.
///
/// Extracted so the mapping is unit-testable in isolation from the dispatcher's
/// internal locks. This is the same code path that runs in `dispatch` after the
/// circuit-breaker recording step, so a panic here would also be observable
/// under the TOCTOU race where the endpoint vanished.
fn outcome_to_delivery_result(
    subscriber_id: String,
    result: std::result::Result<
        std::result::Result<DeliverResponse, Status>,
        tokio::time::error::Elapsed,
    >,
) -> DeliveryResult {
    match result {
        Ok(Ok(response)) => DeliveryResult {
            subscriber_id,
            success: response.success,
            error: if response.error.is_empty() {
                None
            } else {
                Some(response.error)
            },
            retry: response.retry,
            retry_delay_ms: response.retry_delay_ms,
        },
        Ok(Err(e)) => DeliveryResult {
            subscriber_id,
            success: false,
            error: Some(e.message().to_string()),
            retry: true,
            retry_delay_ms: 1000,
        },
        Err(_) => DeliveryResult {
            subscriber_id,
            success: false,
            error: Some("Request timeout".to_string()),
            retry: true,
            retry_delay_ms: 2000,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Documents the no-panic contract for the TOCTOU window between the two
    /// endpoint-map lock acquisitions in `dispatch`. After `refresh_subscribers`
    /// clears the endpoint map concurrently with an in-flight delivery, the
    /// post-I/O branch takes the `else` arm (skip circuit-breaker recording,
    /// emit a `tracing::warn!`) and returns a `DeliveryResult` built from the
    /// actual call outcome. The mapping itself is exhaustively covered by
    /// `outcome_to_delivery_result_*` below; this test asserts the contract by
    /// verifying the mapping function never panics across all three branches.
    #[tokio::test]
    async fn outcome_to_delivery_result_does_not_panic_on_any_outcome() {
        let _ = outcome_to_delivery_result("sub".into(), Ok(Ok(DeliverResponse::default())));
        let _ = outcome_to_delivery_result("sub".into(), Ok(Err(Status::unavailable("down"))));

        // Construct an Elapsed via a real (zero-duration) timeout — `Elapsed::new`
        // is `pub(crate)` so we cannot call it directly.
        let elapsed: tokio::time::error::Elapsed = tokio::time::timeout(
            std::time::Duration::from_millis(0),
            tokio::time::sleep(std::time::Duration::from_secs(1)),
        )
        .await
        .unwrap_err();
        let _ = outcome_to_delivery_result("sub".into(), Err(elapsed));
    }

    #[test]
    fn outcome_to_delivery_result_maps_success() {
        let r = outcome_to_delivery_result(
            "sub-1".into(),
            Ok(Ok(DeliverResponse {
                success: true,
                error: String::new(),
                retry: false,
                retry_delay_ms: 0,
            })),
        );
        assert_eq!(r.subscriber_id, "sub-1");
        assert!(r.success);
        assert!(r.error.is_none());
        assert!(!r.retry);
    }

    #[test]
    fn outcome_to_delivery_result_maps_grpc_error() {
        let r = outcome_to_delivery_result("sub-2".into(), Ok(Err(Status::unavailable("boom"))));
        assert!(!r.success);
        assert!(r.retry);
        assert_eq!(r.error.as_deref(), Some("boom"));
        assert_eq!(r.retry_delay_ms, 1000);
    }

    #[tokio::test]
    async fn outcome_to_delivery_result_maps_timeout() {
        let elapsed: tokio::time::error::Elapsed = tokio::time::timeout(
            std::time::Duration::from_millis(0),
            tokio::time::sleep(std::time::Duration::from_secs(1)),
        )
        .await
        .unwrap_err();
        let r = outcome_to_delivery_result("sub-3".into(), Err(elapsed));
        assert!(!r.success);
        assert!(r.retry);
        assert_eq!(r.error.as_deref(), Some("Request timeout"));
        assert_eq!(r.retry_delay_ms, 2000);
    }

    /// `dispatch_to_all` on an empty subscriber set must short-circuit and
    /// return an empty `Vec` without touching any channels or circuit
    /// breakers. This guards the early-return path added when fan-out was
    /// converted from a sequential loop to a concurrent stream.
    #[tokio::test]
    async fn dispatch_to_all_empty_subscriber_set_returns_empty_vec() {
        let sd = SubscriberDispatcher::new(test_subscriber_repository());
        let results = sd
            .dispatch_to_all("no-such-topic", DeliverRequest::default())
            .await;
        assert!(
            results.is_empty(),
            "empty subscriber set must produce empty result vec, got {results:?}"
        );
    }

    /// Verifies the `fan_out_concurrency` field is initialized to the
    /// documented default. Callers can read this via `fan_out_concurrency()`
    /// to inspect (and tests can pin regressions on) the bound.
    #[tokio::test]
    async fn default_fan_out_concurrency_matches_documented_constant() {
        let sd = SubscriberDispatcher::new(test_subscriber_repository());
        assert_eq!(sd.fan_out_concurrency(), DEFAULT_FAN_OUT_CONCURRENCY);
        assert!(sd.fan_out_concurrency() >= 1);
    }

    /// Exercises the concurrent fan-out path through the circuit-breaker
    /// fast-fail branch (no live gRPC channels required). We pre-populate
    /// the endpoint cache, open every endpoint's circuit via
    /// `record_failure`, then call `dispatch_to_all` and assert one
    /// fast-fail `DeliveryResult` per matching subscriber with the correct
    /// `subscriber_id`. This proves the `buffer_unordered` refactor still
    /// produces a result for every subscriber and preserves `subscriber_id`.
    #[tokio::test]
    async fn dispatch_to_all_returns_one_fast_fail_result_per_subscriber() {
        use crate::grpc_client::dispatcher::{CircuitBreaker, CircuitBreakerConfig};

        let sd = SubscriberDispatcher::new(test_subscriber_repository());

        // Three subscribers all matching topic "orders.created".
        let subs = vec![
            Subscriber::new(
                "svc-a".into(),
                "http://a:1".into(),
                vec!["orders.created".into()],
            ),
            Subscriber::new(
                "svc-b".into(),
                "http://b:1".into(),
                vec!["orders.created".into()],
            ),
            Subscriber::new(
                "svc-c".into(),
                "http://c:1".into(),
                vec!["orders.created".into()],
            ),
        ];
        let expected_ids: std::collections::HashSet<String> =
            subs.iter().map(|s| s.id.to_string()).collect();

        // Seed the endpoint cache (normally `refresh_subscribers` does this)
        // and open every circuit so the fast-fail branch fires inside dispatch.
        {
            let mut endpoints = sd.endpoints.write().await;
            for s in &subs {
                let mut ep = SubscriberEndpoint::new(s.clone());
                let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
                    failure_threshold: 1,
                    ..CircuitBreakerConfig::default()
                });
                cb.record_failure(); // opens immediately
                ep.circuit_breaker = cb;
                endpoints.insert(s.id.to_string(), ep);
            }
        }

        let results = sd
            .dispatch_to_all("orders.created", DeliverRequest::default())
            .await;

        assert_eq!(
            results.len(),
            subs.len(),
            "expected one DeliveryResult per matching subscriber"
        );

        let got_ids: std::collections::HashSet<String> =
            results.iter().map(|r| r.subscriber_id.clone()).collect();
        assert_eq!(
            got_ids, expected_ids,
            "every matching subscriber must appear in the results"
        );

        for r in &results {
            assert!(!r.success, "open circuit must surface as failure");
            assert!(r.retry);
            assert_eq!(r.error.as_deref(), Some("Circuit breaker open"));
            assert_eq!(r.retry_delay_ms, 1000);
        }
    }

    /// Merge-refresh must preserve circuit-breaker state for subscribers that
    /// stay registered: only the subscriber payload is updated. Under the old
    /// clear-and-rebuild, every breaker reset on each refresh and failures
    /// accumulated by a live subscriber were silently forgotten.
    #[tokio::test]
    async fn merge_subscribers_preserves_circuit_breaker_state() {
        use crate::grpc_client::dispatcher::{CircuitBreaker, CircuitBreakerConfig};

        let sd = SubscriberDispatcher::new(test_subscriber_repository());
        let s = Subscriber::new("svc-a".into(), "http://a:1".into(), vec!["orders.*".into()]);
        let id = s.id.to_string();

        {
            let mut endpoints = sd.endpoints.write().await;
            let mut ep = SubscriberEndpoint::new(s.clone());
            let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
                failure_threshold: 1,
                ..CircuitBreakerConfig::default()
            });
            cb.record_failure(); // open circuit — history that must survive
            ep.circuit_breaker = cb;
            endpoints.insert(id.clone(), ep);
        }

        // Same subscriber id, updated endpoint address.
        let mut updated =
            Subscriber::new("svc-a".into(), "http://a:2".into(), vec!["orders.*".into()]);
        updated.id = s.id;

        sd.merge_subscribers(vec![updated]).await;

        let endpoints = sd.endpoints.read().await;
        let ep = endpoints.get(&id).expect("known subscriber must remain");
        assert_eq!(ep.endpoint(), "http://a:2", "subscriber data is refreshed");
        assert_eq!(
            ep.circuit_breaker.state(),
            CircuitBreakerState::Open,
            "breaker history must survive the merge"
        );
    }

    /// Merge-refresh must drop endpoints for subscribers no longer active and
    /// insert brand-new ones, so the cache converges to the database snapshot.
    #[tokio::test]
    async fn merge_subscribers_removes_gone_and_adds_new() {
        let sd = SubscriberDispatcher::new(test_subscriber_repository());

        let stale = Subscriber::new("svc-old".into(), "http://old:1".into(), vec!["*".into()]);
        {
            let mut endpoints = sd.endpoints.write().await;
            endpoints.insert(stale.id.to_string(), SubscriberEndpoint::new(stale.clone()));
        }

        let fresh = Subscriber::new(
            "svc-new".into(),
            "http://new:1".into(),
            vec!["bills.#".into()],
        );
        sd.merge_subscribers(vec![fresh.clone()]).await;

        let endpoints = sd.endpoints.read().await;
        assert!(
            !endpoints.contains_key(&stale.id.to_string()),
            "deactivated subscriber must be removed"
        );
        assert!(
            endpoints.contains_key(&fresh.id.to_string()),
            "new subscriber must be inserted"
        );
    }

    /// In-memory subscriber-repository stub for tests that never hit the
    /// transport or persistence paths: `load_subscribers` is only exercised by
    /// `refresh_subscribers`, which these tests do not call. Every test
    /// replaces `test_db_pool`; the port stub keeps the use case free of any
    /// sqlx dependency in its constructor (clean-architecture inversion).
    fn test_subscriber_repository() -> Arc<dyn SubscriberRepository> {
        struct NeverQueried;

        #[async_trait::async_trait]
        impl SubscriberRepository for NeverQueried {
            async fn create(
                &self,
                _s: &Subscriber,
            ) -> std::result::Result<(), crate::features::subscription::ports::SubscriberError>
            {
                unreachable!("test stub must not be queried")
            }
            async fn get_by_id(
                &self,
                _id: uuid::Uuid,
            ) -> std::result::Result<
                Subscriber,
                crate::features::subscription::ports::SubscriberError,
            > {
                unreachable!("test stub must not be queried")
            }
            async fn get_all_active(
                &self,
            ) -> std::result::Result<
                Vec<Subscriber>,
                crate::features::subscription::ports::SubscriberError,
            > {
                Ok(Vec::new())
            }
            async fn update(
                &self,
                _s: &Subscriber,
            ) -> std::result::Result<(), crate::features::subscription::ports::SubscriberError>
            {
                unreachable!("test stub must not be queried")
            }
            async fn delete(
                &self,
                _id: uuid::Uuid,
            ) -> std::result::Result<(), crate::features::subscription::ports::SubscriberError>
            {
                unreachable!("test stub must not be queried")
            }
            async fn deactivate(
                &self,
                _id: uuid::Uuid,
            ) -> std::result::Result<(), crate::features::subscription::ports::SubscriberError>
            {
                unreachable!("test stub must not be queried")
            }
        }

        Arc::new(NeverQueried)
    }
}
