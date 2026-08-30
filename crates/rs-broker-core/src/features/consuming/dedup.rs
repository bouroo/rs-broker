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
    ///
    /// Note: this is a read-only check. Callers that need to atomically
    /// check-and-mark should use [`Deduplicator::check_and_mark`] instead;
    /// combining `is_duplicate` with `mark_seen` is a check-then-act race.
    pub async fn is_duplicate(&self, key: &str) -> bool {
        let state = self.state.read().await;
        state.seen.contains(key)
    }

    /// Atomically check if `key` is a duplicate and mark it as seen if new.
    ///
    /// Returns `true` if the key was NEW (not previously seen) and has now been
    /// marked. Returns `false` if the key was already seen (duplicate).
    ///
    /// This is the atomic replacement for the racy `is_duplicate` + `mark_seen`
    /// two-step: it performs both operations under a single write lock so
    /// concurrent callers cannot both observe "new" for the same key.
    pub async fn check_and_mark(&self, key: String) -> bool {
        let mut state = self.state.write().await;
        if state.seen.contains(&key) {
            return false;
        }
        // Evict oldest entries if at capacity (same logic as mark_seen).
        while state.seen.len() >= self.max_size {
            if let Some(oldest) = state.order.pop_front() {
                state.seen.remove(&oldest);
            } else {
                break;
            }
        }
        state.seen.insert(key.clone());
        state.order.push_back(key);
        true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_and_mark_returns_true_for_new_key() {
        let dedup = Deduplicator::new(16);
        assert!(dedup.check_and_mark("a".to_string()).await);
    }

    #[tokio::test]
    async fn check_and_mark_returns_false_for_duplicate() {
        let dedup = Deduplicator::new(16);
        assert!(dedup.check_and_mark("a".to_string()).await);
        assert!(!dedup.check_and_mark("a".to_string()).await);
    }

    #[tokio::test]
    async fn check_and_mark_evicts_oldest_at_capacity() {
        let dedup = Deduplicator::new(2);
        assert!(dedup.check_and_mark("a".to_string()).await);
        assert!(dedup.check_and_mark("b".to_string()).await);
        assert!(dedup.check_and_mark("c".to_string()).await);
        assert!(dedup.check_and_mark("a".to_string()).await);
    }

    #[tokio::test]
    async fn check_and_mark_preserves_recent_keys() {
        let dedup = Deduplicator::new(2);
        assert!(dedup.check_and_mark("a".to_string()).await);
        assert!(dedup.check_and_mark("b".to_string()).await);
        assert!(dedup.check_and_mark("c".to_string()).await);
        assert!(!dedup.check_and_mark("b".to_string()).await);
    }
}
