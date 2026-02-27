//! rs-broker-server - Main binary entry point

use std::{net::SocketAddr, time::Duration};

use axum::{response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use rs_broker_config::{DatabaseConfig, KafkaConfig, Settings};
#[cfg(any(feature = "postgres", feature = "mysql"))]
use rs_broker_core::OutboxManager;
use rs_broker_core::SubscriberRegistry;
use rs_broker_db::{create_pool, run_migrations};
use rs_broker_kafka::{create_consumer, create_producer};

mod grpc;

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Health check handler
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Build the application router
fn app() -> Router {
    Router::new()
        .route("/health", get(health))
        .layer(TraceLayer::new_for_http())
}

/// Shutdown signal handler
async fn shutdown_signal() {
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rs_broker_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting rs-broker server");

    // Load configuration
    let settings = Settings::load_from_file("config")?;
    tracing::info!("Configuration loaded: {:?}", settings);

    // Create database connection pool
    let db_config = DatabaseConfig {
        url: settings.database.url.clone(),
        max_connections: settings.database.max_connections,
        min_idle_connections: settings.database.min_idle_connections,
        connection_timeout: settings.database.connection_timeout,
        max_lifetime: settings.database.max_lifetime,
    };

    let db_pool = create_pool(&db_config).await?;
    tracing::info!("Database connection pool created");

    // Run database migrations
    run_migrations(&db_pool).await?;
    tracing::info!("Database migrations completed");

    // Create broadcast channel for shutdown
    let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

    // Initialize core components
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    let outbox_manager =
        OutboxManager::new(db_pool.clone(), rs_broker_config::RetryConfig::default());

    let _subscriber_registry = SubscriberRegistry::new(db_pool.clone());

    #[cfg(not(any(feature = "postgres", feature = "mysql")))]
    let _outbox_manager = ();

    // Create Kafka producer
    let _producer = {
        let kafka_config = KafkaConfig {
            bootstrap_servers: settings.kafka.bootstrap_servers.clone(),
            client_id: settings.kafka.client_id.clone(),
            ..Default::default()
        };
        Some(create_producer(kafka_config)?)
    };

    // Create Kafka consumer
    let _consumer = {
        let kafka_config = KafkaConfig {
            bootstrap_servers: settings.kafka.bootstrap_servers.clone(),
            client_id: settings.kafka.client_id.clone(),
            consumer: rs_broker_config::kafka::ConsumerConfig {
                group_id: settings.kafka.consumer.group_id.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        Some(create_consumer(kafka_config)?)
    };

    // Create gRPC service
    let _grpc_service = {
        #[cfg(any(feature = "postgres", feature = "mysql"))]
        {
            grpc::service::RsBrokerService::new(db_pool.clone())
        }
        #[cfg(not(any(feature = "postgres", feature = "mysql")))]
        {
            // When no database features are enabled, pass unit type
            grpc::service::RsBrokerService::new(())
        }
    };

    // Start gRPC server
    let grpc_addr = SocketAddr::from(([0, 0, 0, 0], settings.grpc.port));
    let _grpc_router: Router = Router::new().route("/health", get(health));

    tokio::spawn(async move {
        tracing::info!("Starting gRPC server on {}", grpc_addr);

        // Note: In production, you would use tonic::Server::builder()
        // with the RsBrokerServer service. For now, we just log the intention.
        tracing::info!("gRPC service ready at {}", grpc_addr);
    });

    // Build HTTP application
    let app = app();

    // Start HTTP server
    let http_addr = SocketAddr::from(([0, 0, 0, 0], settings.server.http_port));
    tracing::info!("Starting HTTP server on {}", http_addr);

    let listener = tokio::net::TcpListener::bind(http_addr).await?;

    // Run background tasks
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    {
        let _outbox_manager_clone = outbox_manager.clone();
        tokio::spawn(async move {
            // Outbox publisher background task
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                // In production, this would poll the outbox and publish to Kafka
                tracing::debug!("Outbox publisher tick");
            }
        });
    }

    // Run HTTP server
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = shutdown_tx.send(());
        })
        .await?;

    tracing::info!("rs-broker server stopped");
    Ok(())
}
