//! gRPC service implementation for RsBroker

use chrono::Utc;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use rs_broker_proto::rsbroker::{
    rs_broker_server::RsBroker, BrokerMetrics, CancelMessageRequest, CancelMessageResponse,
    ComponentHealth, DeliverEvent, GetMessageStatusRequest, GetMessageStatusResponse,
    HealthRequest, HealthResponse, HealthStatus, ListDlqMessagesRequest, ListDlqMessagesResponse,
    ListSubscribersRequest, ListSubscribersResponse, MessageStatus as ProtoMessageStatus,
    PublishBatchRequest, PublishBatchResponse, PublishRequest, PublishResponse,
    RegisterSubscriberRequest, RegisterSubscriberResponse, ReprocessDlqRequest,
    ReprocessDlqResponse, SubscribeEventsRequest, SubscriberInfo, UnregisterSubscriberRequest,
    UnregisterSubscriberResponse, UpdateSubscriberRequest, UpdateSubscriberResponse,
};

#[cfg(any(feature = "postgres", feature = "mysql"))]
use rs_broker_core::OutboxManager;
use rs_broker_db::{
    OutboxMessage, OutboxRepository, SqlxOutboxRepository, SqlxSubscriberRepository, Subscriber,
    SubscriberRepository,
};

/// RsBroker gRPC service implementation
pub struct RsBrokerService {
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    db_pool: rs_broker_db::DbPool,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    _outbox_manager: OutboxManager,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    outbox_repo: SqlxOutboxRepository,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    subscriber_repo: SqlxSubscriberRepository,
    #[cfg(all(not(feature = "postgres"), not(feature = "mysql")))]
    _phantom: std::marker::PhantomData<()>,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl RsBrokerService {
    /// Create a new RsBroker service
    pub fn new(db_pool: rs_broker_db::DbPool) -> Self {
        let outbox_manager =
            OutboxManager::new(db_pool.clone(), rs_broker_config::RetryConfig::default());
        let outbox_repo = SqlxOutboxRepository::new(db_pool.clone());
        let subscriber_repo = SqlxSubscriberRepository::new(db_pool.clone());
        Self {
            db_pool,
            _outbox_manager: outbox_manager,
            outbox_repo,
            subscriber_repo,
        }
    }
}

#[cfg(not(any(feature = "postgres", feature = "mysql")))]
impl RsBrokerService {
    /// Create a new RsBroker service (stub for when no database features are enabled)
    pub fn new(_db_pool: ()) -> Self {
        Self {
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

        // Generate message ID if not provided
        let message_id = if req.message_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            req.message_id
        };

        // Parse payload from bytes to JSON
        let payload: serde_json::Value = if req.payload.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&req.payload)
                .map_err(|e| Status::invalid_argument(format!("Invalid payload JSON: {}", e)))?
        };

        // Parse headers
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

        // Create outbox message
        let mut message = OutboxMessage::new(
            req.aggregate_type,
            req.aggregate_id,
            req.event_type,
            payload,
            req.topic,
        );

        // Override with provided values
        message.id = Uuid::parse_str(&message_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid message_id: {}", e)))?;
        message.headers = headers;
        message.partition_key = if req.partition_key.is_empty() {
            None
        } else {
            Some(req.partition_key)
        };

        // Save to database
        let repo = &self.outbox_repo;
        repo.create(&message)
            .await
            .map_err(|e| Status::internal(format!("Failed to create message: {}", e)))?;

        let response = PublishResponse {
            message_id,
            status: ProtoMessageStatus::Pending.into(),
            duplicate: false,
            accepted_at: Utc::now().timestamp(),
            error: String::new(),
        };

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

    /// Subscribe to events (streaming - not fully implemented)
    async fn subscribe_events(
        &self,
        _request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        // Streaming implementation would require more complex setup
        let stream: Self::SubscribeEventsStream = Box::pin(futures_util::stream::empty());
        Ok(Response::new(stream))
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
            kafka_connected: false, // Would check actual Kafka connection
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
        _request: Request<ReprocessDlqRequest>,
    ) -> Result<Response<ReprocessDlqResponse>, Status> {
        // DLQ reprocessing would require more complex setup
        Err(Status::unimplemented(
            "DLQ reprocessing not yet implemented",
        ))
    }

    /// List DLQ messages
    async fn list_dlq_messages(
        &self,
        _request: Request<ListDlqMessagesRequest>,
    ) -> Result<Response<ListDlqMessagesResponse>, Status> {
        // DLQ listing would require more complex setup
        Err(Status::unimplemented("DLQ listing not yet implemented"))
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

    /// Subscribe to events (streaming - not fully implemented)
    async fn subscribe_events(
        &self,
        _request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        // Streaming implementation would require more complex setup
        let stream: Self::SubscribeEventsStream = Box::pin(futures_util::stream::empty());
        Ok(Response::new(stream))
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
            kafka_connected: false, // Would check actual Kafka connection
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
