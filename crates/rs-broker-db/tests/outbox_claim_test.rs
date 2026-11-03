//! Integration tests for the outbox drain protocol against real PostgreSQL.
//!
//! Proves the claim/mark contract the outbox publisher relies on:
//!   1. Concurrent `claim_pending` calls return disjoint sets (SKIP LOCKED).
//!   2. `mark_published_batch` only marks rows still in `publishing`.
//!   3. Within-lease claims stay owned; `publishing` rows past the lease are
//!      reclaimable.
//!
//! Postgres comes from `TEST_DATABASE_URL` when set (Podman-in-container
//! environments), otherwise a fresh testcontainer. The scenarios share one
//! test function because an external database is shared between runs: the
//! table is truncated up front to make counts deterministic.

use std::sync::Arc;
use std::time::Duration;

use rs_broker_config::DatabaseConfig;
use rs_broker_core::features::publishing::{MessageStatus, OutboxMessage};
use rs_broker_db::{create_pool, run_migrations, DbPool, OutboxRepository, SqlxOutboxRepository};
use testcontainers::{
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};
use uuid::Uuid;

/// Postgres from `TEST_DATABASE_URL`, or a fresh testcontainer.
async fn test_pool() -> (DbPool, Option<ContainerAsync<GenericImage>>) {
    let url = match std::env::var("TEST_DATABASE_URL").ok() {
        Some(url) => url,
        None => {
            let postgres = GenericImage::new("postgres", "18-alpine")
                .with_wait_for(WaitFor::message_on_stdout(
                    "database system is ready to accept connections",
                ))
                .with_exposed_port(ContainerPort::Tcp(5432))
                .with_startup_timeout(Duration::from_secs(120))
                .with_env_var("POSTGRES_USER", "rsbroker")
                .with_env_var("POSTGRES_PASSWORD", "rsbroker_dev_password")
                .with_env_var("POSTGRES_DB", "rsbroker")
                .start()
                .await
                .expect("failed to start postgres container");
            let port = postgres
                .get_host_port_ipv4(5432)
                .await
                .expect("failed to get postgres port");
            let host = postgres
                .get_host()
                .await
                .expect("failed to get postgres host")
                .to_string();
            format!(
                "postgres://rsbroker:rsbroker_dev_password@{}:{}/rsbroker",
                host, port
            )
        }
    };

    let config = DatabaseConfig {
        url,
        ..Default::default()
    };

    // Postgres prints the ready message before it actually accepts
    // authenticated connections from new pools, so retry a few times.
    let mut last_err = None;
    for _ in 0..30 {
        match create_pool(&config).await {
            Ok(pool) => return (pool, None),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
    panic!("failed to create db pool after retries: {:?}", last_err);
}

fn message() -> OutboxMessage {
    OutboxMessage::new(
        "Order".to_string(),
        Uuid::now_v7().to_string(),
        "OrderCreated".to_string(),
        serde_json::json!({"amount": 1}),
        "claim-test".to_string(),
    )
}

async fn set_status(pool: &DbPool, id: Uuid, status: &str) {
    sqlx::query("UPDATE outbox_messages SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outbox_claim_protocol_holds_under_concurrency_and_leases() {
    let (pool, _container) = test_pool().await;
    run_migrations(&pool).await.expect("migrations failed");
    sqlx::query("TRUNCATE outbox_messages CASCADE")
        .execute(&pool)
        .await
        .unwrap();

    let repo = Arc::new(SqlxOutboxRepository::new(pool.clone()));

    // --- 1. Concurrent claims are disjoint and cover every pending row. ---
    let messages: Vec<OutboxMessage> = (0..200).map(|_| message()).collect();
    let expected: Vec<Uuid> = messages.iter().map(|m| m.id).collect();
    repo.create_batch(&messages).await.unwrap();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let repo = repo.clone();
        handles.push(tokio::spawn(async move {
            repo.claim_pending(50, 60).await.unwrap()
        }));
    }
    let mut claimed = Vec::new();
    for handle in handles {
        claimed.extend(handle.await.unwrap());
    }

    let mut claimed_ids: Vec<Uuid> = claimed.iter().map(|m| m.id).collect();
    claimed_ids.sort();
    let mut distinct = claimed_ids.clone();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        claimed_ids.len(),
        "claims must be disjoint under concurrency"
    );
    assert_eq!(
        distinct.len(),
        expected.len(),
        "every pending row must be claimed exactly once"
    );
    assert!(claimed
        .iter()
        .all(|m| m.status == MessageStatus::Publishing));

    // --- 2. Batch mark only touches rows still in `publishing`. -----------
    // Simulate a lease-expired row re-claimed elsewhere: `c` is no longer
    // owned by this batch.
    let (a, b, c) = (claimed_ids[0], claimed_ids[1], claimed_ids[2]);
    set_status(&pool, c, "pending").await;

    let marked = repo.mark_published_batch(&[a, b, c]).await.unwrap();
    assert_eq!(marked, 2, "re-claimed rows must not be marked");

    for id in [a, b] {
        let row = repo.get_by_id(id).await.unwrap();
        assert_eq!(row.status, MessageStatus::Published);
    }
    let row = repo.get_by_id(c).await.unwrap();
    assert_eq!(row.status, MessageStatus::Pending);

    // --- 3. Lease boundary -------------------------------------------------
    // The table's BEFORE UPDATE trigger keeps `updated_at` at NOW(), so
    // staleness is exercised through the lease parameter instead of by
    // backdating the row.
    set_status(&pool, c, "published").await;

    let within_lease = repo.claim_pending(10, 60).await.unwrap();
    assert!(
        within_lease.is_empty(),
        "freshly claimed rows must not be reclaimable within the lease"
    );

    // A -2h lease puts the cutoff 2 hours ahead of the database clock: every
    // `publishing` row is past it, so the reclaim branch must return them.
    let reclaimed = repo.claim_pending(10, -7200).await.unwrap();
    assert_eq!(reclaimed.len(), 10);
    assert!(reclaimed
        .iter()
        .all(|m| m.status == MessageStatus::Publishing));
}
