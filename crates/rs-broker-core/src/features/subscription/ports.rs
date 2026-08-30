//! Subscription ports.

use async_trait::async_trait;
use uuid::Uuid;

use super::domain::Subscriber;

/// Error type for subscriber repository operations.
#[derive(Debug, thiserror::Error)]
pub enum SubscriberError {
    /// Storage failure; the storage driver's message is carried verbatim so
    /// operator-facing error text is unchanged across adapter swaps.
    #[error("Database error: {0}")]
    Database(String),

    #[error("Subscriber not found: {0}")]
    NotFound(Uuid),
}

/// Port: durable storage for subscriber registrations.
#[async_trait]
pub trait SubscriberRepository: Send + Sync {
    /// Create a new subscriber
    async fn create(&self, subscriber: &Subscriber) -> Result<(), SubscriberError>;

    /// Get a subscriber by ID
    async fn get_by_id(&self, id: Uuid) -> Result<Subscriber, SubscriberError>;

    /// Get all active subscribers
    async fn get_all_active(&self) -> Result<Vec<Subscriber>, SubscriberError>;

    /// Update a subscriber
    async fn update(&self, subscriber: &Subscriber) -> Result<(), SubscriberError>;

    /// Delete a subscriber
    async fn delete(&self, id: Uuid) -> Result<(), SubscriberError>;

    /// Deactivate a subscriber
    async fn deactivate(&self, id: Uuid) -> Result<(), SubscriberError>;
}
