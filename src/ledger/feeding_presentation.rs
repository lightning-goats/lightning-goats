use anyhow::{Result, bail};
use serde_json::{Value, json};
use sqlx::Row;

use super::{LedgerStore, feed::feed_credit_in_transaction, to_i64, to_u64};
use crate::{config::RuntimeMode, gateway::FeederSafety};

impl LedgerStore {
    /// Advisory message state. Never used to authorize dispatch or a debit.
    pub async fn update_feeding_presentation(
        &self,
        threshold_sats: u64,
        mode: RuntimeMode,
        safety: Option<FeederSafety>,
    ) -> Result<()> {
        if threshold_sats == 0 {
            bail!("feeding threshold must be positive");
        }
        let reason = if !mode.feeder_enabled() {
            "mode_disabled"
        } else {
            match safety {
                None => "unavailable",
                Some(s) if s.override_enabled => "override",
                Some(s) if !s.remote_enabled => "remote_disabled",
                Some(_) => "ready",
            }
        };
        sqlx::query("INSERT INTO feeding_presentation(singleton,paused,reason,threshold_sats,observed_at) VALUES(1,?,?,?,unixepoch()) ON CONFLICT(singleton) DO UPDATE SET paused=excluded.paused,reason=excluded.reason,threshold_sats=excluded.threshold_sats,observed_at=excluded.observed_at")
            .bind(reason!="ready").bind(reason).bind(to_i64(threshold_sats,"feeding threshold")?)
            .execute(&self.pool).await?;
        Ok(())
    }
}

pub(super) async fn snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    payload: &Value,
) -> Result<Option<Value>> {
    let Some(row) = sqlx::query("SELECT paused,reason,threshold_sats,observed_at,unixepoch() AS now FROM feeding_presentation WHERE singleton=1")
        .fetch_optional(&mut **tx).await? else { return Ok(None); };
    let threshold = to_u64(row.try_get("threshold_sats")?, "feeding threshold")?;
    if threshold == 0 {
        bail!("invalid cached feeding threshold");
    }
    let observed: i64 = row.try_get("observed_at")?;
    let now: i64 = row.try_get("now")?;
    let stale = now.saturating_sub(observed) > 15 || observed > now;
    let reason: String = if stale {
        "unavailable".into()
    } else {
        row.try_get("reason")?
    };
    let paused = stale || row.try_get::<bool, _>("paused")?;
    let credit = match payload.get("feed_credit_sats").and_then(Value::as_u64) {
        Some(credit) => credit,
        None => feed_credit_in_transaction(tx).await?,
    };
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM feed_attempts WHERE status IN ('intent_committed','unknown'))",
    )
    .fetch_one(&mut **tx)
    .await?;
    let queued = (credit / threshold).saturating_sub(u64::from(pending));
    Ok(Some(
        json!({"paused":paused,"reason":reason,"queued_feedings":queued,
        "awaiting_confirmation":pending,"observed_at":observed,"threshold_sats":threshold}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::payment::SettledPayment,
        ledger::{OperatorCreditGrant, OperatorCreditStatus},
        presentation::MessageRenderer,
    };
    use tempfile::TempDir;

    async fn store() -> (TempDir, String, LedgerStore) {
        let root = TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("ledger.db").display());
        let ledger = LedgerStore::connect(&url).await.unwrap();
        (root, url, ledger)
    }
    fn grant(id: &str, sats: u64) -> OperatorCreditGrant {
        OperatorCreditGrant {
            id: id.into(),
            amount_sats: sats,
            address_user: "herd".into(),
            reason: "Isolated paused-queue fixture".into(),
        }
    }
    fn safety(paused: bool) -> Option<FeederSafety> {
        Some(FeederSafety {
            override_enabled: paused,
            remote_enabled: true,
        })
    }

    #[tokio::test]
    async fn paused_credit_and_message_context_survive_restart_without_debits_or_re_render_drift() {
        let (_root, url, ledger) = store().await;
        ledger
            .update_feeding_presentation(1000, RuntimeMode::Canary, safety(true))
            .await
            .unwrap();
        let original = grant("queued", 2340);
        ledger
            .grant_operator_credit(&original, false)
            .await
            .unwrap();
        let event = ledger.events_after(0, 1).await.unwrap().remove(0);
        let payload: Value = serde_json::from_str(&event.payload_json).unwrap();
        assert_eq!(payload["feeding"]["paused"], true);
        assert_eq!(payload["feeding"]["queued_feedings"], 2);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 2340);
        let debits: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ledger_entries WHERE entry_type='FEED_DEBIT'")
                .fetch_one(&ledger.pool)
                .await
                .unwrap();
        assert_eq!(debits, 0);
        let renderer = MessageRenderer::embedded().unwrap();
        let rendered = renderer.render(&event, 1000).unwrap();
        let public = rendered.nostr_content.as_ref().unwrap();
        assert!(public.contains("Feeding is paused."));
        assert!(public.contains("2 feedings are queued"));
        let overlay = rendered.overlay_message.as_ref().unwrap();
        assert!(!overlay.contains("Feeding is paused."));
        assert!(!overlay.contains("Feeder ready!"));
        drop(ledger);
        let reopened = LedgerStore::connect(&url).await.unwrap();
        reopened
            .update_feeding_presentation(1000, RuntimeMode::Canary, safety(false))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .grant_operator_credit(&original, false)
                .await
                .unwrap()
                .status,
            OperatorCreditStatus::Duplicate
        );
        assert_eq!(renderer.render(&event, 1000).unwrap(), rendered);
        assert_eq!(reopened.feed_credit_sats().await.unwrap(), 2340);
    }

    #[tokio::test]
    async fn only_confirmed_inflight_feeding_debits_during_pause_and_is_not_counted_as_fresh_queue()
    {
        let (_root, _url, ledger) = store().await;
        ledger
            .update_feeding_presentation(1000, RuntimeMode::Canary, safety(false))
            .await
            .unwrap();
        ledger
            .grant_operator_credit(&grant("initial", 2340), false)
            .await
            .unwrap();
        let id = ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
        ledger
            .update_feeding_presentation(1000, RuntimeMode::Canary, safety(true))
            .await
            .unwrap();
        ledger
            .grant_operator_credit(&grant("while-pending", 10), false)
            .await
            .unwrap();
        let events = ledger.events_after(0, 10).await.unwrap();
        let pending: Value = serde_json::from_str(&events.last().unwrap().payload_json).unwrap();
        assert_eq!(pending["feeding"]["queued_feedings"], 1);
        assert_eq!(pending["feeding"]["awaiting_confirmation"], true);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 2350);
        ledger.confirm_feed_attempt(id).await.unwrap();
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1350);
        let events = ledger.events_after(0, 10).await.unwrap();
        let confirmed = events.last().unwrap();
        assert_eq!(confirmed.event_type, "feeder_confirmed");
        let data: Value = serde_json::from_str(&confirmed.payload_json).unwrap();
        assert_eq!(data["feeding"]["paused"], true);
        assert_eq!(data["feeding"]["queued_feedings"], 1);
        assert_eq!(data["feeding"]["awaiting_confirmation"], false);
        assert!(ledger.confirm_feed_attempt(id).await.is_err());
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1350);
    }

    #[tokio::test]
    async fn stale_or_future_cache_is_unknown_not_an_assertion_of_readiness() {
        let (_root, _url, ledger) = store().await;
        for (id, offset) in [("stale", -16), ("future", 30)] {
            ledger
                .update_feeding_presentation(1000, RuntimeMode::Canary, safety(false))
                .await
                .unwrap();
            sqlx::query("UPDATE feeding_presentation SET observed_at=unixepoch()+?")
                .bind(offset)
                .execute(&ledger.pool)
                .await
                .unwrap();
            ledger
                .grant_operator_credit(&grant(id, 1000), false)
                .await
                .unwrap();
            let events = ledger.events_after(0, 10).await.unwrap();
            let data: Value = serde_json::from_str(&events.last().unwrap().payload_json).unwrap();
            assert_eq!(data["feeding"]["paused"], true);
            assert_eq!(data["feeding"]["reason"], "unavailable");
        }
    }

    #[tokio::test]
    async fn failed_presentation_cache_does_not_roll_back_operator_or_provider_credit() {
        let (_root, _url, ledger) = store().await;
        sqlx::query("DROP TABLE feeding_presentation")
            .execute(&ledger.pool)
            .await
            .unwrap();
        ledger
            .grant_operator_credit(&grant("operator", 1000), false)
            .await
            .unwrap();
        ledger
            .record_payment(&SettledPayment {
                source: "strike".into(),
                source_id: "synthetic-private-fixture".into(),
                payment_hash: None,
                address_user: "herd".into(),
                credit_pool: "herd".into(),
                amount_msat: 10000,
                settled_at: None,
                context_json: None,
            })
            .await
            .unwrap();
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1010);
        assert_eq!(ledger.events_after(0, 10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn informational_and_weather_messages_continue_while_paused_and_stay_overlay_only() {
        let (_root, _url, ledger) = store().await;
        ledger
            .update_feeding_presentation(1000, RuntimeMode::Canary, safety(true))
            .await
            .unwrap();
        ledger
            .grant_operator_credit(&grant("queued", 2000), false)
            .await
            .unwrap();
        let config = crate::config::InformationalConfig {
            interval_seconds: 60,
            interface_info_enabled: true,
            weather_enabled: false,
            interface_info_probability: 1.0,
            weather_probability: 0.0,
        };
        let gateway = crate::gateway::GatewayClient::new("http://127.0.0.1:9/").unwrap();
        assert_eq!(
            crate::informational::run_informational_cycle(&ledger, &gateway, &config)
                .await
                .unwrap(),
            crate::informational::InformationalChoice::InterfaceInfo
        );
        ledger
            .append_event("weather_status", &json!({"message":"Current weather"}))
            .await
            .unwrap();
        let events = ledger.events_after(1, 10).await.unwrap();
        assert_eq!(events.len(), 2);
        let renderer = MessageRenderer::embedded().unwrap();
        for event in events {
            let rendered = renderer.render(&event, 1000).unwrap();
            assert!(rendered.nostr_content.is_none());
            let message = rendered.overlay_message.unwrap();
            assert!(!message.contains("Feeding is paused."));
            assert!(!message.contains("2 feedings are queued"));
        }
    }
}
