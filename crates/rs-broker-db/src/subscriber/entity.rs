//! Subscriber entity types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Subscriber entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscriber {
    /// Unique subscriber ID
    pub id: Uuid,

    /// Human-readable service name
    pub service_name: String,

    /// gRPC endpoint for delivery
    pub grpc_endpoint: String,

    /// Topic patterns to subscribe to
    pub topic_patterns: Vec<String>,

    /// Whether subscriber is active
    pub active: bool,

    /// Delivery configuration as JSON
    pub delivery_config: Option<serde_json::Value>,

    /// Timestamp when registered
    pub registered_at: DateTime<Utc>,

    /// Timestamp when last updated
    pub updated_at: DateTime<Utc>,
}

impl Subscriber {
    /// Create a new subscriber
    pub fn new(service_name: String, grpc_endpoint: String, topic_patterns: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            service_name,
            grpc_endpoint,
            topic_patterns,
            active: true,
            delivery_config: None,
            registered_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriber_new_creates_expected_defaults() {
        let patterns = vec!["orders.*".to_string(), "payments.*".to_string()];
        let sub = Subscriber::new(
            "order-service".to_string(),
            "http://order-service:50051".to_string(),
            patterns.clone(),
        );

        assert_eq!(sub.service_name, "order-service");
        assert_eq!(sub.grpc_endpoint, "http://order-service:50051");
        assert_eq!(sub.topic_patterns, patterns);
        assert!(sub.active, "newly created subscriber should be active");
        assert!(sub.delivery_config.is_none());
        assert_eq!(sub.registered_at, sub.updated_at);
        assert_ne!(sub.id, Uuid::nil());
    }

    #[test]
    fn subscriber_topic_patterns_are_stored() {
        let patterns = vec![
            "a.b.c".to_string(),
            "x.y".to_string(),
            "single".to_string(),
        ];
        let sub = Subscriber::new("svc".into(), "ep".into(), patterns.clone());
        assert_eq!(sub.topic_patterns.len(), 3);
        assert_eq!(sub.topic_patterns, patterns);
    }

    #[test]
    fn subscriber_active_defaults_to_true() {
        let sub = Subscriber::new("svc".into(), "ep".into(), vec![]);
        assert!(sub.active);
    }

    #[test]
    fn subscriber_new_generates_unique_ids() {
        let a = Subscriber::new("svc".into(), "ep".into(), vec![]);
        let b = Subscriber::new("svc".into(), "ep".into(), vec![]);
        assert_ne!(a.id, b.id);
    }
}
