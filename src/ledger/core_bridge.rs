//! Export proof from correlated private rows. Event names and JSON alone are
//! never evidence of money or food, including historical/unlinked records.
use anyhow::{Context, Result, bail};
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::{LedgerStore, to_i64, to_u64};
use crate::core_bridge::{
    Batch, CoreEvent, EventBody, Feeding, MAX_BATCH_BYTES, MAX_PAGE, Payment, SettledZap, VERSION,
};

impl LedgerStore {
    pub async fn update_solar_context(&self, context: &crate::gateway::SolarContext) -> Result<()> {
        context.validate()?;
        sqlx::query("INSERT INTO solar_presentation(singleton,document_json) VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET document_json=excluded.document_json")
            .bind(serde_json::to_string(context)?).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn lifecycle_context(
        &self,
        mode: crate::config::RuntimeMode,
    ) -> Result<crate::core_bridge::LifecycleContext> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT override_enabled,observed_at FROM feeding_presentation WHERE singleton=1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let solar: Option<String> =
            sqlx::query_scalar("SELECT document_json FROM solar_presentation WHERE singleton=1")
                .fetch_optional(&mut *tx)
                .await?;
        let state = crate::core_bridge::LifecycleContext {
            override_enabled: row
                .as_ref()
                .map(|r| r.try_get::<Option<bool>, _>("override_enabled"))
                .transpose()?
                .flatten(),
            observed_at: row.as_ref().map(|r| r.try_get("observed_at")).transpose()?,
            solar: solar.map(|r| serde_json::from_str(&r)).transpose()?,
            nostr_enabled: mode.nostr_enabled(),
        };
        tx.commit().await?;
        Ok(state)
    }

    pub async fn core_safety(
        &self,
        mode: crate::config::RuntimeMode,
    ) -> Result<crate::core_bridge::Safety> {
        use crate::core_bridge::{Availability, Safety};
        let now = crate::cyberherd::calendar::now()?;
        if !mode.feeder_enabled() {
            return Ok(Safety {
                availability: Availability::Paused,
                observed_at: Some(now),
                nostr_enabled: mode.nostr_enabled(),
            });
        }
        let row = sqlx::query(
            "SELECT paused,reason,observed_at FROM feeding_presentation WHERE singleton=1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(Safety {
                availability: Availability::Unknown,
                observed_at: None,
                nostr_enabled: mode.nostr_enabled(),
            });
        };
        let observed_at: i64 = row.try_get("observed_at")?;
        let paused: bool = row.try_get("paused")?;
        let reason: String = row.try_get("reason")?;
        let availability = match (paused, reason.as_str()) {
            (false, "ready") => Availability::Ready,
            (true, "override" | "remote_disabled" | "mode_disabled") => Availability::Paused,
            _ => Availability::Unknown,
        };
        let mut state = Safety {
            availability,
            observed_at: Some(observed_at),
            nostr_enabled: mode.nostr_enabled(),
        };
        state.availability = state.current_availability(now);
        Ok(state)
    }

    pub async fn core_event_batch(
        &self,
        expected: Option<Uuid>,
        after: u64,
        limit: u16,
    ) -> Result<Batch> {
        if !(1..=MAX_PAGE).contains(&limit) {
            bail!("invalid core event page size");
        }
        let after_i64 = to_i64(after, "core event cursor")?;
        // Ensure the existing durable epoch exists, then read it and events in
        // the same snapshot so a restore/reset cannot race the reported identity.
        self.overlay_stream_id().await?;
        let mut tx = self.pool.begin().await?;
        let epoch: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        let epoch = Uuid::parse_str(&epoch)?;
        if expected.is_some_and(|id| id != epoch) {
            bail!("core ledger epoch changed; explicit operator recovery required");
        }
        let latest = to_u64(
            sqlx::query_scalar("SELECT COALESCE(MAX(seq),0) FROM event_log")
                .fetch_one(&mut *tx)
                .await?,
            "latest core event",
        )?;
        if after > latest {
            bail!("core cursor is ahead of retained history");
        }
        let rows=sqlx::query("SELECT seq,event_type,created_at,CASE WHEN length(CAST(payload_json AS BLOB))<=32768 THEN payload_json ELSE NULL END AS payload_json FROM event_log WHERE seq>? ORDER BY seq LIMIT ?")
            .bind(after_i64).bind(i64::from(limit)).fetch_all(&mut *tx).await?;
        let mut batch = Batch {
            version: VERSION,
            epoch,
            after,
            through: after,
            latest,
            events: Vec::new(),
        };
        let mut bytes = 0usize;
        for row in rows {
            let seq = to_u64(row.try_get("seq")?, "core event sequence")?;
            if seq != batch.through + 1 {
                bail!("core event history has a gap; explicit recovery required");
            }
            let event_type: String = row.try_get("event_type")?;
            let observed_at: i64 = row.try_get("created_at")?;
            let payload: Option<String> = row.try_get("payload_json")?;
            let body = export_body(&mut tx, seq, &event_type, payload.as_deref()).await?;
            let event = CoreEvent {
                seq,
                observed_at,
                body,
            };
            let size = serde_json::to_vec(&event)?.len();
            if bytes.saturating_add(size) > MAX_BATCH_BYTES {
                if batch.events.is_empty() {
                    bail!("single core event exceeds export budget");
                }
                break;
            }
            bytes += size;
            batch.through = seq;
            batch.events.push(event);
        }
        batch.validate(expected, after)?;
        tx.commit().await?;
        Ok(batch)
    }
}

fn payload(raw: Option<&str>) -> Result<Value> {
    let value: Value = serde_json::from_str(raw.context("financial event payload exceeds bound")?)?;
    if !value.is_object() {
        bail!("financial event payload is not an object");
    }
    Ok(value)
}

async fn export_body(
    tx: &mut Transaction<'_, Sqlite>,
    seq: u64,
    event_type: &str,
    raw: Option<&str>,
) -> Result<EventBody> {
    let seq = to_i64(seq, "core event sequence")?;
    let settlement:Option<i64>=sqlx::query_scalar("SELECT r.btc_settlement_id FROM credit_grants g JOIN asset_receipts r ON r.id=g.receipt_id WHERE g.event_seq=?")
        .bind(seq).fetch_optional(&mut **tx).await?.flatten();
    if let Some(id) = settlement {
        if event_type != "payment_received" {
            bail!("settlement event type contradicts source");
        }
        super::credit::verify_native_btc(tx, id).await?;
        let row=sqlx::query("SELECT p.*,l.payment_hash AS ledger_hash,l.payment_source AS ledger_source,l.payment_source_id AS ledger_source_id,l.address_user AS ledger_user,l.credit_pool AS ledger_pool,g.event_seq AS bound_event FROM settled_payments p JOIN asset_receipts r ON r.btc_settlement_id=p.id JOIN credit_grants g ON g.receipt_id=r.id JOIN ledger_entries l ON l.id=g.ledger_entry_id WHERE p.id=?")
            .bind(id).fetch_one(&mut **tx).await?;
        let source: String = row.try_get("source")?;
        let source_id: String = row.try_get("source_id")?;
        let address_user: String = row.try_get("address_user")?;
        let credit_pool: String = row.try_get("credit_pool")?;
        let payment_hash: Option<String> = row.try_get("payment_hash")?;
        let amount_msat = to_u64(row.try_get("amount_msat")?, "settled msat")?;
        let value = payload(raw)?;
        if row.try_get::<i64, _>("bound_event")? != seq
            || row.try_get::<Option<String>, _>("ledger_hash")? != payment_hash
            || row.try_get::<Option<String>, _>("ledger_source")?.as_ref() != Some(&source)
            || row
                .try_get::<Option<String>, _>("ledger_source_id")?
                .as_ref()
                != Some(&source_id)
            || row.try_get::<Option<String>, _>("ledger_user")?.as_ref() != Some(&address_user)
            || row.try_get::<Option<String>, _>("ledger_pool")?.as_ref() != Some(&credit_pool)
            || value["source"] != source
            || value["source_id"] != source_id
            || value["address_user"] != address_user
            || value["credit_pool"] != credit_pool
            || value["amount_sats"].as_u64() != Some(amount_msat / 1000)
            || value["payment_hash"] != serde_json::to_value(&payment_hash)?
        {
            bail!("settlement export provenance is inconsistent");
        }
        let mut paid_at = row
            .try_get::<Option<i64>, _>("settled_at")?
            .unwrap_or(row.try_get("received_at")?);
        let zap=sqlx::query("SELECT j.paid_at,z.request_json,z.payee_pubkey,z.receipt_pubkey,r.invoice,r.description_hash,r.amount_msat,r.address_user,r.credit_pool FROM zap_receipt_jobs j JOIN zap_receive_context z USING(receive_request_id) JOIN strike_receive_requests r USING(receive_request_id) WHERE j.settlement_id=? AND j.source_event_seq=?")
            .bind(id).bind(seq).fetch_optional(&mut **tx).await?;
        let zap = if let Some(zap) = zap {
            if source != "strike"
                || zap.try_get::<i64, _>("amount_msat")? != to_i64(amount_msat, "msat")?
                || zap.try_get::<String, _>("address_user")? != address_user
                || zap.try_get::<String, _>("credit_pool")? != credit_pool
            {
                bail!("settled zap contradicts actual received amount/address");
            }
            paid_at = zap.try_get("paid_at")?;
            Some(SettledZap {
                request_json: zap.try_get("request_json")?,
                payee_pubkey: zap.try_get("payee_pubkey")?,
                receipt_pubkey: zap.try_get("receipt_pubkey")?,
                invoice: zap.try_get("invoice")?,
                description_hash: zap.try_get("description_hash")?,
            })
        } else {
            None
        };
        return Ok(EventBody::Payment(Box::new(Payment {
            settlement_id: id,
            source,
            source_id,
            payment_hash: payment_hash.map(|hash| hash.to_ascii_lowercase()),
            address_user,
            credit_pool,
            amount_msat,
            paid_at,
            zap,
        })));
    }
    if let Some(row)=sqlx::query("SELECT f.id,f.status,f.threshold_sats,f.resolved_at,l.entry_type,l.source_key,l.delta_sats FROM feed_event_sources s JOIN feed_attempts f ON f.id=s.feed_attempt_id JOIN ledger_entries l ON l.feed_attempt_id=f.id WHERE s.event_seq=?")
        .bind(seq).fetch_optional(&mut **tx).await? {
        let id:String=row.try_get("id")?;
        let threshold:i64=row.try_get("threshold_sats")?;
        let value=payload(raw)?;
        if event_type!="feeder_confirmed" || row.try_get::<String,_>("status")?!="confirmed"
            || row.try_get::<String,_>("entry_type")?!="FEED_DEBIT" || row.try_get::<String,_>("source_key")?!=format!("feed:{id}")
            || row.try_get::<i64,_>("delta_sats")? != -threshold || value["feed_attempt_id"]!=id
            || value["threshold_sats"].as_u64()!=Some(to_u64(threshold,"feed threshold")?) {
            bail!("feed export does not match a confirmed UUID and debit");
        }
        let request_id=Uuid::parse_str(&id)?;
        let funding=super::reward_funding::confirmed_funding(tx,request_id).await?;
        return Ok(EventBody::Feeding(Feeding {request_id,threshold_sats:to_u64(threshold,"feed threshold")?,confirmed_at:row.try_get::<Option<i64>,_>("resolved_at")?.context("confirmed feed lacks completion time")?,funding}));
    }
    if let Some(row)=sqlx::query("SELECT g.*,l.entry_type,l.source_key,l.delta_sats,l.address_user AS ledger_user,l.credit_pool AS ledger_pool FROM operator_credit_grants g JOIN ledger_entries l ON l.id=g.ledger_entry_id WHERE g.event_seq=?")
        .bind(seq).fetch_optional(&mut **tx).await? {
        let amount:i64=row.try_get("amount_sats")?;
        let address_user:String=row.try_get("address_user")?;
        let credit_pool:String=row.try_get("credit_pool")?;
        let value=payload(raw)?;
        if event_type!="operator_credit_added" || row.try_get::<String,_>("entry_type")?!="OPERATOR_CREDIT"
            || row.try_get::<String,_>("source_key")?!=format!("operator-credit:{}",row.try_get::<String,_>("grant_id")?)
            || row.try_get::<i64,_>("delta_sats")?!=amount || row.try_get::<String,_>("ledger_user")?!=address_user
            || row.try_get::<String,_>("ledger_pool")?!=credit_pool || value["address_user"]!=address_user
            || value["credit_pool"]!=credit_pool || value["amount_sats"].as_u64()!=Some(to_u64(amount,"virtual credit")?)
            || value["source"]!="operator_credit" || value["grant_id"]!=row.try_get::<String,_>("grant_id")? {
            bail!("operator credit export provenance is inconsistent");
        }
        return Ok(EventBody::OperatorCredit {amount_sats:to_u64(amount,"virtual credit")?,address_user,credit_pool});
    }
    // Historical events, diagnostics and uncorrelated forged names carry no
    // money/feeding authority. Unsupported asset events are also not BTC funds.
    Ok(EventBody::Other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::payment::SettledPayment,
        ledger::{OperatorCreditGrant, StoredStrikeReceiveRequest},
        nostr_event::test_event,
        zap::ZapRequest,
    };
    use serde_json::json;
    use std::time::Duration;

    async fn store() -> (tempfile::TempDir, String, LedgerStore) {
        let root = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("core.db").display());
        let store = LedgerStore::connect(&url).await.unwrap();
        (root, url, store)
    }
    fn payment(id: &str, msat: u64) -> SettledPayment {
        SettledPayment {
            source: "strike".into(),
            source_id: id.into(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: msat,
            settled_at: None,
            context_json: None,
        }
    }
    async fn native(store: &LedgerStore, marker: u8, content: &str, actual: u64) -> (String, i64) {
        let payee = test_event(1, 1, 100, "", vec![]).pubkey;
        let signer = test_event(2, 1, 100, "", vec![]).pubkey;
        let event = test_event(
            3,
            9734,
            100,
            content,
            vec![
                vec!["p".into(), payee.clone()],
                vec!["amount".into(), "1000000".into()],
                vec!["relays".into(), "wss://relay.example.com/".into()],
            ],
        );
        let raw = serde_json::to_string_pretty(&event).unwrap();
        let zap = ZapRequest::validate(&raw, &payee, &signer, 1_000_000).unwrap();
        let now = crate::cyberherd::calendar::now().unwrap();
        let invoice = crate::test_invoices::invoice_with_payment_hash(
            1_000_000,
            &zap.description_hash(),
            Duration::from_secs(now as u64),
            [marker; 32],
        );
        let id = Uuid::new_v4();
        store
            .record_strike_receive_request_with_zap(
                &StoredStrikeReceiveRequest {
                    receive_request_id: id,
                    address_user: "herd".into(),
                    credit_pool: "herd".into(),
                    amount_msat: 1_000_000,
                    description_hash: zap.description_hash(),
                    payment_hash: hex::encode([marker; 32]),
                    invoice,
                    created_provider: None,
                },
                Some(&zap),
            )
            .await
            .unwrap();
        let mut settled = payment(&format!("native-{marker}"), actual);
        settled.context_json = Some(json!({"receive_request_id":id.to_string()}).to_string());
        settled.settled_at = Some(now);
        store.record_payment(&settled).await.unwrap();
        (raw, now)
    }

    #[tokio::test]
    async fn actual_sources_export_but_copied_event_names_do_not_create_authority() {
        let (_root, url, store) = store().await;
        let mut paid = payment("ordinary", 1_000_000);
        paid.payment_hash = Some("AB".repeat(32));
        store.record_payment(&paid).await.unwrap();
        store
            .grant_operator_credit(
                &OperatorCreditGrant {
                    id: "virtual".into(),
                    amount_sats: 305,
                    address_user: "herd".into(),
                    reason: "Synthetic bridge fixture".into(),
                },
                false,
            )
            .await
            .unwrap();
        let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
        store.confirm_feed_attempt(id).await.unwrap();
        let existing = store.events_after(0, 3).await.unwrap();
        store
            .append_event(
                "payment_received",
                &serde_json::from_str::<Value>(&existing[0].payload_json).unwrap(),
            )
            .await
            .unwrap();
        store
            .append_event(
                "feeder_confirmed",
                &serde_json::from_str::<Value>(&existing[2].payload_json).unwrap(),
            )
            .await
            .unwrap();
        let batch = store.core_event_batch(None, 0, 128).await.unwrap();
        assert_eq!(batch.events.len(), 5);
        let EventBody::Payment(payment) = &batch.events[0].body else {
            panic!("missing actual settlement proof")
        };
        assert_eq!(payment.payment_hash, Some("ab".repeat(32)));
        assert!(matches!(
            batch.events[1].body,
            EventBody::OperatorCredit {
                amount_sats: 305,
                ..
            }
        ));
        let EventBody::Feeding(feed) = &batch.events[2].body else {
            panic!("missing confirmed feed proof")
        };
        assert_eq!(feed.request_id, id);
        assert!(matches!(batch.events[3].body, EventBody::Other));
        assert!(matches!(batch.events[4].body, EventBody::Other));
        assert_eq!(store.feed_credit_sats().await.unwrap(), 305);
        let epoch = batch.epoch;
        drop(store);
        let store = LedgerStore::connect(&url).await.unwrap();
        assert_eq!(
            store.core_event_batch(Some(epoch), 0, 128).await.unwrap(),
            batch
        );
        assert!(
            store
                .core_event_batch(Some(Uuid::new_v4()), 0, 128)
                .await
                .is_err()
        );
        assert!(store.core_event_batch(Some(epoch), 6, 128).await.is_err());
        store.reset_overlay_stream().await.unwrap();
        assert!(store.core_event_batch(Some(epoch), 0, 128).await.is_err());
    }

    #[tokio::test]
    async fn feed_binding_is_atomic_and_immutable_and_unconfirmed_work_has_no_proof() {
        let (_root, _url, store) = store().await;
        store
            .record_payment(&payment("funded", 1_000_000))
            .await
            .unwrap();
        let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
        let batch = store.core_event_batch(None, 0, 128).await.unwrap();
        assert!(
            !batch
                .events
                .iter()
                .any(|event| matches!(event.body, EventBody::Feeding(_)))
        );
        sqlx::query("CREATE TRIGGER fail_feed_binding BEFORE INSERT ON feed_event_sources BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").execute(&store.pool).await.unwrap();
        assert!(store.confirm_feed_attempt(id).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(
            store.unresolved_feed_attempt().await.unwrap().unwrap().id,
            id
        );
        assert_eq!(store.latest_event_seq().await.unwrap(), 1);
        sqlx::query("DROP TRIGGER fail_feed_binding")
            .execute(&store.pool)
            .await
            .unwrap();
        store.confirm_feed_attempt(id).await.unwrap();
        for sql in [
            "DELETE FROM feed_event_sources",
            "UPDATE feed_event_sources SET event_seq=1",
        ] {
            assert!(sqlx::query(sql).execute(&store.pool).await.is_err());
        }
        let batch = store.core_event_batch(None, 1, 128).await.unwrap();
        assert!(matches!(batch.events[0].body, EventBody::Feeding(_)));
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn corrupt_source_payload_and_history_gaps_fail_closed() {
        let (_root, _url, store) = store().await;
        store
            .record_payment(&payment("original", 10_000))
            .await
            .unwrap();
        sqlx::query("UPDATE event_log SET payload_json=json_set(payload_json,'$.amount_sats',999) WHERE seq=1").execute(&store.pool).await.unwrap();
        assert!(store.core_event_batch(None, 0, 128).await.is_err());
        sqlx::query("UPDATE event_log SET payload_json=json_set(payload_json,'$.amount_sats',10) WHERE seq=1").execute(&store.pool).await.unwrap();
        assert!(store.core_event_batch(None, 0, 128).await.is_ok());
        store
            .append_event("interface_info", &json!({}))
            .await
            .unwrap();
        store
            .append_event("interface_info", &json!({}))
            .await
            .unwrap();
        sqlx::query("DELETE FROM event_log WHERE seq=2")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(store.core_event_batch(None, 1, 128).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 10);
    }

    #[tokio::test]
    async fn settled_zap_exports_exact_raw_bytes_and_settlement_time_without_signer_health() {
        let (_root, _url, store) = store().await;
        let (raw, paid) = native(&store, 4, "hello 🐐", 1_000_000).await;
        let batch = store.core_event_batch(None, 0, 128).await.unwrap();
        let EventBody::Payment(payment) = &batch.events[0].body else {
            panic!("missing payment")
        };
        assert_eq!(payment.paid_at, paid);
        assert_ne!(payment.paid_at, 100);
        assert_eq!(payment.zap.as_ref().unwrap().request_json, raw);
        let roundtrip: Batch =
            serde_json::from_str(&serde_json::to_string(&batch).unwrap()).unwrap();
        roundtrip.validate(Some(batch.epoch), 0).unwrap();
        assert_eq!(roundtrip, batch);
        assert!(store.next_outbox_entry().await.unwrap().is_none());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn mismatched_p2p_amount_is_real_credit_without_qualified_zap_authority() {
        let (_root, _url, store) = store().await;
        native(&store, 5, "", 900_000).await;
        let batch = store.core_event_batch(None, 0, 128).await.unwrap();
        let EventBody::Payment(payment) = &batch.events[0].body else {
            panic!("missing real credit")
        };
        assert_eq!(payment.amount_msat, 900_000);
        assert!(payment.zap.is_none());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 900);
    }

    #[tokio::test]
    async fn byte_bounded_pages_keep_contiguous_cursor_and_do_not_skip_large_contexts() {
        let (_root, _url, store) = store().await;
        for marker in 10..42 {
            native(&store, marker, &"x".repeat(6900), 1_000_000).await;
        }
        let first = store.core_event_batch(None, 0, 128).await.unwrap();
        assert!(first.through < 32);
        assert!(first.through > 0);
        let next = store
            .core_event_batch(Some(first.epoch), first.through, 128)
            .await
            .unwrap();
        assert_eq!(next.through, 32);
        assert_eq!(first.events.len() + next.events.len(), 32);
        assert!(serde_json::to_vec(&first).unwrap().len() < crate::core_bridge::MAX_FRAME);
        assert!(store.core_event_batch(None, 0, 0).await.is_err());
    }

    #[tokio::test]
    async fn safety_cache_expires_to_unknown_and_shadow_never_reports_ready() {
        use crate::{config::RuntimeMode, core_bridge::Availability, gateway::FeederSafety};
        let (_root, _url, store) = store().await;
        assert_eq!(
            store
                .core_safety(RuntimeMode::Active)
                .await
                .unwrap()
                .availability,
            Availability::Unknown
        );
        store
            .update_feeding_presentation(
                1000,
                RuntimeMode::Active,
                Some(FeederSafety {
                    override_enabled: false,
                    remote_enabled: true,
                }),
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .core_safety(RuntimeMode::Active)
                .await
                .unwrap()
                .availability,
            Availability::Ready
        );
        assert_eq!(
            store
                .core_safety(RuntimeMode::Shadow)
                .await
                .unwrap()
                .availability,
            Availability::Paused
        );
        sqlx::query("UPDATE feeding_presentation SET observed_at=unixepoch()-6")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            store
                .core_safety(RuntimeMode::Active)
                .await
                .unwrap()
                .availability,
            Availability::Unknown
        );
    }
}
