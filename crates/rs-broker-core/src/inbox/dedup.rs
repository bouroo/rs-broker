//! Deduplication logic

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Deduplicator for inbox messages
pub struct Deduplicator {
    seen: Arc<RwLock<HashSet<String>>>,
    max_size: usize,
}

impl Deduplicator {
    /// Create a new deduplicator
    pub fn new(max_size: usize) -> Self {
        Self {
            seen: Arc::new(RwLock::new(HashSet::new())),
            max_size,
        }
    }

    /// Check if a message is a duplicate
    pub async fn is_duplicate(&self, key: &str) -> bool {
        let seen = self.seen.read().await;
        seen.contains(key)
    }

    /// Mark a message as seen
    pub async fn mark_seen(&self, key: String) {
        let mut seen = self.seen.write().await;

        // If we're at max size, clear half
        if seen.len() >= self.max_size {
            let to_remove: Vec<_> = seen.iter().take(self.max_size / 2).cloned().collect();
            for k in to_remove {
                seen.remove(&k);
            }
        }

        seen.insert(key);
    }

    /// Clear all seen messages
    pub async fn clear(&self) {
        let mut seen = self.seen.write().await;
        seen.clear();
    }
}
