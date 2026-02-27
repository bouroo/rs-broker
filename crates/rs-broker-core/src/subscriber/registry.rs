//! Subscriber registry

use uuid::Uuid;

use crate::error::Result;
use rs_broker_db::subscriber::repository::SqlxSubscriberRepository;
use rs_broker_db::{DbPool, Subscriber, SubscriberRepository};

/// Subscriber registry for managing subscribers
pub struct SubscriberRegistry {
    repository: Box<dyn SubscriberRepository>,
}

impl SubscriberRegistry {
    /// Create a new subscriber registry
    pub fn new(pool: DbPool) -> Self {
        let repository = SqlxSubscriberRepository::new(pool);
        Self {
            repository: Box::new(repository),
        }
    }

    /// Register a new subscriber
    pub async fn register(
        &self,
        service_name: String,
        grpc_endpoint: String,
        topic_patterns: Vec<String>,
    ) -> Result<Uuid> {
        let subscriber = Subscriber::new(service_name, grpc_endpoint, topic_patterns);

        self.repository.create(&subscriber).await?;

        Ok(subscriber.id)
    }

    /// Unregister a subscriber
    pub async fn unregister(&self, id: Uuid) -> Result<()> {
        Ok(self.repository.deactivate(id).await?)
    }

    /// Get a subscriber by ID
    pub async fn get(&self, id: Uuid) -> Result<Subscriber> {
        Ok(self.repository.get_by_id(id).await?)
    }

    /// List all active subscribers
    pub async fn list_active(&self) -> Result<Vec<Subscriber>> {
        Ok(self.repository.get_all_active().await?)
    }

    /// Update subscriber
    pub async fn update(&self, subscriber: &Subscriber) -> Result<()> {
        Ok(self.repository.update(subscriber).await?)
    }
}
