//! gRPC callback notifier adapter: implements the delivery feature's
//! `SubscriberNotifier` port over the tonic callback client with pooled
//! channels. This adapter is the only place converting between the delivery
//! feature's transport-free DTOs and the generated proto types.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};
use tonic::transport::Channel;

use rs_broker_core::features::delivery::ports::{
    DeliverNotification, NotificationError, NotificationOutcome, SubscriberNotifier,
};
use rs_broker_proto::rsbroker::{
    rs_broker_callback_client::RsBrokerCallbackClient, DeliverRequest,
};

/// Connection pool for gRPC channels to subscriber endpoints.
///
/// Moved from `rs-broker-core::grpc_client::channel_pool` (a tonic driver does
/// not belong in the feature layer). Behaviour unchanged, including the
/// entry-API fix that keeps concurrent cold-path callers from overwriting a
/// concurrently created endpoint pool.
#[derive(Debug, Clone)]
pub struct ChannelPoolConfig {
    /// Maximum number of channels per endpoint
    pub max_channels_per_endpoint: usize,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Idle timeout before closing channels
    pub idle_timeout: Duration,
}

impl Default for ChannelPoolConfig {
    fn default() -> Self {
        Self {
            max_channels_per_endpoint: 10,
            connect_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// A pooled channel with metadata
struct PooledChannel {
    channel: Channel,
    last_used: Instant,
}

impl PooledChannel {
    fn new(channel: Channel) -> Self {
        Self {
            channel,
            last_used: Instant::now(),
        }
    }

    fn is_expired(&self, idle_timeout: Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }
}

type EndpointPool = Arc<Mutex<Vec<PooledChannel>>>;
type PoolsMap = HashMap<String, EndpointPool>;

pub struct ChannelPool {
    pools: Arc<RwLock<PoolsMap>>,
    config: ChannelPoolConfig,
}

impl ChannelPool {
    /// Create a new channel pool with the given configuration
    pub fn new(config: ChannelPoolConfig) -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Get a channel for the given endpoint, creating one if necessary.
    ///
    /// The miss path re-checks under the write lock via `entry` so a
    /// concurrently created pool is never overwritten.
    pub async fn get_channel(
        &self,
        endpoint: &str,
    ) -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
        let normalized_endpoint = self.normalize_endpoint(endpoint);

        let endpoint_pool = {
            let pools = self.pools.read().await;
            pools.get(&normalized_endpoint).cloned()
        };

        let endpoint_pool = match endpoint_pool {
            Some(pool) => pool,
            None => {
                let mut pools = self.pools.write().await;
                pools
                    .entry(normalized_endpoint.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(Vec::new())))
                    .clone()
            }
        };

        {
            let mut pool_guard = endpoint_pool.lock().await;
            pool_guard.retain(|ch| !ch.is_expired(self.config.idle_timeout));
            if let Some(pooled_channel) = pool_guard.pop() {
                drop(pool_guard); // Release the lock before returning
                return Ok(pooled_channel.channel);
            }
        }

        self.create_channel(&normalized_endpoint).await
    }

    /// Return a channel to the pool for reuse. The miss path re-checks under
    /// the write lock via `entry` so a concurrently created pool is never
    /// overwritten.
    async fn put_channel(&self, endpoint: &str, channel: Channel) {
        let normalized_endpoint = self.normalize_endpoint(endpoint);

        let endpoint_pool = {
            let pools = self.pools.read().await;
            pools.get(&normalized_endpoint).cloned()
        };

        let endpoint_pool = match endpoint_pool {
            Some(pool) => pool,
            None => {
                let mut pools = self.pools.write().await;
                pools
                    .entry(normalized_endpoint.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(Vec::new())))
                    .clone()
            }
        };

        let mut pool_guard = endpoint_pool.lock().await;
        if pool_guard.len() < self.config.max_channels_per_endpoint {
            pool_guard.push(PooledChannel::new(channel));
        }
    }

    async fn create_channel(
        &self,
        endpoint: &str,
    ) -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
        let channel = Channel::from_shared(endpoint.to_string())?
            .connect_timeout(self.config.connect_timeout)
            .connect()
            .await?;
        Ok(channel)
    }

    /// Normalize endpoint URL to ensure consistent key format
    fn normalize_endpoint(&self, endpoint: &str) -> String {
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            endpoint.to_string()
        } else {
            format!("http://{}", endpoint)
        }
    }
}

/// Tonic-backed implementation of the delivery feature's notifier port.
pub struct GrpcSubscriberNotifier {
    channel_pool: ChannelPool,
}

impl Default for GrpcSubscriberNotifier {
    fn default() -> Self {
        Self {
            channel_pool: ChannelPool::new(ChannelPoolConfig::default()),
        }
    }
}

impl GrpcSubscriberNotifier {
    /// Create a notifier over a fresh channel pool with default settings.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SubscriberNotifier for GrpcSubscriberNotifier {
    async fn notify(
        &self,
        endpoint: &str,
        notification: DeliverNotification,
    ) -> std::result::Result<NotificationOutcome, NotificationError> {
        let channel =
            self.channel_pool.get_channel(endpoint).await.map_err(|e| {
                NotificationError::Transport(format!("Failed to get channel: {}", e))
            })?;

        // Return a clone to the client and put the pooled channel back
        // regardless of outcome; tonic's Channel handles reconnection
        // internally, matching the historical dispatcher behaviour.
        let mut client = RsBrokerCallbackClient::new(channel.clone());
        self.channel_pool
            .put_channel(endpoint, channel.clone())
            .await;

        let request = DeliverRequest {
            message_id: notification.message_id,
            topic: notification.topic,
            payload: notification.payload,
            headers: notification
                .headers
                .into_iter()
                .map(|(key, value)| rs_broker_proto::rsbroker::Header { key, value })
                .collect(),
            timestamp: notification.timestamp,
            event_type: notification.event_type,
            retry_count: notification.retry_count,
        };

        let response = client
            .deliver(request)
            .await
            .map(|r| r.into_inner())
            .map_err(|e| NotificationError::Transport(e.message().to_string()))?;

        Ok(NotificationOutcome {
            success: response.success,
            error: response.error,
            retry: response.retry,
            retry_delay_ms: response.retry_delay_ms,
        })
    }
}
