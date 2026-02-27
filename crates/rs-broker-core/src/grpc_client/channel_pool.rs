//! gRPC channel pool for efficient connection reuse

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tonic::transport::{Channel, Error};

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
    created_at: std::time::Instant,
    last_used: std::time::Instant,
}

impl PooledChannel {
    fn new(channel: Channel) -> Self {
        let now = std::time::Instant::now();
        Self {
            channel,
            created_at: now,
            last_used: now,
        }
    }

    fn is_expired(&self, idle_timeout: std::time::Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }
}

/// Thread-safe channel pool for gRPC connections
pub struct ChannelPool {
    pools: Arc<RwLock<HashMap<String, Arc<Mutex<Vec<PooledChannel>>>>>>,
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
    pub async fn get_channel(&self, endpoint: &str) -> Result<Channel, Error> {
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
        }

        Ok(())
    }

    /// Create a new channel to the given endpoint
    async fn create_channel(&self, endpoint: &str) -> Result<Channel, Error> {
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_pool_creation() {
        let pool = ChannelPool::default();
        assert_eq!(pool.config.max_channels_per_endpoint, 10);
    }
}