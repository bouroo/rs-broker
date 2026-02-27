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
