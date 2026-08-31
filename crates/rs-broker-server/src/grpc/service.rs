//! gRPC service implementation for RsBroker
//!
//! All RPC bodies live in transport-free `*_inner` methods on
//! [`RsBrokerService`] so the HTTP/REST layer (`crate::http`) can serve the
//! same operations over the shared state without duplicating business logic.
//! The tonic trait impls in this module are thin delegates.

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

use rs_broker_core::features::publishing::{AcceptError, AcceptMessage, PublishRequestInput};
use rs_broker_db::{
    OutboxRepository, SqlxOutboxRepository, SqlxSubscriberRepository, Subscriber,
    SubscriberRepository,
};

/// Interface-adapter shell around the [`AcceptMessage`] use case: converts
/// the wire request to the transport-free input, delegates validation and
/// outbox insertion to the feature, and maps back to the wire response.
///
/// This is a free function so it can be called from both the unary `publish`
/// method and the bidirectional `stream_publish` without needing to clone the
/// whole service.
//
// `tonic::Status` as the Err variant exceeds clippy's `result_large_err`
// threshold; matching the generated proto traits' own exemption.
#[allow(clippy::result_large_err)]
async fn process_publish(
    use_case: &AcceptMessage,
    req: PublishRequest,
) -> Result<PublishResponse, Status> {
    let input = PublishRequestInput {
        message_id: Some(req.message_id),
        aggregate_type: req.aggregate_type,
        aggregate_id: req.aggregate_id,
        event_type: req.event_type,
        payload: req.payload,
        headers: req.headers.into_iter().map(|h| (h.key, h.value)).collect(),
        topic: req.topic,
        partition_key: Some(req.partition_key).filter(|k| !k.is_empty()),
    };

    let message_id = use_case.apply(input).await.map_err(|e| match e {
        // Wire-compatible error text (validation messages preserved).
        AcceptError::InvalidPayload(_) | AcceptError::InvalidMessageId(_) => {
            Status::invalid_argument(e.to_string())
        }
        _ => Status::internal(e.to_string()),
    })?;

    Ok(PublishResponse {
        message_id: message_id.to_string(),
        status: ProtoMessageStatus::Pending.into(),
        duplicate: false,
        accepted_at: Utc::now().timestamp(),
        error: String::new(),
    })
}

/// RsBroker gRPC service implementation.
///
/// Cheap to clone: repositories and use cases share pooled/arced state, so
/// one instance can back the tonic server while the HTTP layer holds another.
#[derive(Clone)]
pub struct RsBrokerService {
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    db_pool: rs_broker_db::DbPool,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    outbox_repo: SqlxOutboxRepository,
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    accept_message: AcceptMessage,
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
        let accept_message = AcceptMessage::new(std::sync::Arc::new(outbox_repo.clone()));
        let subscriber_repo = SqlxSubscriberRepository::new(db_pool.clone());
        let dlq_handler = DlqHandler::new(std::sync::Arc::new(
            rs_broker_db::SqlxDlqRepository::new(db_pool.clone()),
        ));
        let (event_sender, _) = broadcast::channel(1024);
        Self {
            db_pool,
            outbox_repo,
            accept_message,
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

    /// Shared body of the unary publish RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn publish_inner(
        &self,
        req: PublishRequest,
    ) -> Result<PublishResponse, Status> {
        process_publish(&self.accept_message, req).await
    }

    /// Shared body of the batch publish RPC: individual failures are reported
    /// per message rather than failing the whole batch.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn publish_batch_inner(
        &self,
        req: PublishBatchRequest,
    ) -> Result<PublishBatchResponse, Status> {
        let mut responses = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;

        for msg_req in req.messages {
            match self.publish_inner(msg_req).await {
                Ok(resp) => {
                    success_count += 1;
                    responses.push(resp);
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

        Ok(PublishBatchResponse {
            responses,
            success_count,
            failure_count,
        })
    }

    /// Shared body of the message status RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn get_message_status_inner(
        &self,
        req: GetMessageStatusRequest,
    ) -> Result<GetMessageStatusResponse, Status> {
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

        Ok(GetMessageStatusResponse {
            message_id: req.message_id,
            status: status.into(),
            retry_count: message.retry_count,
            last_updated: message.updated_at.timestamp(),
            error_message: message.error_message.unwrap_or_default(),
            published_at: message.published_at.map(|t| t.timestamp()).unwrap_or(0),
            topic: message.topic,
        })
    }

    /// Shared body of the cancel RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn cancel_message_inner(
        &self,
        req: CancelMessageRequest,
    ) -> Result<CancelMessageResponse, Status> {
        let message_id = Uuid::parse_str(&req.message_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid message_id: {}", e)))?;

        let repo = &self.outbox_repo;

        // Check if message exists and is pending
        let message = match repo.get_by_id(message_id).await {
            Ok(m) => m,
            Err(_) => {
                return Ok(CancelMessageResponse {
                    success: false,
                    status: ProtoMessageStatus::Unspecified.into(),
                    error: "Message not found".to_string(),
                });
            }
        };

        // Only pending messages can be cancelled
        if message.status != rs_broker_db::outbox::MessageStatus::Pending {
            return Ok(CancelMessageResponse {
                success: false,
                status: ProtoMessageStatus::Pending.into(),
                error: "Only pending messages can be cancelled".to_string(),
            });
        }

        // Delete the message
        repo.delete(message_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to cancel message: {}", e)))?;

        Ok(CancelMessageResponse {
            success: true,
            status: ProtoMessageStatus::Pending.into(),
            error: String::new(),
        })
    }

    /// Shared body of the register subscriber RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn register_subscriber_inner(
        &self,
        req: RegisterSubscriberRequest,
    ) -> Result<RegisterSubscriberResponse, Status> {
        let subscriber = Subscriber::new(req.service_name, req.grpc_endpoint, req.topic_patterns);

        let repo = &self.subscriber_repo;
        repo.create(&subscriber)
            .await
            .map_err(|e| Status::internal(format!("Failed to register subscriber: {}", e)))?;

        Ok(RegisterSubscriberResponse {
            subscriber_id: subscriber.id.to_string(),
            success: true,
            error: String::new(),
        })
    }

    /// Shared body of the unregister subscriber RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn unregister_subscriber_inner(
        &self,
        req: UnregisterSubscriberRequest,
    ) -> Result<UnregisterSubscriberResponse, Status> {
        let subscriber_id = Uuid::parse_str(&req.subscriber_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid subscriber_id: {}", e)))?;

        let repo = &self.subscriber_repo;
        repo.deactivate(subscriber_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to unregister subscriber: {}", e)))?;

        Ok(UnregisterSubscriberResponse {
            success: true,
            error: String::new(),
        })
    }

    /// Shared body of the update subscriber RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn update_subscriber_inner(
        &self,
        req: UpdateSubscriberRequest,
    ) -> Result<UpdateSubscriberResponse, Status> {
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

        Ok(UpdateSubscriberResponse {
            success: true,
            error: String::new(),
        })
    }

    /// Shared body of the list subscribers RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn list_subscribers_inner(
        &self,
        _req: ListSubscribersRequest,
    ) -> Result<ListSubscribersResponse, Status> {
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

        Ok(ListSubscribersResponse {
            subscribers: subscriber_infos,
        })
    }

    /// Validate a subscription request and return a fresh broadcast receiver.
    ///
    /// Shared by the gRPC server-stream and the HTTP SSE handler; each
    /// transport applies its own topic filtering and lag policy on top.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) fn subscribe_events_inner(
        &self,
        subscriber_id: &str,
    ) -> Result<broadcast::Receiver<DeliverEvent>, Status> {
        if subscriber_id.is_empty() {
            return Err(Status::invalid_argument("subscriber_id must not be empty"));
        }

        Ok(self.event_sender.subscribe())
    }

    /// Shared body of the health RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn get_health_inner(
        &self,
        _req: HealthRequest,
    ) -> Result<HealthResponse, Status> {
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

        Ok(HealthResponse {
            status,
            components,
            metrics: Some(metrics),
        })
    }

    /// Shared body of the DLQ reprocess RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn reprocess_dlq_inner(
        &self,
        req: ReprocessDlqRequest,
    ) -> Result<ReprocessDlqResponse, Status> {
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

        Ok(ReprocessDlqResponse {
            reprocessed_count: result.reprocessed_count,
            failure_count: result.failure_count,
            errors: result.errors,
        })
    }

    /// Shared body of the list DLQ RPC.
    #[allow(clippy::result_large_err)] // tonic::Status, same as generated traits
    pub(crate) async fn list_dlq_messages_inner(
        &self,
        req: ListDlqMessagesRequest,
    ) -> Result<ListDlqMessagesResponse, Status> {
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

        Ok(ListDlqMessagesResponse {
            messages: proto_messages,
            total_count,
        })
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

    #[allow(clippy::result_large_err)]
    fn unimplemented<T>() -> Result<T, Status> {
        Err(Status::unimplemented("Database features disabled"))
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn publish_inner(
        &self,
        _req: PublishRequest,
    ) -> Result<PublishResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn publish_batch_inner(
        &self,
        _req: PublishBatchRequest,
    ) -> Result<PublishBatchResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn get_message_status_inner(
        &self,
        _req: GetMessageStatusRequest,
    ) -> Result<GetMessageStatusResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn cancel_message_inner(
        &self,
        _req: CancelMessageRequest,
    ) -> Result<CancelMessageResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn register_subscriber_inner(
        &self,
        _req: RegisterSubscriberRequest,
    ) -> Result<RegisterSubscriberResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn unregister_subscriber_inner(
        &self,
        _req: UnregisterSubscriberRequest,
    ) -> Result<UnregisterSubscriberResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn update_subscriber_inner(
        &self,
        _req: UpdateSubscriberRequest,
    ) -> Result<UpdateSubscriberResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn list_subscribers_inner(
        &self,
        _req: ListSubscribersRequest,
    ) -> Result<ListSubscribersResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn subscribe_events_inner(
        &self,
        _subscriber_id: &str,
    ) -> Result<broadcast::Receiver<DeliverEvent>, Status> {
        Self::unimplemented()
    }

    /// Stub health: reports the service itself as up, the database as
    /// disabled — mirrors the historical stub RPC body.
    #[allow(clippy::result_large_err)]
    pub(crate) async fn get_health_inner(
        &self,
        _req: HealthRequest,
    ) -> Result<HealthResponse, Status> {
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

        Ok(HealthResponse {
            status: HealthStatus::Healthy.into(),
            components,
            metrics: Some(metrics),
        })
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn reprocess_dlq_inner(
        &self,
        _req: ReprocessDlqRequest,
    ) -> Result<ReprocessDlqResponse, Status> {
        Self::unimplemented()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) async fn list_dlq_messages_inner(
        &self,
        _req: ListDlqMessagesRequest,
    ) -> Result<ListDlqMessagesResponse, Status> {
        Self::unimplemented()
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
        let response = self.publish_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Publish multiple messages in a batch
    async fn publish_batch(
        &self,
        request: Request<PublishBatchRequest>,
    ) -> Result<Response<PublishBatchResponse>, Status> {
        let response = self.publish_batch_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Get message status by ID
    async fn get_message_status(
        &self,
        request: Request<GetMessageStatusRequest>,
    ) -> Result<Response<GetMessageStatusResponse>, Status> {
        let response = self.get_message_status_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Cancel a pending message
    async fn cancel_message(
        &self,
        request: Request<CancelMessageRequest>,
    ) -> Result<Response<CancelMessageResponse>, Status> {
        let response = self.cancel_message_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Register a new subscriber
    async fn register_subscriber(
        &self,
        request: Request<RegisterSubscriberRequest>,
    ) -> Result<Response<RegisterSubscriberResponse>, Status> {
        let response = self.register_subscriber_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Unregister a subscriber
    async fn unregister_subscriber(
        &self,
        request: Request<UnregisterSubscriberRequest>,
    ) -> Result<Response<UnregisterSubscriberResponse>, Status> {
        let response = self
            .unregister_subscriber_inner(request.into_inner())
            .await?;
        Ok(Response::new(response))
    }

    /// Update subscriber configuration
    async fn update_subscriber(
        &self,
        request: Request<UpdateSubscriberRequest>,
    ) -> Result<Response<UpdateSubscriberResponse>, Status> {
        let response = self.update_subscriber_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// List all subscribers
    async fn list_subscribers(
        &self,
        request: Request<ListSubscribersRequest>,
    ) -> Result<Response<ListSubscribersResponse>, Status> {
        let response = self.list_subscribers_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Subscribe to events (streaming)
    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let req = request.into_inner();
        let subscriber_id = req.subscriber_id;
        let patterns = req.topic_patterns;
        let receiver = self.subscribe_events_inner(&subscriber_id)?;

        let stream =
            tokio_stream::wrappers::BroadcastStream::new(receiver).filter_map(move |result| {
                let patterns = patterns.clone();
                let subscriber_id = subscriber_id.clone();
                async move {
                    match result {
                        Ok(event) => {
                            // Same unified MQTT-semantics matcher the gRPC
                            // callback fan-out uses, so both inbound delivery
                            // paths agree on what a subscriber receives.
                            if rs_broker_core::matches_any(&event.topic, &patterns) {
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
        let accept_message = self.accept_message.clone();
        let incoming = request.into_inner();

        let output = incoming.then(move |result| {
            let accept_message = accept_message.clone();
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

                let response = match process_publish(&accept_message, req).await {
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
        request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let response = self.get_health_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Reprocess DLQ messages
    async fn reprocess_dlq(
        &self,
        request: Request<ReprocessDlqRequest>,
    ) -> Result<Response<ReprocessDlqResponse>, Status> {
        let response = self.reprocess_dlq_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// List DLQ messages
    async fn list_dlq_messages(
        &self,
        request: Request<ListDlqMessagesRequest>,
    ) -> Result<Response<ListDlqMessagesResponse>, Status> {
        let response = self.list_dlq_messages_inner(request.into_inner()).await?;
        Ok(Response::new(response))
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
        request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        let response = self.publish_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Publish multiple messages in a batch
    async fn publish_batch(
        &self,
        request: Request<PublishBatchRequest>,
    ) -> Result<Response<PublishBatchResponse>, Status> {
        let response = self.publish_batch_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Get message status by ID
    async fn get_message_status(
        &self,
        request: Request<GetMessageStatusRequest>,
    ) -> Result<Response<GetMessageStatusResponse>, Status> {
        let response = self.get_message_status_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Cancel a pending message
    async fn cancel_message(
        &self,
        request: Request<CancelMessageRequest>,
    ) -> Result<Response<CancelMessageResponse>, Status> {
        let response = self.cancel_message_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Register a new subscriber
    async fn register_subscriber(
        &self,
        request: Request<RegisterSubscriberRequest>,
    ) -> Result<Response<RegisterSubscriberResponse>, Status> {
        let response = self.register_subscriber_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Unregister a subscriber
    async fn unregister_subscriber(
        &self,
        request: Request<UnregisterSubscriberRequest>,
    ) -> Result<Response<UnregisterSubscriberResponse>, Status> {
        let response = self
            .unregister_subscriber_inner(request.into_inner())
            .await?;
        Ok(Response::new(response))
    }

    /// Update subscriber configuration
    async fn update_subscriber(
        &self,
        request: Request<UpdateSubscriberRequest>,
    ) -> Result<Response<UpdateSubscriberResponse>, Status> {
        let response = self.update_subscriber_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// List all subscribers
    async fn list_subscribers(
        &self,
        request: Request<ListSubscribersRequest>,
    ) -> Result<Response<ListSubscribersResponse>, Status> {
        let response = self.list_subscribers_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Subscribe to events (streaming)
    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let subscriber_id = request.into_inner().subscriber_id;
        self.subscribe_events_inner(&subscriber_id)?;
        unreachable!("stub subscribe_events_inner always returns an error")
    }

    /// Stream publish (bidirectional streaming - not implemented in stub builds)
    async fn stream_publish(
        &self,
        _request: Request<tonic::Streaming<PublishRequest>>,
    ) -> Result<Response<Self::StreamPublishStream>, Status> {
        let stream: Self::StreamPublishStream = Box::pin(futures_util::stream::empty());
        Ok(Response::new(stream))
    }

    /// Get health check
    async fn get_health(
        &self,
        request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let response = self.get_health_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// Reprocess DLQ messages
    async fn reprocess_dlq(
        &self,
        request: Request<ReprocessDlqRequest>,
    ) -> Result<Response<ReprocessDlqResponse>, Status> {
        let response = self.reprocess_dlq_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    /// List DLQ messages
    async fn list_dlq_messages(
        &self,
        request: Request<ListDlqMessagesRequest>,
    ) -> Result<Response<ListDlqMessagesResponse>, Status> {
        let response = self.list_dlq_messages_inner(request.into_inner()).await?;
        Ok(Response::new(response))
    }
}

/// Check if a topic matches any of the given patterns.
///
/// Uses the unified MQTT-semantics matcher (`rs_broker_core::matches_any`),
/// same as the gRPC callback fan-out; see its module docs for the wildcard
/// contract.
#[cfg(test)]
mod subscribe_events_filter {
    use rs_broker_core::matches_any;

    /// `*` as a full segment matches exactly one segment (MQTT), not a suffix.
    #[test]
    fn single_segment_star_wildcard() {
        assert!(matches_any("user.created", &["user.*".to_string()]));
        assert!(!matches_any(
            "user.profile.updated",
            &["user.*".to_string()]
        ));
        assert!(matches_any(
            "user.profile",
            &["user.#".to_string(), "user.*.profile".to_string()]
        ));
    }

    /// Empty pattern lists match nothing, consistent with the fan-out path.
    #[test]
    fn empty_patterns_match_nothing() {
        assert!(!matches_any("user.created", &[]));
    }

    /// `#` matches zero or more remaining segments; `+` exactly one.
    #[test]
    fn plus_and_hash_wildcards() {
        assert!(matches_any("user.created", &["user.+".to_string()]));
        assert!(matches_any("user", &["user.#".to_string()]));
        assert!(matches_any("a.b.c.d", &["#".to_string()]));
    }
}
