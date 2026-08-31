//! End-to-end test: boots the full `App` (REST + gRPC + outbox publisher +
//! Kafka consumer loops) against live PostgreSQL + Kafka and drives a message
//! through the entire pipeline:
//!
//! REST publish → outbox row → Kafka topic → consumer loop → inbox →
//! broadcast → SSE delivery.
//!
//! Requires `TEST_DATABASE_URL` and `TEST_KAFKA_BOOTSTRAP_SERVERS` (see
//! compose.test.yml); the gRPC port must be free.

mod common;

use common::TestHarness;
use futures_util::StreamExt;
use serde_json::json;
use serial_test::serial;

use rs_broker_config::kafka::ConsumerConfig;
use rs_broker_config::{DatabaseConfig, KafkaConfig, Settings};

/// Full-stack pipeline test through the real server loops.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn app_pipeline_rest_publish_to_kafka_to_sse() {
    // Harness provides live Postgres + Kafka (external services when
    // TEST_* env vars are set) plus migration management.
    let harness = TestHarness::new().await;

    // Settings for the full App, pointed at the same live services as the
    // harness but on fixed ports distinct from the harness's inline servers.
    // The consumer subscribes to the topic the test publishes to so the
    // real consumer loop picks messages up and broadcasts DeliverEvents.
    let settings = Settings {
        server: rs_broker_config::ServerConfig {
            mode: rs_broker_config::ServerMode::Both,
            host: "127.0.0.1".to_string(),
            http_port: 18180,
            grpc_port: 15551,
            ..Default::default()
        },
        database: DatabaseConfig {
            url: harness.database_url.clone(),
            ..Default::default()
        },
        kafka: KafkaConfig {
            bootstrap_servers: harness.kafka_bootstrap.clone(),
            consumer: ConsumerConfig {
                group_id: "e2e-pipeline-consumer".to_string(),
                auto_offset_reset: "latest".to_string(),
                topics: vec!["orders".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let app = rs_broker_server::app::AppBuilder::new()
        .with_settings(settings)
        .build()
        .await
        .expect("app build failed");
    let app_handle = tokio::spawn(async move {
        if let Err(e) = app.run().await {
            eprintln!("app exited: {}", e);
        }
    });

    // Wait for the HTTP server to come up.
    let client = reqwest::Client::new();
    let mut healthy = false;
    for _ in 0..40 {
        if let Ok(resp) = client
            .get("http://127.0.0.1:18180/api/v1/health")
            .send()
            .await
        {
            if resp.status().is_success() {
                healthy = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(healthy, "app /api/v1/health never became ready");

    // Open the SSE stream on the app's REST interface.
    let sse_client = client.clone();
    let sse = tokio::spawn(async move {
        let resp = sse_client
            .get("http://127.0.0.1:18180/api/v1/events/stream?subscriber_id=e2e-app&patterns=orders.%23")
            .send()
            .await
            .expect("sse request failed");
        assert_eq!(resp.status(), 200);
        let mut stream = resp.bytes_stream();
        let mut buf = Vec::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
        loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "SSE deadline exceeded; got: {}",
                String::from_utf8_lossy(&buf)
            );
            let chunk =
                tokio::time::timeout(std::time::Duration::from_secs(20), stream.next()).await;
            match chunk {
                Ok(Some(Ok(bytes))) => buf.extend_from_slice(&bytes),
                Ok(Some(Err(e))) => panic!("sse stream error: {}", e),
                Ok(None) => panic!("sse ended early: {}", String::from_utf8_lossy(&buf)),
                Err(_) => continue,
            }
            let text = String::from_utf8_lossy(&buf);
            if text.contains("event: deliver") && text.contains("orders") {
                return text.to_string();
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Publish via REST on the app's interface. The consumer group joins
    // asynchronously after the App starts; with auto_offset_reset=latest a
    // message produced before assignment completes is skipped, so publish
    // one message per second until the SSE stream sees one of them.
    for attempt in 0..20 {
        let resp = client
            .post("http://127.0.0.1:18180/api/v1/publish")
            .json(&json!({
                "aggregate_type": "E2E",
                "aggregate_id": format!("pipeline-{attempt}"),
                "event_type": "PipelineTick",
                "payload": {"step": "rest-to-sse", "attempt": attempt},
                "topic": "orders"
            }))
            .send()
            .await
            .expect("rest publish failed");
        assert_eq!(resp.status(), 200, "rest publish should succeed");
        let body: serde_json::Value = resp.json().await.expect("json");
        let message_id = body["message_id"].as_str().expect("message_id").to_string();
        assert!(!message_id.is_empty());
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // Wait for a REST-published message to flow through the whole pipeline:
    // outbox → Kafka → consumer loop → broadcast → SSE. Kafka may also
    // deliver leftovers from previous runs (same payload signature), which
    // prove the same pipeline, so match on the payload rather than ids.
    let text = tokio::time::timeout(std::time::Duration::from_secs(30), sse)
        .await
        .expect("sse task timeout")
        .expect("sse task panicked");
    assert!(
        text.contains("\"step\":\"rest-to-sse\"") && text.contains("\"topic\":\"orders\""),
        "SSE should deliver a REST-published message; got: {}",
        text
    );

    app_handle.abort();
}
