use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    config::RuntimeMode,
    ledger::{LedgerStore, StoredFeedAttemptStatus},
    openhab::OpenHabClient,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedWorkerStep {
    Idle,
    ShadowBlocked {
        feeds_due: u64,
    },
    OverrideBlocked {
        feeds_due: u64,
    },
    OverrideUnavailable {
        feeds_due: u64,
    },
    UnknownFeedBlocked {
        attempt_id: Uuid,
    },
    Fed {
        attempt_id: Uuid,
        remaining_sats: u64,
    },
}

pub async fn run_feed_step(
    ledger: &LedgerStore,
    openhab: &OpenHabClient,
    threshold_sats: u64,
    mode: RuntimeMode,
) -> Result<FeedWorkerStep> {
    if threshold_sats == 0 {
        bail!("feeder threshold must be greater than zero");
    }

    if let Some(attempt) = ledger.unresolved_feed_attempt().await? {
        return match attempt.status {
            StoredFeedAttemptStatus::Unknown => Ok(FeedWorkerStep::UnknownFeedBlocked {
                attempt_id: attempt.id,
            }),
            StoredFeedAttemptStatus::IntentCommitted => bail!(
                "unreconciled intent_committed feed attempt {} reached the feed worker",
                attempt.id
            ),
        };
    }

    let credit = ledger.feed_credit_sats().await?;
    let feeds_due = credit / threshold_sats;
    if feeds_due == 0 {
        return Ok(FeedWorkerStep::Idle);
    }

    if mode == RuntimeMode::Shadow {
        return Ok(FeedWorkerStep::ShadowBlocked { feeds_due });
    }

    let override_enabled = match openhab.feeder_override_enabled().await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "unable to determine FeederOverride; automatic feeding remains blocked");
            return Ok(FeedWorkerStep::OverrideUnavailable { feeds_due });
        }
    };
    if override_enabled {
        return Ok(FeedWorkerStep::OverrideBlocked { feeds_due });
    }

    let Some(attempt_id) = ledger.begin_feed_attempt(threshold_sats).await? else {
        return Ok(FeedWorkerStep::Idle);
    };

    if let Err(error) = openhab.trigger_feeder().await {
        ledger
            .mark_feed_unknown(attempt_id, &error.to_string())
            .await
            .context("failed marking ambiguous OpenHAB feed as unknown")?;
        return Err(error).context(format!(
            "OpenHAB feed attempt {attempt_id} is ambiguous and requires operator reconciliation"
        ));
    }

    ledger
        .confirm_feed_attempt(attempt_id)
        .await
        .context("OpenHAB accepted feed but durable feed confirmation failed")?;
    let remaining_sats = ledger.feed_credit_sats().await?;

    Ok(FeedWorkerStep::Fed {
        attempt_id,
        remaining_sats,
    })
}

pub async fn run_feed_worker(
    ledger: LedgerStore,
    openhab: OpenHabClient,
    threshold_sats: u64,
    inter_feed_delay: Duration,
    mode: RuntimeMode,
) -> Result<()> {
    loop {
        match run_feed_step(&ledger, &openhab, threshold_sats, mode).await {
            Ok(FeedWorkerStep::Fed {
                attempt_id,
                remaining_sats,
            }) => {
                tracing::info!(%attempt_id, remaining_sats, "automatic feeder activation confirmed");
                sleep(inter_feed_delay).await;
            }
            Ok(FeedWorkerStep::UnknownFeedBlocked { attempt_id }) => {
                tracing::error!(%attempt_id, "automatic feeding blocked by unresolved ambiguous feed");
                sleep(Duration::from_secs(5)).await;
            }
            Ok(FeedWorkerStep::OverrideUnavailable { feeds_due }) => {
                tracing::warn!(
                    feeds_due,
                    "automatic feeding blocked because FeederOverride state is unavailable"
                );
                sleep(Duration::from_secs(5)).await;
            }
            Ok(FeedWorkerStep::OverrideBlocked { feeds_due }) => {
                tracing::info!(feeds_due, "automatic feeding blocked by FeederOverride");
                sleep(Duration::from_secs(2)).await;
            }
            Ok(FeedWorkerStep::ShadowBlocked { feeds_due }) => {
                tracing::debug!(
                    feeds_due,
                    "shadow mode: feed would be due but actuation is disabled"
                );
                sleep(Duration::from_secs(2)).await;
            }
            Ok(FeedWorkerStep::Idle) => sleep(Duration::from_secs(2)).await,
            Err(error) => {
                tracing::error!(%error, "feed worker step failed");
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Router,
        extract::State,
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
    };
    use tempfile::TempDir;
    use tokio::net::TcpListener;

    use super::*;
    use crate::ledger::{PaidInvoice, SettlementOutcome};

    #[derive(Clone)]
    struct MockOpenHabState {
        override_state: &'static str,
        trigger_status: StatusCode,
        triggers: Arc<AtomicUsize>,
    }

    async fn override_handler(State(state): State<MockOpenHabState>) -> impl IntoResponse {
        (StatusCode::OK, state.override_state)
    }

    async fn trigger_handler(State(state): State<MockOpenHabState>) -> impl IntoResponse {
        state.triggers.fetch_add(1, Ordering::SeqCst);
        state.trigger_status
    }

    async fn openhab(state: MockOpenHabState) -> OpenHabClient {
        let app = Router::new()
            .route("/rest/items/FeederOverride/state", get(override_handler))
            .route("/rest/rules/rule123/runnow", post(trigger_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        OpenHabClient::new(
            &format!("http://{address}/"),
            "token".to_owned(),
            "rule123",
            "FeederOverride",
        )
        .unwrap()
    }

    async fn credited_store(sats: u64) -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        store.initialize_cursor(100).await.unwrap();
        let invoice = PaidInvoice {
            pay_index: 101,
            payment_hash: "worker-test-hash".to_owned(),
            label: Some("clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000".to_owned()),
            amount_msat: sats * 1_000,
            settled_at: Some(1_700_000_000),
        };
        assert!(matches!(
            store.record_settlement(&invoice, "herd").await.unwrap(),
            SettlementOutcome::Credited { .. }
        ));
        (directory, store)
    }

    #[tokio::test]
    async fn drains_two_thresholds_and_leaves_340() {
        let (_directory, ledger) = credited_store(2_340).await;
        let triggers = Arc::new(AtomicUsize::new(0));
        let openhab = openhab(MockOpenHabState {
            override_state: "OFF",
            trigger_status: StatusCode::OK,
            triggers: Arc::clone(&triggers),
        })
        .await;

        assert!(matches!(
            run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::Fed {
                remaining_sats: 1_340,
                ..
            }
        ));
        assert!(matches!(
            run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::Fed {
                remaining_sats: 340,
                ..
            }
        ));
        assert_eq!(
            run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::Idle
        );
        assert_eq!(triggers.load(Ordering::SeqCst), 2);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 340);
    }

    #[tokio::test]
    async fn override_blocks_all_due_feeds_without_debit() {
        let (_directory, ledger) = credited_store(2_340).await;
        let triggers = Arc::new(AtomicUsize::new(0));
        let openhab = openhab(MockOpenHabState {
            override_state: "ON",
            trigger_status: StatusCode::OK,
            triggers: Arc::clone(&triggers),
        })
        .await;

        assert_eq!(
            run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::OverrideBlocked { feeds_due: 2 }
        );
        assert_eq!(triggers.load(Ordering::SeqCst), 0);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 2_340);
    }

    #[tokio::test]
    async fn shadow_mode_never_touches_openhab_feeder() {
        let (_directory, ledger) = credited_store(2_340).await;
        let triggers = Arc::new(AtomicUsize::new(0));
        let openhab = openhab(MockOpenHabState {
            override_state: "OFF",
            trigger_status: StatusCode::OK,
            triggers: Arc::clone(&triggers),
        })
        .await;

        assert_eq!(
            run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Shadow)
                .await
                .unwrap(),
            FeedWorkerStep::ShadowBlocked { feeds_due: 2 }
        );
        assert_eq!(triggers.load(Ordering::SeqCst), 0);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 2_340);
    }

    #[tokio::test]
    async fn trigger_failure_becomes_unknown_and_stops_retry() {
        let (_directory, ledger) = credited_store(1_340).await;
        let triggers = Arc::new(AtomicUsize::new(0));
        let openhab = openhab(MockOpenHabState {
            override_state: "OFF",
            trigger_status: StatusCode::INTERNAL_SERVER_ERROR,
            triggers: Arc::clone(&triggers),
        })
        .await;

        assert!(
            run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Active)
                .await
                .is_err()
        );
        assert_eq!(triggers.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1_340);
        let unresolved = ledger.unresolved_feed_attempt().await.unwrap().unwrap();
        assert_eq!(unresolved.status, StoredFeedAttemptStatus::Unknown);

        assert!(matches!(
            run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::UnknownFeedBlocked { .. }
        ));
        assert_eq!(triggers.load(Ordering::SeqCst), 1);
    }
}
