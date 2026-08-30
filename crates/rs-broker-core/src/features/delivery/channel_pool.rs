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

        // Get or create the pool for this endpoint. The miss path re-checks
        // under the write lock via `entry` so a concurrently created pool is
        // never overwritten (whose buffered channels would be dropped).
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

            // Remove expired channels
            pool_guard.retain(|ch| !ch.is_expired(self.config.idle_timeout));

            // Try to get an existing channel
            if let Some(pooled_channel) = pool_guard.pop() {
                drop(pool_guard); // Release the lock before returning
                return Ok(pooled_channel.channel);
            }
        }

        // Pool empty: create a new channel
        let channel = self.create_channel(&normalized_endpoint).await?;
        Ok(channel)
    }

    /// Return a channel to the pool for reuse
    ///
    // The `()` error variant is the historical public signature; the call
    // cannot fail today, but changing the type would break the public API.
    #[allow(clippy::result_unit_err)]
    pub async fn put_channel(&self, endpoint: &str, channel: Channel) -> Result<(), ()> {
        let normalized_endpoint = self.normalize_endpoint(endpoint);

        // Same read-first/entry-on-miss discipline as `get_channel`: never
        // overwrite a pool another caller just installed, or its channels leak.
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

        // Only add back to pool if we haven't exceeded the limit
        let mut pool_guard = endpoint_pool.lock().await;
        if pool_guard.len() < self.config.max_channels_per_endpoint {
            pool_guard.push(PooledChannel::new(channel));
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

    /// Regression for the put/get channel-pool TOCTOU: concurrent cold-path
    /// callers raced an unconditional `pools.insert`, and the last writer
    /// replaced the map entry — silently dropping every channel the earlier
    /// callers had stored. The `Barrier` forces all callers through the
    /// miss branch simultaneously, so `entry().or_insert_with` must be the
    /// only writer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_cold_put_channels_are_not_dropped() {
        let pool = Arc::new(ChannelPool::default());
        let endpoint = "http://race.invalid:1";
        let barrier = Arc::new(tokio::sync::Barrier::new(16));

        let handles: Vec<_> = (0..16)
            .map(|_| {
                let pool = Arc::clone(&pool);
                let endpoint = endpoint.to_string();
                let barrier = Arc::clone(&barrier);
                tokio::spawn(async move {
                    barrier.wait().await;
                    let ch = Channel::from_shared(format!("{endpoint}#"))
                        .unwrap()
                        .connect_lazy();
                    pool.put_channel(&endpoint, ch).await.unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // max_channels_per_endpoint is 10 by default. Under the old
        // insert-overwrite race the surviving map entry was the last writer's
        // freshly created pool, so the size landed well below the cap
        // non-deterministically; with the entry fix every lost race simply
        // adopts the winner's pool and the cap is always reached.
        assert_eq!(
            pool.pool_size(endpoint).await,
            pool.config.max_channels_per_endpoint,
            "concurrent puts must fill the pool to capacity, none lost"
        );
    }
}
