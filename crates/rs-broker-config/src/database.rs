//! Database configuration

use serde::Deserialize;

/// Database configuration
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Database URL
    #[serde(default = "default_database_url")]
    pub url: String,

    /// Maximum number of connections
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Minimum number of idle connections
    #[serde(default = "default_min_idle_connections")]
    pub min_idle_connections: Option<u32>,

    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,

    /// Max lifetime of connections in seconds
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime: u64,
}

fn default_database_url() -> String {
    // Default to localhost without credentials
    // Configure DATABASE_URL environment variable for production
    "postgres://localhost:5432/rsbroker".to_string()
}

fn default_max_connections() -> u32 {
    50 // Increased from 25 based on benchmark analysis for higher throughput
}

fn default_min_idle_connections() -> Option<u32> {
    Some(10) // Increased from 5 based on benchmark analysis for better performance
}

fn default_connection_timeout() -> u64 {
    60 // Increased from 30 seconds to handle high load scenarios
}

fn default_max_lifetime() -> u64 {
    1800 // 30 minutes - increased from 5 minutes based on benchmark analysis
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
            max_connections: default_max_connections(),
            min_idle_connections: default_min_idle_connections(),
            connection_timeout: default_connection_timeout(),
            max_lifetime: default_max_lifetime(),
        }
    }
}
