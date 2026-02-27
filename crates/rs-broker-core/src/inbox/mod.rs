//! Inbox module

pub mod dedup;
pub mod dispatcher;
pub mod manager;

pub use dedup::Deduplicator;
pub use dispatcher::Dispatcher;
pub use manager::InboxManager;
