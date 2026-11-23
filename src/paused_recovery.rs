//! Read-only readiness during recovery, with payment and feeder processing off.
use anyhow::Result;
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use lightning_goats::{config::AppConfig, ledger::LedgerStore};
use serde::Serialize;

#[derive(Serialize)]
struct Status {
    mode: &'static str,
    feed_credit_sats: u64,
    unresolved_feed_attempt: Option<String>,
}

impl Status {
    async fn read(ledger: &LedgerStore) -> Result<Self> {
        Ok(Self {
            mode: "recovery-paused",
            feed_credit_sats: ledger.feed_credit_sats().await?,
            unresolved_feed_attempt: ledger
                .unresolved_feed_attempt()
                .await?
                .map(|attempt| attempt.id.to_string()),
        })
    }
}

async fn status(State(ledger): State<LedgerStore>) -> Result<Json<Status>, StatusCode> {
    Status::read(&ledger).await.map(Json).map_err(|error| {
        tracing::error!(%error, "paused recovery status unavailable");
        StatusCode::SERVICE_UNAVAILABLE
    })
}

pub(super) async fn serve(config: &AppConfig) -> Result<()> {
    let ledger = LedgerStore::connect_recovery_read_only(&config.database.url).await?;
    Status::read(&ledger).await?;
    let app = Router::new()
        .route("/healthz", get(super::healthz))
        .route("/api/v1/status", get(status))
        .with_state(ledger);

    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let listener = tokio::net::TcpListener::bind(config.service.listen).await?;
    tracing::info!(listen = %listener.local_addr()?, "paused recovery status listening");
    tokio::select! {
        result = lightning_goats::server::serve(listener, app) => result,
        _ = super::shutdown_signal() => Ok(()),
        _ = async {
            #[cfg(unix)]
            let _ = terminate.recv().await;
            #[cfg(not(unix))]
            std::future::pending::<()>().await;
        } => Ok(()),
    }
}
