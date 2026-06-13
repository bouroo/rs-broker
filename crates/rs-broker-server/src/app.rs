//! Application builder and runner

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{routing::get, Router};
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;

use rs_broker_config::Settings;
#[cfg(any(feature = "postgres", feature = "mysql"))]
use rs_broker_core::grpc_client::dispatcher::SubscriberDispatcher;
use rs_broker_core::inbox::{Dispatcher, InboxManager};
use rs_broker_core::outbox::OutboxPublisher;
use rs_broker_db::{create_pool, run_migrations};
use rs_broker_kafka::{KafkaConsumer, KafkaProducer};
use rs_broker_proto::rsbroker::{rs_broker_server::RsBrokerServer, DeliverEvent};

use crate::grpc;

/// Application state holding initialized components
pub struct App {
    settings: Settings,
    http_addr: SocketAddr,
    grpc_addr: SocketAddr,
    shutdown_tx: broadcast::Sender<()>,
    grpc_service: RsBrokerServer<grpc::service::RsBrokerService>,
    /// Outbox publisher kept on the `App` so its background task can be
    /// stopped from the shutdown path. `None` when database features are
    /// disabled (the publisher is not constructible in that build).
    publisher: Option<OutboxPublisher>,
    /// Kafka consumer for the inbox pipeline. `None` when no consumer topics
    /// are configured or when database features are disabled.
    consumer: Option<KafkaConsumer>,
    /// Database pool for creating additional components at runtime.
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    db_pool: rs_broker_db::DbPool,
    /// Broadcast sender for fan-out of DeliverEvents to streaming subscribers.
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    event_sender: tokio::sync::broadcast::Sender<DeliverEvent>,
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

        // Construct the Kafka producer once and share it with the publisher
        // and the gRPC health check. The producer itself does not establish
        // a connection at construction time; it only validates the config.
        let producer = Arc::new(KafkaProducer::new(&settings.kafka)?);
        tracing::info!(
            "Kafka producer constructed for {}",
            settings.kafka.bootstrap_servers
        );

        // Build the gRPC service. The Kafka producer validates its config at
        // construction time but does not establish a broker connection, so we
        // pass false for the connected flag; true health is verified at runtime
        // by the outbox publisher when it actually produces messages.
        #[cfg(any(feature = "postgres", feature = "mysql"))]
        let grpc_service_inner =
            grpc::service::RsBrokerService::with_kafka(db_pool.clone(), false);
        #[cfg(any(feature = "postgres", feature = "mysql"))]
        let event_sender = grpc_service_inner.event_sender();

        #[cfg(not(any(feature = "postgres", feature = "mysql")))]
        let grpc_service_inner = grpc::service::RsBrokerService::new(());

        let grpc_service = RsBrokerServer::new(grpc_service_inner);

        // Build the outbox publisher that drains pending messages to Kafka.
        let publisher = OutboxPublisher::with_producer(db_pool.clone(), producer)?;

        // Build the Kafka consumer if topics are configured.
        let consumer = if settings.kafka.consumer.topics.is_empty() {
            tracing::info!("No consumer topics configured — inbox pipeline disabled");
            None
        } else {
            match KafkaConsumer::new(&settings.kafka) {
                Ok(c) => {
                    let topic_refs: Vec<&str> = settings
                        .kafka
                        .consumer
                        .topics
                        .iter()
                        .map(|s| s.as_str())
                        .collect();
                    if let Err(e) = c.subscribe(&topic_refs) {
                        tracing::warn!(
                            "Failed to subscribe consumer to topics {:?}: {}. Consumer disabled.",
                            settings.kafka.consumer.topics,
                            e
                        );
                        None
                    } else {
                        tracing::info!(
                            "Kafka consumer subscribed to topics: {:?}",
                            settings.kafka.consumer.topics
                        );
                        Some(c)
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to create Kafka consumer: {}. Inbox pipeline disabled.",
                        e
                    );
                    None
                }
            }
        };

        let (shutdown_tx, _) = broadcast::channel(1);

        let http_addr = SocketAddr::from(([0, 0, 0, 0], settings.server.http_port));
        let grpc_addr = SocketAddr::from(([0, 0, 0, 0], settings.grpc.port));

        Ok(App {
            settings,
            http_addr,
            grpc_addr,
            shutdown_tx,
            grpc_service,
            publisher: Some(publisher),
            consumer,
            #[cfg(any(feature = "postgres", feature = "mysql"))]
            db_pool: db_pool.clone(),
            #[cfg(any(feature = "postgres", feature = "mysql"))]
            event_sender,
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
        let grpc_addr = self.grpc_addr;
        let grpc_service = self.grpc_service;
        let shutdown_tx = self.shutdown_tx;
        let mut publisher = self.publisher;
        #[cfg(any(feature = "postgres", feature = "mysql"))]
        let consumer = self.consumer;
        #[cfg(not(any(feature = "postgres", feature = "mysql")))]
        let _consumer = self.consumer;
        let settings = self.settings;

        #[cfg(any(feature = "postgres", feature = "mysql"))]
        let db_pool = self.db_pool;
        #[cfg(any(feature = "postgres", feature = "mysql"))]
        let event_sender = self.event_sender;

        // Start gRPC server in background
        tokio::spawn(async move {
            tracing::info!("Starting gRPC server on {}", grpc_addr);
            if let Err(err) = tonic::transport::Server::builder()
                .add_service(grpc_service)
                .serve(grpc_addr)
                .await
            {
                tracing::error!("gRPC server failed: {}", err);
            }
        });

        // Start the outbox publisher background worker using config values.
        if let Some(publisher) = publisher.as_mut() {
            let batch_size = settings.server.publisher_batch_size;
            let interval_ms = settings.server.publisher_interval_ms;
            publisher.start(batch_size, interval_ms);
            tracing::info!(
                "Outbox publisher started (batch_size={}, interval_ms={})",
                batch_size,
                interval_ms
            );
        }

        // Start the consumer pipeline if a consumer is available.
        #[cfg(any(feature = "postgres", feature = "mysql"))]
        if let Some(consumer) = consumer {
            let inbox_mgr = InboxManager::new(db_pool.clone());
            let dispatcher = {
                let sd = Arc::new(SubscriberDispatcher::new(db_pool.clone()));
                Dispatcher::with_subscriber_dispatcher(db_pool, sd)
            };
            spawn_consumer_loop(consumer, inbox_mgr, dispatcher, event_sender);
        }

        // Start HTTP server (blocks until shutdown)
        let listener = tokio::net::TcpListener::bind(self.http_addr).await?;
        tracing::info!("Starting HTTP server on {}", self.http_addr);

        let app = Self::router();

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(shutdown_tx))
            .await?;

        // Stop the publisher after the HTTP server returns from shutdown.
        if let Some(publisher) = publisher.take() {
            let mut publisher = publisher;
            publisher.stop().await;
            tracing::info!("Outbox publisher stopped");
        }

        tracing::info!("rs-broker server stopped (config: {:?})", settings);
        Ok(())
    }
}

/// Spawn the consumer loop that reads from Kafka and stores messages in the inbox,
/// dispatches to subscribers, and broadcasts events for streaming subscribers.
#[cfg(any(feature = "postgres", feature = "mysql"))]
fn spawn_consumer_loop(
    consumer: KafkaConsumer,
    inbox: InboxManager,
    dispatcher: Dispatcher,
    event_sender: tokio::sync::broadcast::Sender<DeliverEvent>,
) {
    let mut rx = consumer.stream();

    tokio::spawn(async move {
        while let Some(result) = rx.recv().await {
            match result {
                Ok(msg) => {
                    let payload: serde_json::Value = if msg.payload.is_empty() {
                        serde_json::json!({})
                    } else {
                        match serde_json::from_slice(&msg.payload) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to deserialize payload from topic {}: {}",
                                    msg.topic,
                                    e
                                );
                                continue;
                            }
                        }
                    };

                    let event_type = payload
                        .get("event_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    match inbox
                        .store_message(msg.topic.clone(), msg.partition, msg.offset, payload.clone())
                        .await
                    {
                        Ok(id) => {
                            tracing::trace!(
                                "Stored incoming message {} from topic {}",
                                id,
                                msg.topic
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Failed to store incoming message: {}", e);
                            continue;
                        }
                    }

                    // Dispatch to registered gRPC subscribers.
                    match dispatcher.dispatch(&msg.topic, &msg.payload).await {
                        Ok(count) => {
                            tracing::trace!(
                                "Dispatched message from topic {} to {} subscribers",
                                msg.topic,
                                count
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to dispatch message from topic {}: {}",
                                msg.topic,
                                e
                            );
                        }
                    }

                    // Broadcast the event for streaming subscribers.
                    let event = DeliverEvent {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        topic: msg.topic.clone(),
                        partition: msg.partition,
                        offset: msg.offset,
                        key: msg.key.clone().unwrap_or_default(),
                        payload: msg.payload.clone().into(),
                        headers: Vec::new(),
                        timestamp: chrono::Utc::now().timestamp(),
                        event_type,
                    };
                    let _ = event_sender.send(event);
                }
                Err(e) => {
                    tracing::warn!("Consumer stream error: {}", e);
                }
            }
        }
        tracing::info!("Consumer loop ended");
    });
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
