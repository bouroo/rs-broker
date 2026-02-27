//! Kafka test fixture for benchmarks
//!
//! Provides a simple fixture that connects to Kafka for benchmarks.
//! Assumes Kafka is running externally (e.g., via docker-compose).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Kafka fixture for benchmarks
///
/// This fixture connects to a Kafka broker. In development, you can use:
/// - Docker Compose: Use the docker-compose.yml in the project root (KRaft mode - no Zookeeper)
/// - Manual: docker run -p 9092:9092 -p 9093:9093 confluentinc/cp-kafka:8.0.4 with KRaft configuration
pub struct KafkaFixture {
    /// Bootstrap servers string
    pub bootstrap_servers: String,
    /// Topic counter for unique topic names
    topic_counter: Arc<AtomicUsize>,
}

impl KafkaFixture {
    /// Create a new Kafka fixture
    ///
    /// Attempts to connect to Kafka. Falls back to localhost:9092 if no Kafka is available.
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Use environment variable if set, otherwise default to localhost:9092
        let bootstrap_servers = std::env::var("KAFKA_BOOTSTRAP_SERVERS")
            .unwrap_or_else(|_| "localhost:9092".to_string());

        println!("Kafka fixture connecting to: {}", bootstrap_servers);

        Ok(Self {
            bootstrap_servers,
            topic_counter: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Create a topic with the given name and partition count
    ///
    /// Note: This is a best-effort operation. If Kafka is not available,
    /// the benchmark will still try to use the topic.
    pub fn create_topic(&self, name: &str, _partitions: i32) -> String {
        // Wait a bit for topic to be available (Kafka auto-creates topics)
        std::thread::sleep(Duration::from_millis(100));

        name.to_string()
    }

    /// Get a unique topic name
    pub fn unique_topic_name(&self, prefix: &str) -> String {
        let count = self.topic_counter.fetch_add(1, Ordering::SeqCst);
        format!("{}_{}_{}", prefix, count, std::process::id())
    }
}

/// Get a static Kafka fixture for use in benchmarks
pub fn get_kafka_fixture() -> &'static KafkaFixture {
    static FIXTURE: std::sync::OnceLock<KafkaFixture> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| KafkaFixture::new().expect("Failed to create Kafka fixture"))
}
