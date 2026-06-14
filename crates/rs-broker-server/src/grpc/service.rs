//! gRPC service implementation for RsBroker

use chrono::Utc;
use futures_util::StreamExt;
use tokio::sync::broadcast;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use rs_broker_core::dlq::{DlqHandler, DlqSelector};
use rs_broker_proto::rsbroker::{
    rs_broker_server::RsBroker, BrokerMetrics, CancelMessageRequest, CancelMessageResponse,
    ComponentHealth, DeliverEvent, DlqMessageInfo, GetMessageStatusRequest,
    GetMessageStatusResponse, HealthRequest, HealthResponse, HealthStatus, ListDlqMessagesRequest,
    ListDlqMessagesResponse, ListSubscribersRequest, ListSubscribersResponse,
    MessageStatus as ProtoMessageStatus, PublishBatchRequest, PublishBatchResponse, PublishRequest,
    PublishResponse, RegisterSubscriberRequest, RegisterSubscriberResponse, ReprocessDlqRequest,
    ReprocessDlqResponse, SubscribeEventsRequest, SubscriberInfo, UnregisterSubscriberRequest,
    UnregisterSubscriberResponse, UpdateSubscriberRequest, UpdateSubscriberResponse,
};

use rs_broker_db::{
    OutboxMessage, OutboxRepository, SqlxOutboxRepository, SqlxSubscriberRepository, Subscriber,
    SubscriberRepository,
};

/// Process a single publish request against the given outbox repository.
///
/// This is a free function so it can be called from both the unary `publish`
/// method and the bidirectional `stream_publish` without needing to clone the
/// whole service.
async fn process_publish(
    outbox_repo: &SqlxOutboxRepository,
    req: PublishRequest,
) -> Result<PublishResponse, Status> {
    let message_id = if req.message_id.is_empty() {
        Uuid::now_v7().to_string()
    } else {
        req.message_id
    };

    let payload: serde_json::Value = if req.payload.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&req.payload)
            .map_err(|e| Status::invalid_argument(format!("Invalid payload JSON: {}", e)))?
    };

    let headers = if req.headers.is_empty() {
        None
    } else {
        let headers_map: std::collections::HashMap<String, String> = req
            .headers
            .iter()
            .map(|h| (h.key.clone(), h.value.clone()))
            .collect();
        serde_json::to_value(headers_map).ok()
    };

    let mut message = OutboxMessage::new(
        req.aggregate_type,
        req.aggregate_id,
        req.event_type,
        payload,
        req.topic,
    );

    message.id = Uuid::parse_str(&message_id)
        .map_err(|e| Status::invalid_argument(format!("Invalid message_id: {}", e)))?;
    message.headers = headers;
    message.partition_key = if req.partition_key.is_empty() {
        None
    } else {
        Some(req.partition_key)
    };

    outbox_repo
        .create(&message)
        .await
        .map_err(|e| Status::internal(format!("Failed to create message: {}", e)))?;

    Ok(PublishResponse {
        message_id,
        status: ProtoMessageStatus::Pending.into(),
        duplicate: false,
        accepted_at: Utc::now().timestamp(),
        error: String::new(),
    })
}

/// RsBroker gRPC service implementation
pub struct RsBrokerService {
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    db_pool: rs_broker_db::DbPool,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    outbox_repo: SqlxOutboxRepository,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    subscriber_repo: SqlxSubscriberRepository,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    dlq_handler: DlqHandler,
    /// Whether the Kafka producer was successfully constructed.
    ///
    /// The service itself does not own the producer, so the flag is set by
    /// the caller once producer construction completes. A failed construction
    /// must report `false` so health checks surface the misconfiguration.
    #[allow(dead_code)]
    kafka_connected: bool,
    /// Broadcast channel for fan-out of `DeliverEvent`s to streaming subscribers.
    event_sender: broadcast::Sender<DeliverEvent>,
    #[cfg(all(not(feature = "postgres"), not(feature = "mysql")))]
    _phantom: std::marker::PhantomData<()>,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl RsBrokerService {
    /// Create a new RsBroker service without a Kafka producer.
    #[allow(dead_code)]
    pub fn new(db_pool: rs_broker_db::DbPool) -> Self {
        Self::with_kafka(db_pool, false)
    }

    /// Create a new RsBroker service with the Kafka health flag.
    pub fn with_kafka(db_pool: rs_broker_db::DbPool, kafka_connected: bool) -> Self {
        let outbox_repo = SqlxOutboxRepository::new(db_pool.clone());
        let subscriber_repo = SqlxSubscriberRepository::new(db_pool.clone());
        let dlq_handler = DlqHandler::new(db_pool.clone());
        let (event_sender, _) = broadcast::channel(1024);
        Self {
            db_pool,
            outbox_repo,
            subscriber_repo,
            dlq_handler,
            kafka_connected,
            event_sender,
        }
    }

    /// Returns a clone of the broadcast sender.
    ///
    /// External components (e.g. publish path, Kafka consumer) can use this to
    /// broadcast `DeliverEvent`s to all active streaming subscribers.
    pub fn event_sender(&self) -> broadcast::Sender<DeliverEvent> {
        self.event_sender.clone()
    }

    /// Process a single publish request, returning the response or a Status error.
    async fn publish_single(&self, req: PublishRequest) -> Result<PublishResponse, Status> {
        process_publish(&self.outbox_repo, req).await
    }
}

#[cfg(not(any(feature = "postgres", feature = "mysql")))]
impl RsBrokerService {
    /// Create a new RsBroker service (stub for when no database features are enabled)
    pub fn new(_db_pool: ()) -> Self {
        let (event_sender, _) = broadcast::channel(1024);
        Self {
            kafka_connected: false,
            event_sender,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[tonic::async_trait]
#[cfg(any(feature = "postgres", feature = "mysql"))]
impl RsBroker for RsBrokerService {
    type SubscribeEventsStream = std::pin::Pin<
        Box<
            dyn tonic::codegen::tokio_stream::Stream<Item = Result<DeliverEvent, tonic::Status>>
                + Send,
        >,
    >;
    type StreamPublishStream = std::pin::Pin<
        Box<
            dyn tonic::codegen::tokio_stream::Stream<Item = Result<PublishResponse, tonic::Status>>
                + Send,
        >,
    >;

    /// Publish a single message to the outbox
    async fn publish(
        &self,
        request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        let req = request.into_inner();
        let response = self.publish_single(req).await?;
        Ok(Response::new(response))
    }

    /// Publish multiple messages in a batch
    async fn publish_batch(
        &self,
        request: Request<PublishBatchRequest>,
    ) -> Result<Response<PublishBatchResponse>, Status> {
        let req = request.into_inner();
        let mut responses = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;

        for msg_req in req.messages {
            let result = self.publish(Request::new(msg_req)).await;
            match result {
                Ok(resp) => {
                    success_count += 1;
                    responses.push(resp.into_inner());
                }
                Err(e) => {
                    failure_count += 1;
                    responses.push(PublishResponse {
                        message_id: String::new(),
                        status: ProtoMessageStatus::Pending.into(),
                        duplicate: false,
                        accepted_at: 0,
                        error: e.message().to_string(),
                    });
                }
            }
        }

        Ok(Response::new(PublishBatchResponse {
            responses,
            success_count,
            failure_count,
        }))
    }

    /// Get message status by ID
    async fn get_message_status(
        &self,
        request: Request<GetMessageStatusRequest>,
    ) -> Result<Response<GetMessageStatusResponse>, Status> {
        let req = request.into_inner();
        let message_id = Uuid::parse_str(&req.message_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid message_id: {}", e)))?;

        let repo = &self.outbox_repo;
        let message = repo
            .get_by_id(message_id)
            .await
            .map_err(|e| Status::not_found(format!("Message not found: {}", e)))?;

        let status = match message.status {
            rs_broker_db::outbox::MessageStatus::Pending => ProtoMessageStatus::Pending,
            rs_broker_db::outbox::MessageStatus::Publishing => ProtoMessageStatus::Publishing,
            rs_broker_db::outbox::MessageStatus::Published => ProtoMessageStatus::Published,
            rs_broker_db::outbox::MessageStatus::Retrying => ProtoMessageStatus::Retrying,
            rs_broker_db::outbox::MessageStatus::Failed => ProtoMessageStatus::Failed,
            rs_broker_db::outbox::MessageStatus::Dlq => ProtoMessageStatus::Dlq,
        };

        let response = GetMessageStatusResponse {
            message_id: req.message_id,
            status: status.into(),
            retry_count: message.retry_count,
            last_updated: message.updated_at.timestamp(),
            error_message: message.error_message.unwrap_or_default(),
            published_at: message.published_at.map(|t| t.timestamp()).unwrap_or(0),
            topic: message.topic,
        };

        Ok(Response::new(response))
    }

    /// Cancel a pending message
    async fn cancel_message(
        &self,
        request: Request<CancelMessageRequest>,
    ) -> Result<Response<CancelMessageResponse>, Status> {
        let req = request.into_inner();
        let message_id = Uuid::parse_str(&req.message_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid message_id: {}", e)))?;

        let repo = &self.outbox_repo;

        // Check if message exists and is pending
        let message = match repo.get_by_id(message_id).await {
            Ok(m) => m,
            Err(_) => {
                return Ok(Response::new(CancelMessageResponse {
                    success: false,
                    status: ProtoMessageStatus::Unspecified.into(),
                    error: "Message not found".to_string(),
                }));
            }
        };

        // Only pending messages can be cancelled
        if message.status != rs_broker_db::outbox::MessageStatus::Pending {
            return Ok(Response::new(CancelMessageResponse {
                success: false,
                status: ProtoMessageStatus::Pending.into(),
                error: "Only pending messages can be cancelled".to_string(),
            }));
        }

        // Delete the message
        repo.delete(message_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to cancel message: {}", e)))?;

        Ok(Response::new(CancelMessageResponse {
            success: true,
            status: ProtoMessageStatus::Pending.into(),
            error: String::new(),
        }))
    }

    /// Register a new subscriber
    async fn register_subscriber(
        &self,
        request: Request<RegisterSubscriberRequest>,
    ) -> Result<Response<RegisterSubscriberResponse>, Status> {
        let req = request.into_inner();

        let subscriber = Subscriber::new(req.service_name, req.grpc_endpoint, req.topic_patterns);

        let repo = &self.subscriber_repo;
        repo.create(&subscriber)
            .await
            .map_err(|e| Status::internal(format!("Failed to register subscriber: {}", e)))?;

        Ok(Response::new(RegisterSubscriberResponse {
            subscriber_id: subscriber.id.to_string(),
            success: true,
            error: String::new(),
        }))
    }

    /// Unregister a subscriber
    async fn unregister_subscriber(
        &self,
        request: Request<UnregisterSubscriberRequest>,
    ) -> Result<Response<UnregisterSubscriberResponse>, Status> {
        let req = request.into_inner();
        let subscriber_id = Uuid::parse_str(&req.subscriber_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid subscriber_id: {}", e)))?;

        let repo = &self.subscriber_repo;
        repo.deactivate(subscriber_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to unregister subscriber: {}", e)))?;

        Ok(Response::new(UnregisterSubscriberResponse {
            success: true,
            error: String::new(),
        }))
    }

    /// Update subscriber configuration
    async fn update_subscriber(
        &self,
        request: Request<UpdateSubscriberRequest>,
    ) -> Result<Response<UpdateSubscriberResponse>, Status> {
        let req = request.into_inner();
        let subscriber_id = Uuid::parse_str(&req.subscriber_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid subscriber_id: {}", e)))?;

        let repo = &self.subscriber_repo;
        let mut subscriber = repo
            .get_by_id(subscriber_id)
            .await
            .map_err(|e| Status::not_found(format!("Subscriber not found: {}", e)))?;

        // Update fields
        if !req.grpc_endpoint.is_empty() {
            subscriber.grpc_endpoint = req.grpc_endpoint;
        }
        if !req.topic_patterns.is_empty() {
            subscriber.topic_patterns = req.topic_patterns;
        }
        subscriber.active = req.active;

        repo.update(&subscriber)
            .await
            .map_err(|e| Status::internal(format!("Failed to update subscriber: {}", e)))?;

        Ok(Response::new(UpdateSubscriberResponse {
            success: true,
            error: String::new(),
        }))
    }

    /// List all subscribers
    async fn list_subscribers(
        &self,
        _request: Request<ListSubscribersRequest>,
    ) -> Result<Response<ListSubscribersResponse>, Status> {
        let repo = &self.subscriber_repo;
        let subscribers = repo
            .get_all_active()
            .await
            .map_err(|e| Status::internal(format!("Failed to list subscribers: {}", e)))?;

        let subscriber_infos: Vec<SubscriberInfo> = subscribers
            .into_iter()
            .map(|s| SubscriberInfo {
                subscriber_id: s.id.to_string(),
                service_name: s.service_name,
                grpc_endpoint: s.grpc_endpoint,
                topic_patterns: s.topic_patterns,
                active: s.active,
                registered_at: s.registered_at.timestamp(),
            })
            .collect();

        Ok(Response::new(ListSubscribersResponse {
            subscribers: subscriber_infos,
        }))
    }

    /// Subscribe to events (streaming)
    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let req = request.into_inner();
        let subscriber_id = req.subscriber_id;
        let patterns = req.topic_patterns;

        if subscriber_id.is_empty() {
            return Err(Status::invalid_argument("subscriber_id must not be empty"));
        }

        let receiver = self.event_sender.subscribe();

        let stream =
            tokio_stream::wrappers::BroadcastStream::new(receiver).filter_map(move |result| {
                let patterns = patterns.clone();
                let subscriber_id = subscriber_id.clone();
                async move {
                    match result {
                        Ok(event) => {
                            if topic_matches(&event.topic, &patterns) {
                                Some(Ok(event))
                            } else {
                                None
                            }
                        }
                        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(
                            skipped,
                        )) => {
                            tracing::warn!(
                                skipped,
                                subscriber_id = %subscriber_id,
                                "subscriber lagged behind broadcast"
                            );
                            None
                        }
                    }
                }
            });

        Ok(Response::new(Box::pin(stream)))
    }

    /// Bidirectional streaming publish: process each incoming PublishRequest
    /// and return a PublishResponse for each. Individual message failures are
    /// reported in the response rather than terminating the stream.
    async fn stream_publish(
        &self,
        request: Request<tonic::Streaming<PublishRequest>>,
    ) -> Result<Response<Self::StreamPublishStream>, Status> {
        let outbox_repo = self.outbox_repo.clone();
        let incoming = request.into_inner();

        let output = incoming.then(move |result| {
            let outbox_repo = outbox_repo.clone();
            async move {
                let req = match result {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(PublishResponse {
                            message_id: String::new(),
                            status: ProtoMessageStatus::Pending.into(),
                            duplicate: false,
                            accepted_at: 0,
                            error: format!("Stream error: {}", e),
                        });
                    }
                };

                let response = match process_publish(&outbox_repo, req).await {
                    Ok(resp) => resp,
                    Err(status) => PublishResponse {
                        message_id: String::new(),
                        status: ProtoMessageStatus::Pending.into(),
                        duplicate: false,
                        accepted_at: 0,
                        error: status.message().to_string(),
                    },
                };
                Ok(response)
            }
        });

        let stream: Self::StreamPublishStream = Box::pin(output);
        Ok(Response::new(stream))
    }

    /// Get health check
    async fn get_health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        // Check database connection
        let db_healthy = self.db_pool.acquire().await.is_ok();

        // Get metrics
        let outbox_repo = &self.outbox_repo;
        let subscriber_repo = &self.subscriber_repo;

        let outbox_count = outbox_repo
            .get_pending(1000)
            .await
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        let subscriber_count = subscriber_repo
            .get_all_active()
            .await
            .map(|s| s.len() as i64)
            .unwrap_or(0);

        let (status, components) = if db_healthy {
            (
                HealthStatus::Healthy.into(),
                vec![ComponentHealth {
                    name: "database".to_string(),
                    status: HealthStatus::Healthy.into(),
                    message: "Connected".to_string(),
                }],
            )
        } else {
            (
                HealthStatus::Unhealthy.into(),
                vec![ComponentHealth {
                    name: "database".to_string(),
                    status: HealthStatus::Unhealthy.into(),
                    message: "Connection failed".to_string(),
                }],
            )
        };

        let metrics = BrokerMetrics {
            outbox_pending: outbox_count,
            inbox_pending: 0,
            published_today: 0,
            processed_today: 0,
            dlq_count: 0,
            active_subscribers: subscriber_count,
            kafka_connected: self.kafka_connected,
            database_connected: db_healthy,
        };

        Ok(Response::new(HealthResponse {
            status,
            components,
            metrics: Some(metrics),
        }))
    }

    /// Reprocess DLQ messages
    async fn reprocess_dlq(
        &self,
        request: Request<ReprocessDlqRequest>,
    ) -> Result<Response<ReprocessDlqResponse>, Status> {
        let req = request.into_inner();

        let selector = if !req.message_id.is_empty() {
            let id = Uuid::parse_str(&req.message_id)
                .map_err(|e| Status::invalid_argument(format!("Invalid message_id: {}", e)))?;
            DlqSelector::Id(id)
        } else if !req.topic.is_empty() {
            DlqSelector::Topic(&req.topic)
        } else if req.all {
            DlqSelector::All
        } else {
            return Err(Status::invalid_argument(
                "One of message_id, topic, or all must be specified",
            ));
        };

        let result = self
            .dlq_handler
            .reprocess(selector, &self.outbox_repo)
            .await
            .map_err(|e| Status::internal(format!("Reprocess failed: {}", e)))?;

        Ok(Response::new(ReprocessDlqResponse {
            reprocessed_count: result.reprocessed_count,
            failure_count: result.failure_count,
            errors: result.errors,
        }))
    }

    /// List DLQ messages
    async fn list_dlq_messages(
        &self,
        request: Request<ListDlqMessagesRequest>,
    ) -> Result<Response<ListDlqMessagesResponse>, Status> {
        let req = request.into_inner();

        let topic = if req.topic.is_empty() {
            None
        } else {
            Some(req.topic.as_str())
        };
        let limit = if req.limit <= 0 { 50 } else { req.limit } as i64;
        let offset = req.offset as i64;

        let messages = self
            .dlq_handler
            .get_messages(topic, limit, offset)
            .await
            .map_err(|e| Status::internal(format!("Failed to list DLQ messages: {}", e)))?;

        let total_count = self
            .dlq_handler
            .count(topic)
            .await
            .map_err(|e| Status::internal(format!("Failed to count DLQ messages: {}", e)))?
            as i32;

        let proto_messages: Vec<DlqMessageInfo> = messages
            .into_iter()
            .map(|m| DlqMessageInfo {
                message_id: m.id.to_string(),
                original_topic: m.original_topic,
                dlq_topic: m.dlq_topic,
                failure_reason: m.failure_reason,
                retry_count: m.retry_count,
                created_at: m.created_at.timestamp(),
            })
            .collect();

        Ok(Response::new(ListDlqMessagesResponse {
            messages: proto_messages,
            total_count,
        }))
    }
}

#[tonic::async_trait]
#[cfg(not(any(feature = "postgres", feature = "mysql")))]
impl RsBroker for RsBrokerService {
    type SubscribeEventsStream = std::pin::Pin<
        Box<
            dyn tonic::codegen::tokio_stream::Stream<Item = Result<DeliverEvent, tonic::Status>>
                + Send,
        >,
    >;
    type StreamPublishStream = std::pin::Pin<
        Box<
            dyn tonic::codegen::tokio_stream::Stream<Item = Result<PublishResponse, tonic::Status>>
                + Send,
        >,
    >;

    /// Publish a single message to the outbox
    async fn publish(
        &self,
        _request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Publish multiple messages in a batch
    async fn publish_batch(
        &self,
        _request: Request<PublishBatchRequest>,
    ) -> Result<Response<PublishBatchResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Get message status by ID
    async fn get_message_status(
        &self,
        _request: Request<GetMessageStatusRequest>,
    ) -> Result<Response<GetMessageStatusResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Cancel a pending message
    async fn cancel_message(
        &self,
        _request: Request<CancelMessageRequest>,
    ) -> Result<Response<CancelMessageResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Register a new subscriber
    async fn register_subscriber(
        &self,
        _request: Request<RegisterSubscriberRequest>,
    ) -> Result<Response<RegisterSubscriberResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Unregister a subscriber
    async fn unregister_subscriber(
        &self,
        _request: Request<UnregisterSubscriberRequest>,
    ) -> Result<Response<UnregisterSubscriberResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Update subscriber configuration
    async fn update_subscriber(
        &self,
        _request: Request<UpdateSubscriberRequest>,
    ) -> Result<Response<UpdateSubscriberResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// List all subscribers
    async fn list_subscribers(
        &self,
        _request: Request<ListSubscribersRequest>,
    ) -> Result<Response<ListSubscribersResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Subscribe to events (streaming)
    async fn subscribe_events(
        &self,
        _request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// Stream publish (bidirectional streaming - not fully implemented)
    async fn stream_publish(
        &self,
        _request: Request<tonic::Streaming<PublishRequest>>,
    ) -> Result<Response<Self::StreamPublishStream>, Status> {
        // Streaming implementation would require more complex setup
        let stream: Self::StreamPublishStream = Box::pin(futures_util::stream::empty());
        Ok(Response::new(stream))
    }

    /// Get health check
    async fn get_health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        // When no database features are enabled, report basic health
        let status = HealthStatus::Healthy.into();
        let components = vec![ComponentHealth {
            name: "database".to_string(),
            status: HealthStatus::Unhealthy.into(),
            message: "Database features disabled".to_string(),
        }];

        let metrics = BrokerMetrics {
            outbox_pending: 0,
            inbox_pending: 0,
            published_today: 0,
            processed_today: 0,
            dlq_count: 0,
            active_subscribers: 0,
            kafka_connected: self.kafka_connected,
            database_connected: false,
        };

        Ok(Response::new(HealthResponse {
            status,
            components,
            metrics: Some(metrics),
        }))
    }

    /// Reprocess DLQ messages
    async fn reprocess_dlq(
        &self,
        _request: Request<ReprocessDlqRequest>,
    ) -> Result<Response<ReprocessDlqResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    /// List DLQ messages
    async fn list_dlq_messages(
        &self,
        _request: Request<ListDlqMessagesRequest>,
    ) -> Result<Response<ListDlqMessagesResponse>, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }
}

/// Check if a topic matches any of the given patterns.
///
/// Supports:
/// - `*` matches everything
/// - `prefix.*` matches any topic starting with `prefix.`
/// - Exact string match otherwise
fn topic_matches(topic: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    for pattern in patterns {
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix(".*") {
            if topic.starts_with(&format!("{}.", prefix)) {
                return true;
            }
        } else if topic == pattern {
            return true;
        }
    }
    false
}
