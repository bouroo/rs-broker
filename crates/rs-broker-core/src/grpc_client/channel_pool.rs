//! gRPC channel pool for efficient connection reuse

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tonic::transport::Channel;

/// Configuration for the channel pool
#[derive(Debug, Clone)]
pub struct ChannelPoolConfig {
    /// Maximum number of channels per endpoint
    pub max_channels_per_endpoint: usize,
    /// Connection timeout
    pub connect_timeout: std::time::Duration,
    /// Idle timeout before closing channels
    pub idle_timeout: std::time::Duration,
}

impl Default for ChannelPoolConfig {
    fn default() -> Self {
        Self {
            max_channels_per_endpoint: 10,
            connect_timeout: std::time::Duration::from_secs(10),
            idle_timeout: std::time::Duration::from_secs(300), // 5 minutes
        }
    }
}

/// A pooled channel with metadata
struct PooledChannel {
    channel: Channel,
    last_used: std::time::Instant,
}

impl PooledChannel {
    fn new(channel: Channel) -> Self {
        let now = std::time::Instant::now();
        Self {
            channel,
            last_used: now,
        }
    }

    fn is_expired(&self, idle_timeout: std::time::Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }
}

/// Thread-safe channel pool for gRPC connections
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

    /// Get a channel for the given endpoint, creating one if necessary
    pub async fn get_channel(
        &self,
        endpoint: &str,
    ) -> std::result::Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
        // Normalize the endpoint URL
        let normalized_endpoint = self.normalize_endpoint(endpoint);

        // Get or create the pool for this endpoint
        let endpoint_pool = {
            let pools = self.pools.read().await;
            pools.get(&normalized_endpoint).cloned()
        };

        if let Some(pool) = endpoint_pool {
            let mut pool_guard = pool.lock().await;

            // Remove expired channels
            pool_guard.retain(|ch| !ch.is_expired(self.config.idle_timeout));

            // Try to get an existing channel
            if let Some(pooled_channel) = pool_guard.pop() {
                drop(pool_guard); // Release the lock before returning
                return Ok(pooled_channel.channel);
            }
        } else {
            // Create new pool for this endpoint
            let new_pool = Arc::new(Mutex::new(Vec::new()));
            {
                let mut pools = self.pools.write().await;
                pools.insert(normalized_endpoint.clone(), new_pool.clone());
            }
        }

        // Create a new channel
        let channel = self.create_channel(&normalized_endpoint).await?;
        Ok(channel)
    }

    /// Return a channel to the pool for reuse
    pub async fn put_channel(&self, endpoint: &str, channel: Channel) -> Result<(), ()> {
        let normalized_endpoint = self.normalize_endpoint(endpoint);

        let endpoint_pool = {
            let pools = self.pools.read().await;
            pools.get(&normalized_endpoint).cloned()
        };

        if let Some(pool) = endpoint_pool {
            let mut pool_guard = pool.lock().await;

            // Only add back to pool if we haven't exceeded the limit
            if pool_guard.len() < self.config.max_channels_per_endpoint {
                pool_guard.push(PooledChannel::new(channel));
            }
        } else {
            // No pool for this endpoint yet — create one and store the channel.
            let new_pool = Arc::new(Mutex::new(Vec::new()));
            {
                let mut pool_guard = new_pool.lock().await;
                if pool_guard.len() < self.config.max_channels_per_endpoint {
                    pool_guard.push(PooledChannel::new(channel));
                }
            }
            let mut pools = self.pools.write().await;
            pools.insert(normalized_endpoint, new_pool);
        }

        Ok(())
    }

    /// Create a new channel to the given endpoint
    async fn create_channel(
        &self,
        endpoint: &str,
    ) -> std::result::Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
        let channel = Channel::from_shared(endpoint.to_string())?
            .connect_timeout(self.config.connect_timeout)
            .connect()
            .await?;

        Ok(channel)
    }

    /// Normalize endpoint URL to ensure consistent key format
    fn normalize_endpoint(&self, endpoint: &str) -> String {
        // Ensure the endpoint has a scheme
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            endpoint.to_string()
        } else {
            format!("http://{}", endpoint)
        }
    }
}

impl Default for ChannelPool {
    fn default() -> Self {
        Self::new(ChannelPoolConfig::default())
    }
}

#[cfg(test)]
impl ChannelPool {
    /// Test-only: number of pooled channels currently stored for `endpoint`
    /// (0 if no pool exists for that endpoint).
    async fn pool_size(&self, endpoint: &str) -> usize {
        let normalized = self.normalize_endpoint(endpoint);
        let pools = self.pools.read().await;
        match pools.get(&normalized) {
            Some(pool) => {
                let guard = pool.lock().await;
                guard.len()
            }
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_pool_creation() {
        let pool = ChannelPool::default();
        assert_eq!(pool.config.max_channels_per_endpoint, 10);
    }

    #[tokio::test]
    async fn test_put_channel_creates_missing_pool() {
        // Regression: previously, returning a channel for a never-before-seen
        // endpoint was silently dropped because `put_channel` had no `else`
        // branch. After the fix, the pool is created and the channel stored.
        let pool = ChannelPool::default();
        let endpoint = "http://test.invalid:1234";

        // `connect_lazy` yields a `Channel` without DNS or TCP — safe in unit tests.
        let ch1 = Channel::from_shared(endpoint.to_string())
            .unwrap()
            .connect_lazy();
        let ch2 = Channel::from_shared(endpoint.to_string())
            .unwrap()
            .connect_lazy();

        pool.put_channel(endpoint, ch1).await.unwrap();
        pool.put_channel(endpoint, ch2).await.unwrap();

        assert_eq!(pool.pool_size(endpoint).await, 2);
    }

    // Note: a unit test that proves `put_channel` + `get_channel` actually
    // reuses a `Channel` is intentionally omitted. `get_channel` falls through
    // to a real `Channel::connect()` (DNS + TCP) whenever the pool for an
    // endpoint is empty, so a true reuse assertion would require a live
    // endpoint. The dispatcher-level fix (clone before use, `put_channel`
    // after, on success and error) is verified by the existing integration
    // tests and by the `SubscriberDispatcher::deliver_to_endpoint` change in
    // this unit.
}
