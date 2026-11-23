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

pub(super) async fn retain_claim(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    digest: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO breez_collected_claims(request_id,proof_digest) VALUES (?,?) ON CONFLICT(request_id) DO NOTHING")
        .bind(id).bind(digest).execute(&mut **tx).await?;
    let original: String =
        sqlx::query_scalar("SELECT proof_digest FROM breez_collected_claims WHERE request_id=?")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
    ensure!(original == digest, "conflicting project proof evidence");
    Ok(())
}

impl LedgerStore {
    /// Issue or recover the original project invoice for a reserved foreign-mint sweep.
    pub async fn issue_breez_treasury_invoice(
        &self,
        collector: &BreezCollectorConfig,
        registration: &crate::cashu::receipt_authority::ReceiptPolicy,
        policy: &crate::cashu::treasury::Policy,
        id: uuid::Uuid,
    ) -> Result<lightning_goats_cashu_receipt::treasury::Issued> {
        use lightning_goats_cashu_receipt::treasury::{Issued, hex_digest};
        collector.validate()?;
        let intent = self
            .cashu_treasury_intent(id)
            .await?
            .context("unknown treasury intent")?;
        policy.check_new(&intent)?;
        ensure!(
            intent.core_epoch == registration.core_epoch
                && intent.wallet_epoch == registration.wallet_epoch
                && intent.wallet_instance == registration.wallet_instance
                && intent.destination.origin == crate::cashu::treasury::PROJECT_MINT_ORIGIN,
            "project treasury destination or registration changed"
        );
        let digest = hex_digest(intent.digest()?);
        let terms = json!({"purpose":"treasury_conversion","amount_sat":intent.deposit_sats,
                            "description_hash":digest})
        .to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT INTO breez_project_invoices(id,intent) VALUES (?,?) ON CONFLICT(id) DO NOTHING",
        )
        .bind(id.to_string())
        .bind(&terms)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query("SELECT intent,invoice FROM breez_project_invoices WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        ensure!(
            row.try_get::<String, _>("intent")? == terms,
            "treasury invoice identity conflict"
        );
        tx.commit().await?;
        let existing: Option<String> = row.try_get("invoice")?;
        let invoice = if let Some(invoice) = existing {
            invoice
        } else {
            #[derive(Deserialize)]
            struct Reply {
                invoice: String,
            }
            let request = json!({"treasury_id":id,"amount_sat":intent.deposit_sats,"description_hash":digest});
            let reply: Reply = crate::private_ipc::request(
                &collector.socket,
                collector.peer_uid,
                &request,
                65536,
                20,
            )
            .await?;
            reply.invoice
        };
        let parsed: lightning_invoice::Bolt11Invoice = invoice.parse()?;
        let payment_hash = parsed.payment_hash().to_string();
        let signed = crate::bolt11::verify(
            &invoice,
            intent
                .deposit_sats
                .checked_mul(1000)
                .context("treasury amount overflow")?,
            &payment_hash,
            &digest,
        )?;
        crate::bolt11::verify_issuance_expiry(&signed, intent.invoice_expiry_seconds)?;
        self.bind_breez_invoice(id, &invoice, &payment_hash).await?;
        Ok(Issued {
            intent,
            receive_request_id: id,
            payment_hash,
            invoice,
        })
    }

    /// Reconcile one due project invoice against its original collector claim.
    pub async fn poll_breez_claim(&self, collector: &BreezCollectorConfig) -> Result<bool> {
        let row = sqlx::query(
            "UPDATE breez_project_invoices SET next_poll=unixepoch()+30
             WHERE id=(SELECT i.id FROM breez_project_invoices i
                WHERE (i.invoice IS NOT NULL OR json_type(i.intent,'$.purpose') IS NULL OR
                  (json_extract(i.intent,'$.purpose')='contact_fee' AND i.created_at>unixepoch()-900))
                  AND (json_type(i.intent,'$.purpose') IS NULL
                       OR json_extract(i.intent,'$.purpose') IN ('contact_fee','treasury_conversion'))
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
            if !contact {
                #[derive(Deserialize)]
                struct OriginalInvoice {
                    request_id: String,
                    invoice: String,
                    payment_hash: String,
                }
                let reply: Option<OriginalInvoice> = crate::private_ipc::request(
                    &collector.socket,
                    collector.peer_uid,
                    &json!({"feeder_invoice_lookup":id}),
                    65536,
                    20,
                )
                .await?;
                if let Some(reply) = reply {
                    use sha2::{Digest, Sha256};
                    ensure!(
                        reply.request_id == id,
                        "collector returned a different original intent"
                    );
                    let description = terms["description"]
                        .as_str()
                        .context("missing invoice commitment")?;
                    let msat = terms["amount_sat"]
                        .as_u64()
                        .and_then(|n| n.checked_mul(1000))
                        .context("invalid invoice amount")?;
                    let signed = crate::bolt11::verify(
                        &reply.invoice,
                        msat,
                        &reply.payment_hash,
                        &hex::encode(Sha256::digest(description.as_bytes())),
                    )?;
                    crate::bolt11::verify_expiry_terms(
                        &signed,
                        crate::config::HOME_INVOICE_EXPIRY_SECONDS,
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?,
                    )?;
                    self.bind_breez_invoice(id.parse()?, &reply.invoice, &reply.payment_hash)
                        .await?;
                } else {
                    sqlx::query("UPDATE breez_project_invoices SET next_poll=unixepoch()+300 WHERE id=? AND invoice IS NULL")
                        .bind(&id).execute(&self.pool).await?;
                }
                return Ok(true);
            }
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
        let conversion = terms["purpose"] == "treasury_conversion";
        let lookup = if conversion {
            json!({"treasury_claim_status":id})
        } else if contact {
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
            if conversion {
                let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
                let valid: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM breez_project_invoices b
                     JOIN cashu_treasury_intents t ON t.id=b.id
                     WHERE b.id=? AND b.payment_hash=? AND b.invoice IS NOT NULL
                       AND json_extract(b.intent,'$.purpose')='treasury_conversion'
                       AND json_extract(b.intent,'$.description_hash')=t.intent_digest
                       AND json_extract(b.intent,'$.amount_sat')=t.deposit_sats AND t.deposit_sats=?
                       AND NOT EXISTS(SELECT 1 FROM settled_payments WHERE payment_hash=b.payment_hash COLLATE NOCASE))")
                    .bind(&id).bind(&hash).bind(i64::try_from(status.amount_sat)?).fetch_one(&mut *tx).await?;
                ensure!(
                    valid,
                    "conversion claim differs from original treasury intent"
                );
                retain_claim(&mut tx, &id, &digest).await?;
                tx.commit().await?;
                return Ok(true);
            }
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
                super::SettlementEvidence::MintClaim {
                    invoice_id: status.request_id.parse()?,
                    digest: &digest,
                },
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
        let claim_lookup = json!({"contact_claim_status":intent.id});
        let claim_reply = json!({"request_id":intent.id, "amount_sat":100,
            "payment_hash":"ad".repeat(32), "proof_digest":"cd".repeat(32)});
        let listener = tokio::net::UnixListener::bind(&collector.socket).unwrap();
        let server = tokio::spawn(async move {
            for (expected, reply) in [(expected, reply), (claim_lookup, claim_reply)] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let req: Value = crate::private_ipc::read(&mut stream, 65536).await.unwrap();
                assert_eq!(req, expected);
                crate::private_ipc::write(&mut stream, &reply, 65536)
                    .await
                    .unwrap();
            }
        });
        sqlx::query("UPDATE breez_project_invoices SET next_poll=0")
            .execute(&ledger.pool)
            .await
            .unwrap();
        assert!(ledger.poll_breez_claim(&collector).await.unwrap());
        let status = ledger
            .contact_status(intent.id, &capability)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.state, "issued");
        assert_eq!(status.invoice, Some(invoice));
        assert!(ledger.reserve_contact_delivery().await.unwrap().is_none());
        sqlx::query("UPDATE breez_project_invoices SET next_poll=0")
            .execute(&ledger.pool)
            .await
            .unwrap();
        assert!(ledger.poll_breez_claim(&collector).await.unwrap());
        server.await.unwrap();
        let delivery = ledger.reserve_contact_delivery().await.unwrap().unwrap();
        assert_eq!(delivery.contact_id, intent.id);
        assert_eq!(delivery.envelope_json, intent.envelope_json);
        ledger
            .finish_contact_delivery(&delivery, Some(&delivery.relays[0]))
            .await
            .unwrap();
        assert_eq!(
            ledger
                .contact_status(intent.id, &capability)
                .await
                .unwrap()
                .unwrap()
                .state,
            "delivered"
        );
        assert!(ledger.reserve_contact_delivery().await.unwrap().is_none());
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
        let description = "dexter fixture metadata";
        let terms = json!({"amount_sat":10,"user":"dexter","credit_pool":"herd","zap":false,"description":description});
        ledger.prepare_breez_invoice(id, &terms).await.unwrap();
        // The collector issued the invoice and the original IPC reply was lost.
        // Restart after expiry. The only permissible recovery operation is lookup.
        drop(ledger);
        let ledger = LedgerStore::connect(&url).await.unwrap();
        let invoice = crate::test_invoices::invoice_with_expiry(
            10_000,
            &hex::encode(<sha2::Sha256 as sha2::Digest>::digest(
                description.as_bytes(),
            )),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                - Duration::from_secs(1200),
            [0xac; 32],
            Duration::from_secs(900),
        );
        let socket = directory.path().join("collector.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let collector = BreezCollectorConfig {
            socket,
            peer_uid: crate::private_ipc::uid().unwrap(),
        };
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let lookup: Value = crate::private_ipc::read(&mut stream, 65536).await.unwrap();
            assert_eq!(lookup, json!({"feeder_invoice_lookup":id}));
            crate::private_ipc::write(
                &mut stream,
                &json!({"request_id":id,"invoice":invoice,"payment_hash":hash}),
                65536,
            )
            .await
            .unwrap();
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
        verify_cashu_reward_recovery(&reopened, &url, directory.path(), feed).await;
    }

    async fn verify_cashu_reward_recovery(
        ledger: &LedgerStore,
        url: &str,
        directory: &std::path::Path,
        feed: Uuid,
    ) {
        use crate::payout::{
            cashu::ExecutionRequest,
            outcome::{Disposition, Header, Packet, Row, next_hash},
        };
        use crate::reward_funding::{RewardFundingSource, RewardPolicy};
        use sha2::{Digest, Sha256};
        let key = |index| crate::nostr_event::test_event(index, 1, 100, "", vec![]).pubkey;
        let policy = RewardPolicy {
            owner: key(1),
            funding_source: RewardFundingSource::RetainedCashu,
            member_percent_ceiling: 50,
            max_sats_per_feed: 5,
            max_sats_per_24h: 10,
        };
        let epoch = ledger.overlay_stream_id().await.unwrap();
        let reservation = ledger
            .reserve_reward(&policy, epoch, feed, &"aa".repeat(32), 5)
            .await
            .unwrap();
        let preimage = [7u8; 32];
        let hash: [u8; 32] = Sha256::digest(preimage).into();
        let description_hash = "ab".repeat(32);
        let invoice = crate::test_invoices::invoice_with_payment_hash(
            3000,
            &description_hash,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap(),
            hash,
        );
        let claim = ExecutionRequest {
            reservation: reservation.clone(),
            member: key(2),
            disposition_id: Uuid::new_v4(),
            invoice,
            description_hash,
            payment_hash: hex::encode(hash),
            gross_sats: 5,
            amount_sat: 3,
            maximum_fee_sat: 2,
            nip57: false,
            collector_payload_digest: "ef".repeat(32),
        };
        ledger
            .claim_cashu_execution(&claim, &policy, true, false)
            .await
            .unwrap();
        let row = Row {
            member: claim.member.clone(),
            gross_sats: 5,
            disposition: Disposition::CashuPaid {
                disposition_id: claim.disposition_id,
                payment_hash: claim.payment_hash.clone(),
                amount_sats: 3,
                fee_sats: 1,
            },
        };
        let mut header = Header {
            scope: Header::cashu_scope(&reservation),
            reservation,
            rows: 1,
            final_hash: "00".repeat(32),
        };
        let before_hash = header.seed().unwrap();
        header.final_hash = next_hash(&before_hash, &row).unwrap();
        let packet = Packet {
            header,
            after: 0,
            before_hash,
            row,
        };
        let socket = directory.join("payout-receipt.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let collector = BreezCollectorConfig {
            socket,
            peer_uid: crate::private_ipc::uid().unwrap(),
        };
        let serve = tokio::spawn(async move {
            for paid in [false, true] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request: Value = crate::private_ipc::read(&mut stream, 65536).await.unwrap();
                assert_eq!(
                    request,
                    json!({"payout_receipt":claim.disposition_id,"feed_id":feed,"payment_hash":claim.payment_hash,"invoice_digest":hex::encode(Sha256::digest(claim.invoice.as_bytes()))})
                );
                let result = paid.then(|| json!({"state":"PAID","quote":"fixture","amount_sat":3,"fee_sat":1,"payment_hash":hex::encode(hash),"payment_preimage":hex::encode(preimage)}));
                crate::private_ipc::write(&mut stream, &result, 65536)
                    .await
                    .unwrap();
            }
        });
        assert!(
            ledger
                .reconcile_cashu_outcome(&packet, &collector)
                .await
                .is_err()
        );
        let pending = ledger
            .payout_outcome_progress(&packet.header)
            .await
            .unwrap();
        assert_eq!((pending.through, pending.released_sats), (0, 0));
        let settled = ledger
            .reconcile_cashu_outcome(&packet, &collector)
            .await
            .unwrap();
        serve.await.unwrap();
        assert!(settled.finalized);
        assert_eq!(
            (settled.spent_sats, settled.held_sats, settled.released_sats),
            (4, 0, 1)
        );
        let restarted = LedgerStore::connect(url).await.unwrap();
        assert_eq!(
            restarted
                .reconcile_cashu_outcome(&packet, &collector)
                .await
                .unwrap(),
            settled
        );
        assert_eq!(restarted.feed_credit_sats().await.unwrap(), 0);
    }
}

#[cfg(test)]
#[path = "direct_operator_fixture.rs"]
mod direct_operator_fixture;
