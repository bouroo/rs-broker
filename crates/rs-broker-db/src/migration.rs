//! Database migration utilities

/// Error type for migration operations
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(String),
}

/// Run database migrations
/// Note: This expects migrations to be at workspace root ./migrations
#[cfg(all(feature = "postgres", not(feature = "mysql")))]
pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<(), MigrationError> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|e| MigrationError::Migration(e.to_string()))?;

    Ok(())
}

#[cfg(all(feature = "mysql", not(feature = "postgres")))]
pub async fn run_migrations(pool: &sqlx::MySqlPool) -> Result<(), MigrationError> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|e| MigrationError::Migration(e.to_string()))?;

    Ok(())
}

#[cfg(not(any(feature = "postgres", feature = "mysql")))]
compile_error!("Either 'postgres' or 'mysql' feature must be enabled");
