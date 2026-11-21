//! Reconcile claimed HOME project quotes against core-owned invoice terms.
use super::LedgerStore;
use crate::{config::BreezCollectorConfig, domain::payment::SettledPayment};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::time::Duration;

#[derive(Deserialize)]
struct ClaimStatus {
    request_id: String,
    payment_hash: String,
    amount_sat: u64,
    proof_digest: Option<String>,
}

impl LedgerStore {
    async fn poll_breez_claim(&self, collector: &BreezCollectorConfig) -> Result<bool> {
        let row = sqlx::query(
            "UPDATE breez_project_invoices SET next_poll=unixepoch()+30
             WHERE id=(SELECT i.id FROM breez_project_invoices i
                WHERE (i.invoice IS NOT NULL OR
                  (json_extract(i.intent,'$.purpose')='contact_fee' AND i.created_at>unixepoch()-900))
                  AND i.next_poll<=unixepoch()
                  AND NOT EXISTS(SELECT 1 FROM breez_collected_claims c WHERE c.request_id=i.id)
                  AND NOT EXISTS(SELECT 1 FROM settled_payments s WHERE s.source='breez' AND s.source_id=i.id)
                ORDER BY i.next_poll,i.created_at,i.id LIMIT 1)
             RETURNING id,payment_hash,intent",
        ).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(false);
        };
        let id: String = row.try_get("id")?;
        let hash: Option<String> = row.try_get("payment_hash")?;
        let terms: Value = serde_json::from_str(&row.try_get::<String, _>("intent")?)?;
        let contact = terms["purpose"] == "contact_fee";
        let Some(hash) = hash else {
            ensure!(contact, "missing feeder invoice");
            self.issue_reserved_contact(
                collector,
                id.parse()?,
                terms["description_hash"]
                    .as_str()
                    .context("missing contact description hash")?,
            )
            .await?;
            return Ok(true);
        };
        let lookup = if contact {
            json!({"contact_claim_status":id})
        } else {
            json!({"claim_status":id})
        };
        let status: ClaimStatus =
            crate::private_ipc::request(&collector.socket, collector.peer_uid, &lookup, 65536, 20)
                .await?;
        ensure!(
            status.request_id == id
                && status.payment_hash == hash
                && terms["amount_sat"].as_u64() == Some(status.amount_sat),
            "project claim differs from issued invoice"
        );
        if let Some(digest) = status.proof_digest {
            if contact {
                self.accept_breez_contact_claim(&id, &hash, &digest).await?;
                return Ok(true);
            }
            self.record_verified_payment(
                &SettledPayment {
                    source: "breez".into(),
                    source_id: id.clone(),
                    payment_hash: Some(hash),
                    address_user: terms["user"]
                        .as_str()
                        .context("missing project user")?
                        .into(),
                    credit_pool: terms["credit_pool"]
                        .as_str()
                        .context("missing project pool")?
                        .into(),
                    amount_msat: status
                        .amount_sat
                        .checked_mul(1000)
                        .context("invalid project amount")?,
                    settled_at: None,
                    context_json: Some(json!({"receive_request_id":id}).to_string()),
                },
                Some(&digest),
            )
            .await?;
        }
        Ok(true)
    }

    pub async fn run_breez_claim_recovery(&self, collector: BreezCollectorConfig) -> Result<()> {
        loop {
            match self.poll_breez_claim(&collector).await {
                Ok(true) => continue,
                Err(_) => tracing::warn!("Breez claim reconciliation deferred"),
                Ok(false) => {}
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn interrupted_contact_creation_recovers_the_same_invoice_for_status_polling() {
        let dir = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", dir.path().join("core.db").display());
        let ledger = LedgerStore::connect(&url).await.unwrap();
        let intent = crate::contact::tests::fixture();
        let collector = BreezCollectorConfig {
            socket: dir.path().join("collector.sock"),
            peer_uid: crate::private_ipc::uid().unwrap(),
        };
        let capability = "ab".repeat(32);
        assert!(
            ledger
                .create_breez_contact_invoice(&collector, &intent, &capability)
                .await
                .is_err()
        );
        drop(ledger);
        let ledger = LedgerStore::connect(&url).await.unwrap();
        let invoice = crate::test_invoices::invoice_with_expiry(
            crate::contact::FEE_MSAT,
            &intent.description_hash,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap(),
            [0xad; 32],
            Duration::from_secs(900),
        );
        let reply = json!({"invoice":invoice});
        let expected = json!({"contact_id":intent.id,"description_hash":intent.description_hash});
        let listener = tokio::net::UnixListener::bind(&collector.socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req: Value = crate::private_ipc::read(&mut stream, 65536).await.unwrap();
            assert_eq!(req, expected);
            crate::private_ipc::write(&mut stream, &reply, 65536)
                .await
                .unwrap();
        });
        sqlx::query("UPDATE breez_project_invoices SET next_poll=0")
            .execute(&ledger.pool)
            .await
            .unwrap();
        assert!(ledger.poll_breez_claim(&collector).await.unwrap());
        server.await.unwrap();
        let status = ledger
            .contact_status(intent.id, &capability)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.state, "issued");
        assert_eq!(status.invoice, Some(invoice));
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn paid_claim_replay_after_restart_credits_the_named_goat_once() {
        let directory = tempfile::TempDir::new().unwrap();
        let url = format!(
            "sqlite://{}",
            directory.path().join("core.sqlite").display()
        );
        let ledger = LedgerStore::connect(&url).await.unwrap();
        ledger.initialize_reward_funding().await.unwrap();
        let id = Uuid::new_v4();
        let hash = "ac".repeat(32);
        let terms = json!({"amount_sat":10,"user":"dexter","credit_pool":"herd","zap":false});
        ledger.prepare_breez_invoice(id, &terms).await.unwrap();
        ledger
            .bind_breez_invoice(id, "fixture", &hash)
            .await
            .unwrap();
        let socket = directory.path().join("collector.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let collector = BreezCollectorConfig {
            socket,
            peer_uid: crate::private_ipc::uid().unwrap(),
        };
        let server = tokio::spawn(async move {
            for collected in [false, true] {
                let (mut connection, _) = listener.accept().await.unwrap();
                let request: Value = crate::private_ipc::read(&mut connection, 65536)
                    .await
                    .unwrap();
                assert_eq!(request, json!({"claim_status":id}));
                crate::private_ipc::write(&mut connection,&json!({"request_id":id,"payment_hash":hash,"amount_sat":10,"proof_digest":collected.then(|| "de".repeat(32))}),65536).await.unwrap();
            }
        });
        assert!(ledger.poll_breez_claim(&collector).await.unwrap());
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
        sqlx::query("UPDATE breez_project_invoices SET next_poll=0")
            .execute(&ledger.pool)
            .await
            .unwrap();
        assert!(ledger.poll_breez_claim(&collector).await.unwrap());
        server.await.unwrap();
        let reopened = LedgerStore::connect(&url).await.unwrap();
        sqlx::query("UPDATE breez_project_invoices SET next_poll=0")
            .execute(&reopened.pool)
            .await
            .unwrap();
        assert!(!reopened.poll_breez_claim(&collector).await.unwrap());
        assert_eq!(reopened.feed_credit_sats().await.unwrap(), 10);
        let user: String =
            sqlx::query_scalar("SELECT address_user FROM settled_payments WHERE source='breez'")
                .fetch_one(&reopened.pool)
                .await
                .unwrap();
        assert_eq!(user, "dexter");
        let feed = reopened.begin_feed_attempt(10).await.unwrap().unwrap();
        reopened.confirm_feed_attempt(feed).await.unwrap();
        let mut tx = reopened.pool.begin().await.unwrap();
        let funding = crate::ledger::inspect_confirmed_reward_funding(&mut tx, feed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!((funding.strike_sats, funding.retained_cashu_sats), (0, 10));
        tx.rollback().await.unwrap();
    }
}
