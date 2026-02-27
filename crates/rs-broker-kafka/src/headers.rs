//! Kafka message headers utilities

use rdkafka::message::OwnedHeaders;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Kafka message headers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageHeaders {
    /// Header key-value pairs
    pub headers: HashMap<String, String>,
}

impl MessageHeaders {
    /// Create new empty headers
    pub fn new() -> Self {
        Self {
            headers: HashMap::new(),
        }
    }

    /// Add a header
    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.headers.insert(key.into(), value.into());
    }

    /// Get a header value
    pub fn get(&self, key: &str) -> Option<&String> {
        self.headers.get(key)
    }

    /// Convert to RDKafka headers
    pub fn to_rdkafka(&self) -> OwnedHeaders {
        let mut headers = rdkafka::message::OwnedHeaders::new();
        for (k, v) in &self.headers {
            headers = headers.insert(rdkafka::message::Header {
                key: k.as_str(),
                value: Some(v.as_bytes()),
            });
        }
        headers
    }
}
