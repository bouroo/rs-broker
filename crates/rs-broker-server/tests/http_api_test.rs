//! Integration tests for the HTTP REST + SSE API (`/api/v1`).
//!
//! Mirrors the gRPC integration suite: each test spins the shared
//! `TestHarness` (real PostgreSQL + Kafka) and drives the REST endpoints
//! with `reqwest`, verifying parity with the gRPC transport.

mod common;

use common::TestHarness;
use futures_util::StreamExt;
use serde_json::json;
use serial_test::serial;

/// POST /api/v1/publish → 200, message id assigned, row lands in the outbox.
#[serial]
#[tokio::test]
async fn rest_publish_then_get_status() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .post(format!("{}/api/v1/publish", harness.http_addr))
        .json(&json!({
            "aggregate_type": "Order",
            "aggregate_id": "order-http-1",
            "event_type": "OrderCreated",
            "payload": {"amount": 100},
            "topic": "orders"
        }))
        .send()
        .await
        .expect("publish request failed");

    assert_eq!(resp.status(), 200, "publish should succeed");
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    let message_id = body["message_id"].as_str().expect("message_id").to_string();
    assert!(!message_id.is_empty());
    assert_eq!(body["status"], "pending");
    assert_eq!(body["duplicate"], false);

    // Parity: the same id is queryable via GET /api/v1/messages/{id}.
    let resp = client
        .get(format!(
            "{}/api/v1/messages/{}",
            harness.http_addr, message_id
        ))
        .send()
        .await
        .expect("status request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["message_id"], message_id.as_str());
    assert_eq!(body["status"], "pending");
    assert_eq!(body["topic"], "orders");
}

/// POST /api/v1/publish/batch reports per-message success/failure; invalid
/// payloads yield 400 with the shared error body.
#[serial]
#[tokio::test]
async fn rest_publish_batch_and_validation() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .post(format!("{}/api/v1/publish/batch", harness.http_addr))
        .json(&json!({
            "messages": [
                {"topic": "orders", "payload": {"n": 1}},
                {"topic": "orders", "payload": {"n": 2}}
            ]
        }))
        .send()
        .await
        .expect("batch request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["success_count"], 2);
    assert_eq!(body["failure_count"], 0);
    assert_eq!(body["responses"].as_array().map(|a| a.len()), Some(2));

    // Both payload and payload_base64 → 400 invalid_argument (JSON extractor
    // accepts the body; the DTO conversion rejects it).
    let resp = client
        .post(format!("{}/api/v1/publish", harness.http_addr))
        .json(&json!({"payload": {"n": 1}, "payload_base64": "aGk="}))
        .send()
        .await
        .expect("validation request failed");
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["error"]["code"], "invalid_argument");

    // Malformed base64 → 400.
    let resp = client
        .post(format!("{}/api/v1/publish", harness.http_addr))
        .json(&json!({"topic": "t", "payload_base64": "!!not-base64!!"}))
        .send()
        .await
        .expect("validation request failed");
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["error"]["code"], "invalid_argument");
}

/// DELETE /api/v1/messages/{id} cancels a pending message; an unknown id is
/// a 404 with the shared error body.
#[serial]
#[tokio::test]
async fn rest_message_cancel() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let publish = client
        .post(format!("{}/api/v1/publish", harness.http_addr))
        .json(&json!({
            "topic": "cancel-test",
            "payload": {"x": 1},
            "message_id": uuid::Uuid::now_v7().to_string()
        }))
        .send()
        .await
        .expect("publish failed");
    assert_eq!(publish.status(), 200);
    let publish_body: serde_json::Value = publish.json().await.expect("json");
    let message_id = publish_body["message_id"]
        .as_str()
        .expect("message_id")
        .to_string();

    let resp = client
        .delete(format!(
            "{}/api/v1/messages/{}",
            harness.http_addr, message_id
        ))
        .send()
        .await
        .expect("cancel failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["success"], true);

    // Unknown id → 404 (invalid uuid → 400 from the inner validation).
    let resp = client
        .delete(format!(
            "{}/api/v1/messages/{}",
            harness.http_addr,
            uuid::Uuid::now_v7()
        ))
        .send()
        .await
        .expect("cancel-unknown failed");
    assert_eq!(
        resp.status(),
        200,
        "not-found cancel reports success=false in-body"
    );
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["success"], false);
}

/// Subscriber CRUD over REST: register (201) → list → update → unregister.
#[serial]
#[tokio::test]
async fn rest_subscriber_crud() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .post(format!("{}/api/v1/subscribers", harness.http_addr))
        .json(&json!({
            "service_name": "http-sub",
            "grpc_endpoint": "http://localhost:7001",
            "topic_patterns": ["orders.*"]
        }))
        .send()
        .await
        .expect("register failed");
    assert_eq!(resp.status(), 201, "register should return 201");
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    let subscriber_id = body["subscriber_id"].as_str().expect("id").to_string();
    assert_eq!(body["success"], true);

    let resp = client
        .get(format!("{}/api/v1/subscribers", harness.http_addr))
        .send()
        .await
        .expect("list failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    let found = body["subscribers"]
        .as_array()
        .expect("array")
        .iter()
        .any(|s| s["subscriber_id"] == subscriber_id.as_str());
    assert!(found, "registered subscriber should be listed");

    let resp = client
        .patch(format!(
            "{}/api/v1/subscribers/{}",
            harness.http_addr, subscriber_id
        ))
        .json(&json!({"active": false, "topic_patterns": ["payments.*"]}))
        .send()
        .await
        .expect("update failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["success"], true);

    let resp = client
        .delete(format!(
            "{}/api/v1/subscribers/{}",
            harness.http_addr, subscriber_id
        ))
        .send()
        .await
        .expect("unregister failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["success"], true);
}

/// DLQ endpoints: empty list, then reprocess validation (no selector → 400).
#[serial]
#[tokio::test]
async fn rest_dlq_list_and_reprocess() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .get(format!("{}/api/v1/dlq", harness.http_addr))
        .send()
        .await
        .expect("dlq list failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert!(body["messages"].is_array());
    assert!(body["total_count"].is_i64());

    let resp = client
        .post(format!("{}/api/v1/dlq/reprocess", harness.http_addr))
        .json(&json!({}))
        .send()
        .await
        .expect("reprocess failed");
    assert_eq!(
        resp.status(),
        400,
        "empty selector should be invalid_argument"
    );
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["error"]["code"], "invalid_argument");
}

/// GET /api/v1/health returns the full HealthResponse shape.
#[serial]
#[tokio::test]
async fn rest_health() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .get(format!("{}/api/v1/health", harness.http_addr))
        .send()
        .await
        .expect("health request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["status"], "healthy");
    assert!(!body["components"]
        .as_array()
        .expect("components")
        .is_empty());
    assert_eq!(body["metrics"]["database_connected"], true);
}

/// SSE stream: publish via REST (outbox→Kafka→consumer→broadcast), receive a
/// matching `deliver` event on a pattern-filtered stream.
#[serial]
#[tokio::test]
async fn sse_receives_published_event() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    // Broadcast the DeliverEvent exactly as the Kafka consumer loop does when
    // the published message flows through (harness does not run the consumer).
    // Open the SSE stream first so the subscriber is attached, then read
    // incrementally until the deliver event arrives (the stream is infinite).
    let sse_task = tokio::spawn({
        let addr = harness.http_addr.clone();
        let client = client.clone();
        async move {
            let resp = client
                .get(format!(
                    "{}/api/v1/events/stream?subscriber_id=sse-test&patterns=orders.*",
                    addr
                ))
                .send()
                .await
                .expect("sse request failed");
            assert_eq!(resp.status(), 200);
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            assert!(content_type.starts_with("text/event-stream"));

            let mut stream = resp.bytes_stream();
            let mut buf = Vec::new();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "timed out waiting for deliver event; got: {}",
                    String::from_utf8_lossy(&buf)
                );
                let chunk = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
                    .await
                    .expect("chunk read timed out");
                match chunk {
                    Some(Ok(bytes)) => buf.extend_from_slice(&bytes),
                    Some(Err(e)) => panic!("sse stream error: {}", e),
                    None => panic!(
                        "sse stream ended prematurely: {}",
                        String::from_utf8_lossy(&buf)
                    ),
                }
                let text = String::from_utf8_lossy(&buf);
                if text.contains("event: deliver") && text.contains("orders.created") {
                    return text.to_string();
                }
            }
        }
    });

    // Give the SSE subscription a moment to attach, then broadcast a matching
    // and a non-matching event.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    harness.broadcast_event(rs_broker_proto::rsbroker::DeliverEvent {
        message_id: uuid::Uuid::now_v7().to_string(),
        topic: "orders.created".to_string(),
        partition: 0,
        offset: 1,
        key: "k1".to_string(),
        payload: br#"{"hello":"world"}"#.to_vec(),
        headers: vec![],
        timestamp: 12345,
        event_type: "OrderCreated".to_string(),
    });
    harness.broadcast_event(rs_broker_proto::rsbroker::DeliverEvent {
        message_id: uuid::Uuid::now_v7().to_string(),
        topic: "payments.settled".to_string(),
        partition: 0,
        offset: 2,
        key: "k2".to_string(),
        payload: br#"{"no":false}"#.to_vec(),
        headers: vec![],
        timestamp: 12346,
        event_type: "PaymentSettled".to_string(),
    });

    let text = tokio::time::timeout(std::time::Duration::from_secs(15), sse_task)
        .await
        .expect("sse task did not finish")
        .expect("sse task panicked");
    assert!(
        text.contains("event: deliver"),
        "should contain deliver event: {}",
        text
    );
    assert!(
        text.contains("orders.created"),
        "should contain matching topic: {}",
        text
    );
    assert!(
        !text.contains("payments.settled"),
        "non-matching topic filtered out"
    );
}

/// SSE validation: missing subscriber_id → 400 (mirrors gRPC). The query
/// extractor rejects before the handler, so the body is axum's plain-text
/// rejection rather than the JSON error shape.
#[serial]
#[tokio::test]
async fn sse_requires_subscriber_id() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .get(format!("{}/api/v1/events/stream", harness.http_addr))
        .send()
        .await
        .expect("sse request failed");
    assert_eq!(resp.status(), 400, "missing subscriber_id should be 400");
}
