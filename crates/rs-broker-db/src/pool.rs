//! Database connection pool management

#[cfg(feature = "mysql")]
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
#[cfg(feature = "postgres")]
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;
use thiserror::Error;

use rs_broker_config::DatabaseConfig;

/// Error type for database pool operations
#[derive(Debug, Error)]
pub enum PoolError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Database pool type alias
#[cfg(all(feature = "postgres", not(feature = "mysql")))]
pub type DbPool = PgPool;

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
pub type DbPool = MySqlPool;

/// Create a database connection pool
#[cfg(all(feature = "postgres", not(feature = "mysql")))]
pub async fn create_pool(config: &DatabaseConfig) -> Result<DbPool, PoolError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(config.connection_timeout))
        .max_lifetime(Duration::from_secs(config.max_lifetime))
        .connect(&config.url)
        .await?;

    Ok(pool)
}

/// Create a database connection pool for MariaDB
#[cfg(all(feature = "mysql", not(feature = "postgres")))]
pub async fn create_pool(config: &DatabaseConfig) -> Result<DbPool, PoolError> {
    let pool = MySqlPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(config.connection_timeout))
        .max_lifetime(Duration::from_secs(config.max_lifetime))
        .connect(&config.url)
        .await?;

    Ok(pool)
}

#[cfg(not(any(feature = "postgres", feature = "mysql")))]
compile_error!("Either 'postgres' or 'mysql' feature must be enabled");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_config_default_produces_expected_values() {
        let cfg = DatabaseConfig::default();
        assert_eq!(cfg.url, "postgres://localhost:5432/rsbroker");
        assert_eq!(cfg.max_connections, 50);
        assert_eq!(cfg.min_idle_connections, Some(10));
        assert_eq!(cfg.connection_timeout, 60);
        assert_eq!(cfg.max_lifetime, 1800);
    }

    #[test]
    fn default_database_url_returns_expected_string() {
        assert_eq!(
            DatabaseConfig::default().url,
            "postgres://localhost:5432/rsbroker"
        );
    }

    #[test]
    fn pool_error_database_displays_cause() {
        let err = PoolError::Config("missing url".to_string());
        assert_eq!(err.to_string(), "Configuration error: missing url");
    }
}
