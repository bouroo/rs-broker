//! rs-broker-server - Main binary entry point

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod app;
mod grpc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rs_broker_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting rs-broker server");

    let app = app::AppBuilder::new().with_config()?.build().await?;

    app.run().await
}
