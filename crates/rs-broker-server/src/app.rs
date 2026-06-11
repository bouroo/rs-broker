//! Application builder and runner

use std::net::SocketAddr;

use axum::{routing::get, Router};
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;

use rs_broker_config::Settings;
use rs_broker_db::{create_pool, run_migrations};

/// Application state holding initialized components
pub struct App {
    #[allow(dead_code)]
    settings: Settings,
    http_addr: SocketAddr,
    #[allow(dead_code)]
    grpc_addr: SocketAddr,
    #[allow(dead_code)]
    shutdown_tx: broadcast::Sender<()>,
}

/// Builder for constructing the application
pub struct AppBuilder {
    settings: Option<Settings>,
}

impl AppBuilder {
    /// Create a new app builder
    pub fn new() -> Self {
        Self { settings: None }
    }

    /// Load settings from config file
    pub fn with_config(mut self) -> anyhow::Result<Self> {
        let settings = Settings::load_from_file("config")?;
        self.settings = Some(settings);
        Ok(self)
    }

    /// Use provided settings
    #[allow(dead_code)]
    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Build the application, initializing all components
    pub async fn build(self) -> anyhow::Result<App> {
        let settings = self
            .settings
            .ok_or_else(|| anyhow::anyhow!("Settings not configured"))?;

        tracing::info!("Configuration loaded: {:?}", settings);

        let db_pool = create_pool(&settings.database).await?;
        tracing::info!("Database connection pool created");

        run_migrations(&db_pool).await?;
        tracing::info!("Database migrations completed");

        // TODO: wire Kafka producer/consumer, gRPC service, background outbox publisher
        // using the db_pool and settings once those subsystems are ready.
        let _ = db_pool;

        let (shutdown_tx, _) = broadcast::channel(1);

        let http_addr = SocketAddr::from(([0, 0, 0, 0], settings.server.http_port));
        let grpc_addr = SocketAddr::from(([0, 0, 0, 0], settings.grpc.port));

        Ok(App {
            settings,
            http_addr,
            grpc_addr,
            shutdown_tx,
        })
    }
}

impl Default for AppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Build the HTTP router
    fn router() -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .layer(TraceLayer::new_for_http())
    }

    /// Run the application until shutdown
    pub async fn run(self) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(self.http_addr).await?;
        tracing::info!("Starting HTTP server on {}", self.http_addr);

        let app = Self::router();

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(self.shutdown_tx))
            .await?;

        tracing::info!("rs-broker server stopped");
        Ok(())
    }
}

async fn health_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn shutdown_signal(shutdown_tx: broadcast::Sender<()>) {
    use tokio::signal;

    let ctrl_c = async {
        if let Err(err) = signal::ctrl_c().await {
            tracing::error!("Failed to install Ctrl+C handler: {}", err);
        }
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        } else {
            tracing::error!("Failed to install signal handler");
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received");
    let _ = shutdown_tx.send(());
}
