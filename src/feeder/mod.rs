use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    config::RuntimeMode,
    gateway::{FeedOutcome, FeedRequestStatus, GatewayClient},
    ledger::{LedgerStore, StoredFeedAttemptStatus},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedWorkerStep {
    Idle,
    NotDispatched {
        attempt_id: Uuid,
    },
    ShadowBlocked {
        feeds_due: u64,
    },
    OverrideBlocked {
        feeds_due: u64,
    },
    RemoteDisabled {
        feeds_due: u64,
    },
    SafetyUnavailable {
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
    gateway: &GatewayClient,
    threshold_sats: u64,
    mode: RuntimeMode,
) -> Result<FeedWorkerStep> {
    if threshold_sats == 0 {
        bail!("feeder threshold must be greater than zero");
    }

    if let Some(attempt) = ledger.unresolved_feed_attempt().await? {
        // GET only. A 404, timeout, malformed result or receipt is never proof
        // of non-dispatch, including when the original POST is still in flight.
        let status = match gateway.feed_request_status(attempt.id).await {
            Ok(status) => status,
            Err(_) => {
                return Ok(FeedWorkerStep::UnknownFeedBlocked {
                    attempt_id: attempt.id,
                });
            }
        };
        return apply_outcome(ledger, attempt.id, attempt.status, status).await;
    }

    let credit = ledger.feed_credit_sats().await?;
    let feeds_due = credit / threshold_sats;
    if feeds_due == 0 {
        return Ok(FeedWorkerStep::Idle);
    }

    if !mode.feeder_enabled() {
        return Ok(FeedWorkerStep::ShadowBlocked { feeds_due });
    }

    let safety = match gateway.feeder_safety().await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "unable to determine trusted feeder safety state; automatic feeding remains blocked");
            return Ok(FeedWorkerStep::SafetyUnavailable { feeds_due });
        }
    };
    if safety.override_enabled {
        return Ok(FeedWorkerStep::OverrideBlocked { feeds_due });
    }
    if !safety.remote_enabled {
        return Ok(FeedWorkerStep::RemoteDisabled { feeds_due });
    }

    let Some(attempt_id) = ledger.begin_feed_attempt(threshold_sats).await? else {
        return Ok(FeedWorkerStep::Idle);
    };

    let status = match gateway.request_feed(attempt_id).await {
        Ok(status) => status,
        Err(error) => {
            ledger
                .mark_feed_unknown(attempt_id, "gateway response unavailable or uncorrelated")
                .await?;
            return Err(error).context("gateway feed outcome remains unresolved");
        }
    };
    apply_outcome(
        ledger,
        attempt_id,
        StoredFeedAttemptStatus::IntentCommitted,
        status,
    )
    .await
}

async fn apply_outcome(
    ledger: &LedgerStore,
    attempt_id: Uuid,
    stored: StoredFeedAttemptStatus,
    status: FeedRequestStatus,
) -> Result<FeedWorkerStep> {
    status.validate(attempt_id)?;
    match status.status {
        FeedOutcome::Confirmed => {
            match stored {
                StoredFeedAttemptStatus::IntentCommitted => {
                    ledger.confirm_feed_attempt(attempt_id).await?
                }
                StoredFeedAttemptStatus::Unknown => {
                    ledger.reconcile_unknown_as_fed(attempt_id).await?
                }
            }
            Ok(FeedWorkerStep::Fed {
                attempt_id,
                remaining_sats: ledger.feed_credit_sats().await?,
            })
        }
        FeedOutcome::NotDispatched => {
            ledger
                .resolve_feed_not_dispatched(
                    attempt_id,
                    status
                        .refusal
                        .context("missing refusal")?
                        .retry_after_seconds,
                )
                .await?;
            Ok(FeedWorkerStep::NotDispatched { attempt_id })
        }
        FeedOutcome::Pending | FeedOutcome::Ambiguous => {
            if stored == StoredFeedAttemptStatus::IntentCommitted {
                ledger
                    .mark_feed_unknown(
                        attempt_id,
                        "gateway physical outcome unresolved; status polling only",
                    )
                    .await?;
            }
            Ok(FeedWorkerStep::UnknownFeedBlocked { attempt_id })
        }
    }
}

pub async fn run_feed_worker(
    ledger: LedgerStore,
    gateway: GatewayClient,
    threshold_sats: u64,
    inter_feed_delay: Duration,
    mode: RuntimeMode,
) -> Result<()> {
    loop {
        match run_feed_step(&ledger, &gateway, threshold_sats, mode).await {
            Ok(FeedWorkerStep::Fed {
                attempt_id,
                remaining_sats,
            }) => {
                tracing::info!(%attempt_id, remaining_sats, "automatic feeder activation confirmed through trusted gateway");
                sleep(inter_feed_delay).await;
            }
            Ok(FeedWorkerStep::UnknownFeedBlocked { attempt_id }) => {
                tracing::error!(%attempt_id, "automatic feeding blocked by unresolved ambiguous feed");
                sleep(Duration::from_secs(5)).await;
            }
            Ok(FeedWorkerStep::SafetyUnavailable { feeds_due }) => {
                tracing::warn!(
                    feeds_due,
                    "automatic feeding blocked because trusted gateway safety state is unavailable"
                );
                sleep(Duration::from_secs(5)).await;
            }
            Ok(FeedWorkerStep::OverrideBlocked { feeds_due }) => {
                tracing::info!(feeds_due, "automatic feeding blocked by FeederOverride");
                sleep(Duration::from_secs(2)).await;
            }
            Ok(FeedWorkerStep::RemoteDisabled { feeds_due }) => {
                tracing::info!(
                    feeds_due,
                    "automatic feeding blocked because LightningGoatsRemoteEnabled is OFF"
                );
                sleep(Duration::from_secs(2)).await;
            }
            Ok(FeedWorkerStep::ShadowBlocked { feeds_due }) => {
                tracing::debug!(
                    feeds_due,
                    "shadow mode: feed would be due but actuation is disabled"
                );
                sleep(Duration::from_secs(2)).await;
            }
            Ok(FeedWorkerStep::NotDispatched { .. }) => sleep(Duration::from_secs(2)).await,
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
    };
    use tempfile::TempDir;
    use tokio::net::TcpListener;

    use super::*;
    use crate::{
        domain::payment::SettledPayment,
        gateway::FeederSafety,
        ledger::{SettlementOutcome, StoredFeedAttemptStatus},
    };

    #[derive(Clone)]
    struct MockGatewayState {
        safety: FeederSafety,
        posts: Arc<AtomicUsize>,
        fail_post: Arc<AtomicBool>,
    }

    async fn safety_handler(State(state): State<MockGatewayState>) -> Json<FeederSafety> {
        Json(state.safety)
    }

    async fn feed_handler(
        State(state): State<MockGatewayState>,
        Path(id): Path<Uuid>,
    ) -> impl IntoResponse {
        state.posts.fetch_add(1, Ordering::SeqCst);
        if state.fail_post.load(Ordering::SeqCst) {
            StatusCode::GATEWAY_TIMEOUT.into_response()
        } else {
            Json(FeedRequestStatus {
                request_id: id,
                status: FeedOutcome::Confirmed,
                refusal: None,
            })
            .into_response()
        }
    }

    async fn gateway(state: MockGatewayState) -> GatewayClient {
        let app = Router::new()
            .route("/v1/feeder/override", get(safety_handler))
            .route("/v1/feeder/request/{id}", post(feed_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        GatewayClient::new(&format!("http://{address}/")).unwrap()
    }

    async fn credited_store(sats: u64) -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        let payment = SettledPayment {
            source: "test".to_owned(),
            source_id: format!("worker-{sats}"),
            payment_hash: Some(format!("{sats:064x}")),
            address_user: "herd".to_owned(),
            credit_pool: "herd".to_owned(),
            amount_msat: sats * 1_000,
            settled_at: Some(1_700_000_000),
            context_json: None,
        };
        assert!(matches!(
            store.record_payment(&payment).await.unwrap(),
            SettlementOutcome::Credited { .. }
        ));
        (directory, store)
    }

    fn open_safety() -> FeederSafety {
        FeederSafety {
            override_enabled: false,
            remote_enabled: true,
        }
    }

    #[tokio::test]
    async fn drains_two_thresholds_and_leaves_340() {
        let (_directory, ledger) = credited_store(2_340).await;
        let posts = Arc::new(AtomicUsize::new(0));
        let gateway = gateway(MockGatewayState {
            safety: open_safety(),
            posts: Arc::clone(&posts),
            fail_post: Arc::new(AtomicBool::new(false)),
        })
        .await;

        assert!(matches!(
            run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::Fed {
                remaining_sats: 1_340,
                ..
            }
        ));
        assert!(matches!(
            run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::Fed {
                remaining_sats: 340,
                ..
            }
        ));
        assert_eq!(
            run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::Idle
        );
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 340);
    }

    #[tokio::test]
    async fn ambiguous_gateway_failure_blocks_retry() {
        let (_directory, ledger) = credited_store(1_000).await;
        let posts = Arc::new(AtomicUsize::new(0));
        let gateway = gateway(MockGatewayState {
            safety: open_safety(),
            posts: Arc::clone(&posts),
            fail_post: Arc::new(AtomicBool::new(true)),
        })
        .await;

        assert!(
            run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Active)
                .await
                .is_err()
        );
        let unresolved = ledger.unresolved_feed_attempt().await.unwrap().unwrap();
        assert_eq!(unresolved.status, StoredFeedAttemptStatus::Unknown);
        assert_eq!(posts.load(Ordering::SeqCst), 1);

        assert!(matches!(
            run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            FeedWorkerStep::UnknownFeedBlocked { .. }
        ));
        assert_eq!(posts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn safety_controls_block_before_intent() {
        for (safety, expected) in [
            (
                FeederSafety {
                    override_enabled: true,
                    remote_enabled: true,
                },
                "override",
            ),
            (
                FeederSafety {
                    override_enabled: false,
                    remote_enabled: false,
                },
                "remote",
            ),
        ] {
            let (_directory, ledger) = credited_store(1_000).await;
            let gateway = gateway(MockGatewayState {
                safety,
                posts: Arc::new(AtomicUsize::new(0)),
                fail_post: Arc::new(AtomicBool::new(false)),
            })
            .await;
            let step = run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Active)
                .await
                .unwrap();
            match expected {
                "override" => assert!(matches!(step, FeedWorkerStep::OverrideBlocked { .. })),
                "remote" => assert!(matches!(step, FeedWorkerStep::RemoteDisabled { .. })),
                _ => unreachable!(),
            }
            assert!(ledger.unresolved_feed_attempt().await.unwrap().is_none());
        }
    }
}
