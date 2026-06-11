//! Deduplication logic

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Deduplicator for inbox messages
pub struct Deduplicator {
    state: Arc<RwLock<DedupState>>,
    max_size: usize,
}

struct DedupState {
    seen: HashSet<String>,
    order: VecDeque<String>,
}

impl Deduplicator {
    /// Create a new deduplicator
    pub fn new(max_size: usize) -> Self {
        Self {
            state: Arc::new(RwLock::new(DedupState {
                seen: HashSet::new(),
                order: VecDeque::new(),
            })),
            max_size,
        }
    }

    /// Check if a message is a duplicate
    pub async fn is_duplicate(&self, key: &str) -> bool {
        let state = self.state.read().await;
        state.seen.contains(key)
    }

    /// Mark a message as seen
    pub async fn mark_seen(&self, key: String) {
        let mut state = self.state.write().await;

        // Evict oldest entries if at capacity
        while state.seen.len() >= self.max_size {
            if let Some(oldest) = state.order.pop_front() {
                state.seen.remove(&oldest);
            } else {
                break;
            }
        }

        if state.seen.insert(key.clone()) {
            state.order.push_back(key);
        }
    }

    /// Clear all seen messages
    pub async fn clear(&self) {
        let mut state = self.state.write().await;
        state.seen.clear();
        state.order.clear();
    }
}
