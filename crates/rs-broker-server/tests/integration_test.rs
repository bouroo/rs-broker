//! Integration tests for the rs-broker gRPC API and database layer.
//!
//! Each test creates its own `TestHarness` (real PostgreSQL + Kafka
//! containers) and exercises the live gRPC and HTTP servers.

mod common;

use common::TestHarness;
use rs_broker_db::OutboxRepository;
use rs_broker_proto::rsbroker::{
    CancelMessageRequest, GetMessageStatusRequest, ListSubscribersRequest,
    MessageStatus as ProtoMessageStatus, PublishRequest, RegisterSubscriberRequest,
};
use uuid::Uuid;

#[tokio::test]
async fn test_health_endpoint() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .get(format!("{}/health", harness.http_addr))
        .send()
        .await
        .expect("health request failed");

    assert!(resp.status().is_success(), "expected 2xx, got {}", resp.status());

    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn test_publish_message() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let req = PublishRequest {
        aggregate_type: "Order".to_string(),
        aggregate_id: "order-123".to_string(),
        event_type: "OrderCreated".to_string(),
        payload: br#"{"amount":100}"#.to_vec(),
        topic: "orders".to_string(),
        ..Default::default()
    };

    let response = client
        .publish(req)
        .await
        .expect("publish RPC failed")
        .into_inner();

    assert!(!response.message_id.is_empty(), "message_id should be set");
    let message_id = response.message_id.clone();
    assert_eq!(
        ProtoMessageStatus::try_from(response.status).unwrap(),
        ProtoMessageStatus::Pending,
        "status should be Pending"
    );

    // Verify the row was actually written to the database.
    let repo = harness.outbox_repo();
    let id = Uuid::parse_str(&message_id).expect("invalid message_id uuid");
    let stored = repo
        .get_by_id(id)
        .await
        .expect("message not found in outbox");
    assert_eq!(stored.aggregate_type, "Order");
    assert_eq!(stored.aggregate_id, "order-123");
    assert_eq!(stored.event_type, "OrderCreated");
    assert_eq!(stored.topic, "orders");
}

#[tokio::test]
async fn test_get_message_status() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let publish_resp = client
        .publish(PublishRequest {
            aggregate_type: "Payment".to_string(),
            aggregate_id: "pay-1".to_string(),
            event_type: "PaymentReceived".to_string(),
            payload: br#"{"amount":50}"#.to_vec(),
            topic: "payments".to_string(),
            ..Default::default()
        })
        .await
        .expect("publish failed")
        .into_inner();

    let message_id = publish_resp.message_id.clone();
    assert!(!message_id.is_empty());

    let status_resp = client
        .get_message_status(GetMessageStatusRequest {
            message_id: message_id.clone(),
            ..Default::default()
        })
        .await
        .expect("get_message_status failed")
        .into_inner();

    assert_eq!(status_resp.message_id, message_id);
    assert_eq!(
        ProtoMessageStatus::try_from(status_resp.status).unwrap(),
        ProtoMessageStatus::Pending
    );
}

#[tokio::test]
async fn test_register_subscriber() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let reg_resp = client
        .register_subscriber(RegisterSubscriberRequest {
            service_name: "test-service".to_string(),
            grpc_endpoint: "http://localhost:9999".to_string(),
            topic_patterns: vec!["orders.*".to_string()],
            ..Default::default()
        })
        .await
        .expect("register_subscriber failed")
        .into_inner();

    assert!(reg_resp.success, "registration should succeed");
    assert!(
        !reg_resp.subscriber_id.is_empty(),
        "subscriber_id should be returned"
    );

    // Verify the subscriber shows up in the listing.
    let list_resp = client
        .list_subscribers(ListSubscribersRequest::default())
        .await
        .expect("list_subscribers failed")
        .into_inner();

    let found = list_resp
        .subscribers
        .iter()
        .find(|s| s.subscriber_id == reg_resp.subscriber_id);
    assert!(found.is_some(), "registered subscriber should be listed");
    let sub = found.unwrap();
    assert_eq!(sub.service_name, "test-service");
    assert_eq!(sub.grpc_endpoint, "http://localhost:9999");
    assert_eq!(sub.topic_patterns, vec!["orders.*".to_string()]);
}

#[tokio::test]
async fn test_cancel_message() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let publish_resp = client
        .publish(PublishRequest {
            aggregate_type: "Order".to_string(),
            aggregate_id: "order-cancel".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: br#"{"amount":1}"#.to_vec(),
            topic: "orders".to_string(),
            ..Default::default()
        })
        .await
        .expect("publish failed")
        .into_inner();

    let message_id = publish_resp.message_id.clone();

    let cancel_resp = client
        .cancel_message(CancelMessageRequest {
            message_id: message_id.clone(),
            ..Default::default()
        })
        .await
        .expect("cancel_message failed")
        .into_inner();

    assert!(cancel_resp.success, "cancellation should succeed");

    // After cancel, the message is deleted from the outbox.
    let repo = harness.outbox_repo();
    let id = Uuid::parse_str(&message_id).expect("invalid uuid");
    let result = repo.get_by_id(id).await;
    assert!(
        result.is_err(),
        "expected cancelled message to be removed from outbox"
    );
}
