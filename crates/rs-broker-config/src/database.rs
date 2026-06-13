//! Database configuration

use serde::Deserialize;

/// Database backend type.
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseType {
    #[default]
    Postgres,
    Mysql,
}

impl std::fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Postgres => f.write_str("postgres"),
            Self::Mysql => f.write_str("mysql"),
        }
    }
}

/// Database configuration
///
/// Supports two configuration styles:
///
/// 1. **Explicit URL** — set `url` directly (e.g. via
///    `RS_BROKER_DATABASE__URL`).
/// 2. **Component fields** — set `host`, `port`, `username`, `password`, and
///    `database` individually (e.g. via `RS_BROKER_DATABASE__HOST`).
///
/// When `url` is non-empty it takes precedence; otherwise the URL is built
/// from the component fields via [`DatabaseConfig::connection_url`].
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Fully-qualified database URL.
    ///
    /// When non-empty this value is used verbatim. When empty (the default)
    /// the connection URL is derived from the component fields below.
    #[serde(default)]
    pub url: String,

    /// Database backend type.
    #[serde(default)]
    pub r#type: DatabaseType,

    /// Database host.
    #[serde(default = "default_host")]
    pub host: String,

    /// Database port.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Database username.
    #[serde(default = "default_username")]
    pub username: String,

    /// Database password.
    #[serde(default)]
    pub password: String,

    /// Database name.
    #[serde(default = "default_database")]
    pub database: String,

    /// Maximum number of connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Minimum number of idle connections.
    #[serde(default = "default_min_idle_connections")]
    pub min_idle_connections: Option<u32>,

    /// Minimum number of connections maintained by the pool (env compat).
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,

    /// Connection timeout in seconds.
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,

    /// Max lifetime of connections in seconds.
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime: u64,

    /// Run SQL migrations on startup.
    #[serde(default = "default_auto_migrate")]
    pub auto_migrate: bool,
}

fn default_host() -> String {
    "localhost".to_string()
}

fn default_port() -> u16 {
    5432
}

fn default_username() -> String {
    "rsbroker".to_string()
}

fn default_database() -> String {
    "rsbroker".to_string()
}

fn default_max_connections() -> u32 {
    50 // Increased from 25 based on benchmark analysis for higher throughput
}

fn default_min_idle_connections() -> Option<u32> {
    Some(10) // Increased from 5 based on benchmark analysis for better performance
}

fn default_min_connections() -> u32 {
    5
}

fn default_connection_timeout() -> u64 {
    60 // Increased from 30 seconds to handle high load scenarios
}

fn default_max_lifetime() -> u64 {
    1800 // 30 minutes - increased from 5 minutes based on benchmark analysis
}

fn default_auto_migrate() -> bool {
    true
}

impl DatabaseConfig {
    /// Resolve the connection URL.
    ///
    /// Returns the explicit `url` if it was provided, otherwise builds a URL
    /// from the component fields (`type`, `host`, `port`, `username`,
    /// `password`, `database`).
    pub fn connection_url(&self) -> String {
        if !self.url.is_empty() {
            return self.url.clone();
        }

        let scheme = self.r#type.to_string();
        let password_part = if self.password.is_empty() {
            String::new()
        } else {
            format!(":{}", self.password)
        };
        format!(
            "{scheme}://{username}{password}@{host}:{port}/{database}",
            username = self.username,
            password = password_part,
            host = self.host,
            port = self.port,
            database = self.database,
        )
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            r#type: DatabaseType::default(),
            host: default_host(),
            port: default_port(),
            username: default_username(),
            password: String::new(),
            database: default_database(),
            max_connections: default_max_connections(),
            min_idle_connections: default_min_idle_connections(),
            min_connections: default_min_connections(),
            connection_timeout: default_connection_timeout(),
            max_lifetime: default_max_lifetime(),
            auto_migrate: default_auto_migrate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_config_default_builds_url_from_components() {
        let cfg = DatabaseConfig::default();
        assert!(cfg.url.is_empty(), "default url should be empty");
        // connection_url builds from components.
        let url = cfg.connection_url();
        assert_eq!(
            url, "postgres://rsbroker@localhost:5432/rsbroker",
            "no password → no colon before @",
        );
    }

    #[test]
    fn database_config_explicit_url_takes_precedence() {
        let cfg = DatabaseConfig {
            url: "postgres://u:p@db:5433/app".to_string(),
            ..Default::default()
        };
        assert_eq!(cfg.connection_url(), "postgres://u:p@db:5433/app");
    }

    #[test]
    fn database_config_url_includes_password_when_set() {
        let cfg = DatabaseConfig {
            host: "pg".into(),
            password: "secret".into(),
            ..Default::default()
        };
        let url = cfg.connection_url();
        assert_eq!(url, "postgres://rsbroker:secret@pg:5432/rsbroker");
    }

    #[test]
    fn database_config_mysql_scheme() {
        let cfg = DatabaseConfig {
            r#type: DatabaseType::Mysql,
            ..Default::default()
        };
        assert!(cfg.connection_url().starts_with("mysql://"));
    }

    #[test]
    fn database_type_display() {
        assert_eq!(DatabaseType::Postgres.to_string(), "postgres");
        assert_eq!(DatabaseType::Mysql.to_string(), "mysql");
    }
}
