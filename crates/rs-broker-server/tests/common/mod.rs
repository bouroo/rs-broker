//! Shared test harness for rs-broker integration tests.
//!
//! Spins up real PostgreSQL and Kafka containers via `testcontainers`,
//! applies migrations, and starts the gRPC + HTTP servers on ephemeral
//! ports. The harness keeps the containers alive for the lifetime of
//! the harness and aborts the server tasks on drop.

use std::net::SocketAddr;

use testcontainers::{
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

use rs_broker_config::{DatabaseConfig, KafkaConfig};
use rs_broker_db::{create_pool, run_migrations, DbPool, SqlxOutboxRepository};
use rs_broker_proto::rsbroker::rs_broker_client::RsBrokerClient;
use rs_broker_proto::rsbroker::rs_broker_server::RsBrokerServer;
use rs_broker_server::grpc::service::RsBrokerService;

/// Bundles the running containers so they stay alive for the test's duration.
///
/// When the test harness is running inside a Podman container and the
/// `TEST_DATABASE_URL` / `TEST_KAFKA_BOOTSTRAP_SERVERS` environment variables
/// are set, the corresponding container is `None` and the test process
/// connects to externally-managed services.
struct Containers {
    _postgres: Option<ContainerAsync<GenericImage>>,
    _kafka: Option<ContainerAsync<GenericImage>>,
}

/// Test harness providing live database, gRPC, and HTTP endpoints.
pub struct TestHarness {
    pub db_pool: DbPool,
    pub grpc_addr: String,
    pub http_addr: String,
    _containers: Containers,
    _grpc_task: tokio::task::JoinHandle<()>,
    _http_task: tokio::task::JoinHandle<()>,
}

impl TestHarness {
    /// Spin up containers, run migrations, and start the gRPC and HTTP
    /// servers in the background. Returns once the servers are bound.
    ///
    /// Honors two environment variables to skip container startup and connect
    /// to externally-managed services instead — used when running inside a
    /// Podman test container where Docker socket access is unavailable:
    ///   * `TEST_DATABASE_URL` — PostgreSQL connection string
    ///   * `TEST_KAFKA_BOOTSTRAP_SERVERS` — Kafka bootstrap servers
    pub async fn new() -> Self {
        let external_db_url = std::env::var("TEST_DATABASE_URL").ok();
        let external_kafka = std::env::var("TEST_KAFKA_BOOTSTRAP_SERVERS").ok();

        // ----- PostgreSQL ----------------------------------------------------
        let postgres = if external_db_url.is_none() {
            Some(
                GenericImage::new("postgres", "18-alpine")
                    .with_wait_for(WaitFor::message_on_stdout(
                        "database system is ready to accept connections",
                    ))
                    .with_exposed_port(ContainerPort::Tcp(5432))
                    .with_startup_timeout(std::time::Duration::from_secs(120))
                    .with_env_var("POSTGRES_USER", "rsbroker")
                    .with_env_var("POSTGRES_PASSWORD", "rsbroker_dev_password")
                    .with_env_var("POSTGRES_DB", "rsbroker")
                    .start()
                    .await
                    .expect("failed to start postgres container"),
            )
        } else {
            None
        };

        let database_url = match &external_db_url {
            Some(url) => url.clone(),
            None => {
                let pg = postgres
                    .as_ref()
                    .expect("postgres container must exist when TEST_DATABASE_URL is unset");
                let pg_port = pg
                    .get_host_port_ipv4(5432)
                    .await
                    .expect("failed to get postgres port");
                let pg_host = pg
                    .get_host()
                    .await
                    .expect("failed to get postgres host")
                    .to_string();
                format!(
                    "postgres://rsbroker:rsbroker_dev_password@{}:{}/rsbroker",
                    pg_host, pg_port
                )
            }
        };

        let db_config = DatabaseConfig {
            url: database_url.clone(),
            ..Default::default()
        };

        // Postgres prints the ready message before it actually accepts
        // authenticated connections from new pools, so retry a few times.
        let db_pool = {
            let mut last_err = None;
            let mut pool = None;
            for _ in 0..30 {
                match create_pool(&db_config).await {
                    Ok(p) => {
                        pool = Some(p);
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                }
            }
            pool.unwrap_or_else(|| panic!("failed to create db pool after retries: {:?}", last_err))
        };
        run_migrations(&db_pool)
            .await
            .expect("failed to run migrations");

        // ----- Kafka (KRaft single-node) -------------------------------------
        let kafka = if external_kafka.is_none() {
            Some(
                GenericImage::new("confluentinc/cp-kafka", "8.0.4")
                    .with_wait_for(WaitFor::message_on_stdout("Kafka Server started"))
                    .with_exposed_port(ContainerPort::Tcp(9092))
                    .with_startup_timeout(std::time::Duration::from_secs(180))
                    .with_env_var("KAFKA_NODE_ID", "1")
                    .with_env_var("KAFKA_PROCESS_ROLES", "broker,controller")
                    .with_env_var(
                        "KAFKA_LISTENERS",
                        "PLAINTEXT://0.0.0.0:9092,CONTROLLER://0.0.0.0:9093",
                    )
                    .with_env_var("KAFKA_ADVERTISED_LISTENERS", "PLAINTEXT://127.0.0.1:9092")
                    .with_env_var("KAFKA_CONTROLLER_LISTENER_NAMES", "CONTROLLER")
                    .with_env_var(
                        "KAFKA_LISTENER_SECURITY_PROTOCOL_MAP",
                        "CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT",
                    )
                    .with_env_var("KAFKA_CONTROLLER_QUORUM_VOTERS", "1@127.0.0.1:9093")
                    .with_env_var("KAFKA_INTER_BROKER_LISTENER_NAME", "PLAINTEXT")
                    .with_env_var("KAFKA_OFFSETS_TOPIC_REPLICATION_FACTOR", "1")
                    .with_env_var("KAFKA_TRANSACTION_STATE_LOG_MIN_ISR", "1")
                    .with_env_var("KAFKA_TRANSACTION_STATE_LOG_REPLICATION_FACTOR", "1")
                    .with_env_var("KAFKA_AUTO_CREATE_TOPICS_ENABLE", "true")
                    .with_env_var("KAFKA_NUM_PARTITIONS", "1")
                    .with_env_var("CLUSTER_ID", "MkU3OEVBNTcwNTJENDM2Qk")
                    .start()
                    .await
                    .expect("failed to start kafka container"),
            )
        } else {
            None
        };

        let _kafka_config = KafkaConfig {
            bootstrap_servers: match &external_kafka {
                Some(servers) => servers.clone(),
                None => {
                    let k = kafka.as_ref().expect(
                        "kafka container must exist when TEST_KAFKA_BOOTSTRAP_SERVERS is unset",
                    );
                    let kafka_port = k
                        .get_host_port_ipv4(9092)
                        .await
                        .expect("failed to get kafka port");
                    let kafka_host = k
                        .get_host()
                        .await
                        .expect("failed to get kafka host")
                        .to_string();
                    format!("{}:{}", kafka_host, kafka_port)
                }
            },
            ..Default::default()
        };

        // ----- gRPC server ----------------------------------------------------
        let grpc_service = RsBrokerService::with_kafka(db_pool.clone(), false);
        let grpc_server = RsBrokerServer::new(grpc_service);

        let grpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind grpc listener");
        let grpc_addr: SocketAddr = grpc_listener
            .local_addr()
            .expect("failed to read grpc local addr");
        let grpc_addr_incoming = tokio_stream::wrappers::TcpListenerStream::new(grpc_listener);

        let grpc_task = tokio::spawn(async move {
            if let Err(err) = tonic::transport::Server::builder()
                .add_service(grpc_server)
                .serve_with_incoming(grpc_addr_incoming)
                .await
            {
                eprintln!("gRPC server error: {}", err);
            }
        });

        // ----- HTTP server (health endpoint) ---------------------------------
        use axum::{routing::get, Json, Router};
        use serde_json::json;

        async fn health_handler() -> Json<serde_json::Value> {
            Json(json!({
                "status": "healthy",
                "version": env!("CARGO_PKG_VERSION")
            }))
        }

        let http_app = Router::new().route("/health", get(health_handler));
        let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind http listener");
        let http_addr: SocketAddr = http_listener
            .local_addr()
            .expect("failed to read http local addr");

        let http_task = tokio::spawn(async move {
            if let Err(err) = axum::serve(http_listener, http_app).await {
                eprintln!("HTTP server error: {}", err);
            }
        });

        Self {
            db_pool,
            grpc_addr: format!("http://{}", grpc_addr),
            http_addr: format!("http://{}", http_addr),
            _containers: Containers {
                _postgres: postgres,
                _kafka: kafka,
            },
            _grpc_task: grpc_task,
            _http_task: http_task,
        }
    }

    /// Build a gRPC client connected to the harness's gRPC server.
    pub async fn grpc_client(&self) -> RsBrokerClient<tonic::transport::Channel> {
        RsBrokerClient::connect(self.grpc_addr.clone())
            .await
            .expect("failed to connect gRPC client")
    }

    /// Build a reqwest HTTP client.
    pub fn http_client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }

    /// Build a repository handle for direct database assertions.
    pub fn outbox_repo(&self) -> SqlxOutboxRepository {
        SqlxOutboxRepository::new(self.db_pool.clone())
    }
}
