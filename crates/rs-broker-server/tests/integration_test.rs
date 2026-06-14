//! Integration tests for the rs-broker gRPC API and database layer.
//!
//! Each test creates its own `TestHarness` (real PostgreSQL + Kafka
//! containers) and exercises the live gRPC and HTTP servers.

mod common;

use common::TestHarness;
use rs_broker_db::OutboxRepository;
use rs_broker_proto::rsbroker::{
    CancelMessageRequest, GetMessageStatusRequest, HealthRequest,
    HealthStatus as ProtoHealthStatus, ListDlqMessagesRequest, ListSubscribersRequest,
    MessageStatus as ProtoMessageStatus, PublishBatchRequest, PublishRequest,
    RegisterSubscriberRequest, SubscribeEventsRequest, UnregisterSubscriberRequest,
    UpdateSubscriberRequest,
};
use serial_test::serial;
use tokio_stream::StreamExt;
use uuid::Uuid;

#[serial]
#[tokio::test]
async fn test_health_endpoint() {
    let harness = TestHarness::new().await;
    let client = harness.http_client();

    let resp = client
        .get(format!("{}/health", harness.http_addr))
        .send()
        .await
        .expect("health request failed");

    assert!(
        resp.status().is_success(),
        "expected 2xx, got {}",
        resp.status()
    );

    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert_eq!(body["status"], "healthy");
}

#[serial]
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

#[serial]
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

#[serial]
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

#[serial]
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

#[serial]
#[tokio::test]
async fn test_publish_batch() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let messages = vec![
        PublishRequest {
            aggregate_type: "Order".to_string(),
            aggregate_id: "batch-1".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: br#"{"id":1}"#.to_vec(),
            topic: "orders".to_string(),
            ..Default::default()
        },
        PublishRequest {
            aggregate_type: "Order".to_string(),
            aggregate_id: "batch-2".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: br#"{"id":2}"#.to_vec(),
            topic: "orders".to_string(),
            ..Default::default()
        },
        PublishRequest {
            aggregate_type: "Order".to_string(),
            aggregate_id: "batch-3".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: br#"{"id":3}"#.to_vec(),
            topic: "orders".to_string(),
            ..Default::default()
        },
    ];

    let response = client
        .publish_batch(PublishBatchRequest {
            messages,
            ..Default::default()
        })
        .await
        .expect("publish_batch failed")
        .into_inner();

    assert_eq!(response.success_count, 3, "all 3 messages should succeed");
    assert_eq!(response.failure_count, 0, "no failures expected");
    assert_eq!(response.responses.len(), 3);

    for resp in &response.responses {
        assert!(
            !resp.message_id.is_empty(),
            "each response should have a message_id"
        );
        assert_eq!(
            ProtoMessageStatus::try_from(resp.status).unwrap(),
            ProtoMessageStatus::Pending,
            "each message should be Pending"
        );
    }
}

#[serial]
#[tokio::test]
async fn test_unregister_subscriber() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let reg_resp = client
        .register_subscriber(RegisterSubscriberRequest {
            service_name: "unreg-service".to_string(),
            grpc_endpoint: "http://localhost:8888".to_string(),
            topic_patterns: vec!["events.*".to_string()],
            ..Default::default()
        })
        .await
        .expect("register_subscriber failed")
        .into_inner();
    assert!(reg_resp.success);
    let subscriber_id = reg_resp.subscriber_id.clone();

    let unreg_resp = client
        .unregister_subscriber(UnregisterSubscriberRequest {
            subscriber_id: subscriber_id.clone(),
            ..Default::default()
        })
        .await
        .expect("unregister_subscriber failed")
        .into_inner();
    assert!(
        unreg_resp.success,
        "unregister should succeed: {}",
        unreg_resp.error
    );

    // After unregistration, listing active subscribers should not include it.
    let list_resp = client
        .list_subscribers(ListSubscribersRequest {
            active_only: true,
            ..Default::default()
        })
        .await
        .expect("list_subscribers failed")
        .into_inner();

    let found = list_resp
        .subscribers
        .iter()
        .find(|s| s.subscriber_id == subscriber_id);
    assert!(
        found.is_none(),
        "deactivated subscriber should not appear in active listing"
    );
}

#[serial]
#[tokio::test]
async fn test_update_subscriber() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let reg_resp = client
        .register_subscriber(RegisterSubscriberRequest {
            service_name: "update-service".to_string(),
            grpc_endpoint: "http://localhost:7777".to_string(),
            topic_patterns: vec!["old.*".to_string()],
            ..Default::default()
        })
        .await
        .expect("register_subscriber failed")
        .into_inner();
    assert!(reg_resp.success);
    let subscriber_id = reg_resp.subscriber_id.clone();

    let new_endpoint = "http://localhost:7778";
    let new_patterns = vec!["new.topic".to_string(), "another.*".to_string()];

    let update_resp = client
        .update_subscriber(UpdateSubscriberRequest {
            subscriber_id: subscriber_id.clone(),
            grpc_endpoint: new_endpoint.to_string(),
            topic_patterns: new_patterns.clone(),
            active: true,
            ..Default::default()
        })
        .await
        .expect("update_subscriber failed")
        .into_inner();
    assert!(
        update_resp.success,
        "update should succeed: {}",
        update_resp.error
    );

    // Verify update by listing.
    let list_resp = client
        .list_subscribers(ListSubscribersRequest {
            active_only: false,
            ..Default::default()
        })
        .await
        .expect("list_subscribers failed")
        .into_inner();

    let found = list_resp
        .subscribers
        .iter()
        .find(|s| s.subscriber_id == subscriber_id)
        .expect("updated subscriber should be in list");
    assert_eq!(found.grpc_endpoint, new_endpoint);
    assert_eq!(found.topic_patterns, new_patterns);
}

#[serial]
#[tokio::test]
async fn test_grpc_health_check() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let resp = client
        .get_health(HealthRequest {
            include_metrics: true,
        })
        .await
        .expect("get_health failed")
        .into_inner();

    assert_eq!(
        ProtoHealthStatus::try_from(resp.status).unwrap(),
        ProtoHealthStatus::Healthy,
        "broker should report healthy when DB is connected"
    );
    assert!(
        !resp.components.is_empty(),
        "components list should not be empty"
    );
    let metrics = resp
        .metrics
        .expect("metrics should be present when include_metrics=true");
    assert!(
        metrics.database_connected,
        "database_connected should be true"
    );
}

#[serial]
#[tokio::test]
async fn test_publish_invalid_uuid() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let result = client
        .get_message_status(GetMessageStatusRequest {
            message_id: "not-a-uuid".to_string(),
            ..Default::default()
        })
        .await;

    let status = result.expect_err("expected InvalidArgument error for invalid uuid");
    assert_eq!(
        status.code(),
        tonic::Code::InvalidArgument,
        "expected InvalidArgument, got {:?}",
        status.code()
    );
}

#[serial]
#[tokio::test]
async fn test_publish_invalid_payload() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let result = client
        .publish(PublishRequest {
            aggregate_type: "Order".to_string(),
            aggregate_id: "bad-payload".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: b"not json{{{".to_vec(),
            topic: "orders".to_string(),
            ..Default::default()
        })
        .await;

    let status = result.expect_err("expected InvalidArgument error for invalid payload JSON");
    assert_eq!(
        status.code(),
        tonic::Code::InvalidArgument,
        "expected InvalidArgument, got {:?}",
        status.code()
    );
}

#[serial]
#[tokio::test]
async fn test_cancel_nonexistent_message() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let nonexistent_id = Uuid::now_v7().to_string();

    let resp = client
        .cancel_message(CancelMessageRequest {
            message_id: nonexistent_id,
            ..Default::default()
        })
        .await
        .expect("cancel_message RPC should not error for nonexistent id")
        .into_inner();

    assert!(
        !resp.success,
        "cancel should report failure for nonexistent message"
    );
    assert!(
        !resp.error.is_empty(),
        "error message should be populated when message not found"
    );
}

#[serial]
#[tokio::test]
async fn test_idempotent_publish() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let message_id = Uuid::now_v7().to_string();

    let first = client
        .publish(PublishRequest {
            message_id: message_id.clone(),
            aggregate_type: "Order".to_string(),
            aggregate_id: "idem-1".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: br#"{"id":1}"#.to_vec(),
            topic: "orders".to_string(),
            ..Default::default()
        })
        .await
        .expect("first publish failed")
        .into_inner();
    assert_eq!(first.message_id, message_id);

    // Second publish with the same message_id should fail (unique constraint violation).
    let second = client
        .publish(PublishRequest {
            message_id: message_id.clone(),
            aggregate_type: "Order".to_string(),
            aggregate_id: "idem-2".to_string(),
            event_type: "OrderCreated".to_string(),
            payload: br#"{"id":2}"#.to_vec(),
            topic: "orders".to_string(),
            ..Default::default()
        })
        .await;

    let err = second
        .expect_err("second publish with same message_id should fail with unique-constraint error");
    // The current implementation does not check for duplicates before insert,
    // so the DB error is surfaced as Status::internal.
    assert!(
        err.code() == tonic::Code::Internal || err.code() == tonic::Code::AlreadyExists,
        "expected Internal or AlreadyExists, got {:?}",
        err.code()
    );
}

#[serial]
#[tokio::test]
async fn test_list_dlq_empty() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let resp = client
        .list_dlq_messages(ListDlqMessagesRequest::default())
        .await
        .expect("list_dlq_messages failed")
        .into_inner();

    assert!(
        resp.messages.is_empty(),
        "fresh DB should have no DLQ messages"
    );
    assert_eq!(resp.total_count, 0, "total_count should be 0");
}

#[serial]
#[tokio::test]
async fn test_subscribe_events_stream() {
    let harness = TestHarness::new().await;
    let mut client = harness.grpc_client().await;

    let stream_resp = client
        .subscribe_events(SubscribeEventsRequest {
            subscriber_id: "test-sub".to_string(),
            topic_patterns: vec!["orders.*".to_string()],
            ..Default::default()
        })
        .await
        .expect("subscribe_events failed");
    let mut stream = stream_resp.into_inner();

    // No events are being broadcast; verify the stream can be established
    // and simply times out waiting for the first event.
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next()).await;
    assert!(
        result.is_err(),
        "stream should time out without producing events"
    );
}
