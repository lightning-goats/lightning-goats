//! Source-bound internal treasury receives. Every durable boundary is retained;
//! neither a deposit nor a wallet result creates a second contribution.
use anyhow::{Context, Result, ensure};
use lightning_goats_cashu_receipt::treasury::{
    Intent, Issued, MeltData, PURPOSE, Source, hex_digest,
};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::super::{LedgerStore, StoredStrikeReceiveRequest, to_i64, to_u64};
use crate::cashu::{
    receipt_authority::ReceiptPolicy,
    treasury::{PeerMelt, Policy, TREASURY_USER},
};

/// Core worker input. Public receiving does not expose this operation.
pub struct TreasuryPlan {
    pub id: Uuid,
    pub mint_id: String,
    pub sources: Vec<(i64, u64)>,
    pub deposit_sats: u64,
    pub maximum_fee_sats: u64,
}

#[cfg(unix)]
#[derive(Clone, Copy, serde::Serialize)]
pub(crate) struct TreasurySettlementState {
    pub(crate) strike_deposit_retained: bool,
    pub(crate) wallet_report_retained: bool,
    pub(crate) settlement_complete: bool,
}

impl LedgerStore {
    #[cfg(test)]
    pub(crate) async fn test_cashu_fee_budget(&self, now: i64, limit: u64) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        daily_fee_remaining(&mut tx, now, limit).await
    }

    #[cfg(unix)]
    pub(crate) async fn treasury_settlement_state(
        &self,
        issued: &Issued,
        receive_id: Uuid,
    ) -> Result<TreasurySettlementState> {
        let mut tx = self.pool.begin().await?;
        require_epoch(&mut tx, issued.intent.core_epoch).await?;
        let id = issued.intent.id.to_string();
        let deposit: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT receive_id,payment_hash,deposit_sats FROM cashu_treasury_deposits WHERE intent_id=?",
        ).bind(&id).fetch_optional(&mut *tx).await?;
        if let Some((retained_id, hash, sats)) = &deposit {
            ensure!(
                *retained_id == receive_id.to_string()
                    && *hash == issued.payment_hash
                    && to_u64(*sats, "retained treasury deposit")? == issued.intent.deposit_sats,
                "approved receive differs from the retained treasury deposit"
            );
        }
        let report: Option<(String, String)> = sqlx::query_as(
            "SELECT result_digest,result_json FROM cashu_treasury_wallet_results WHERE intent_id=?",
        )
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((digest, json)) = &report {
            let data: MeltData = serde_json::from_str(json)?;
            data.validate(&issued.intent)?;
            ensure!(
                hex_digest(data.digest()?) == *digest,
                "retained wallet report digest differs"
            );
        }
        let complete: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM cashu_treasury_completions WHERE intent_id=?)",
        )
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
        let has_deposit = deposit.is_some();
        let has_report = report.is_some();
        ensure!(
            complete == (has_deposit && has_report),
            "retained treasury state is inconsistent"
        );
        tx.commit().await?;
        Ok(TreasurySettlementState {
            strike_deposit_retained: has_deposit,
            wallet_report_retained: has_report,
            settlement_complete: complete,
        })
    }

    pub async fn reserve_cashu_treasury(
        &self,
        registration: &ReceiptPolicy,
        policy: &Policy,
        plan: &TreasuryPlan,
    ) -> Result<Intent> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let intent = reserve_in_transaction(&mut tx, registration, policy, plan).await?;
        tx.commit().await?;
        Ok(intent)
    }

    pub async fn cashu_treasury_intent(&self, id: Uuid) -> Result<Option<Intent>> {
        let mut tx = self.pool.begin().await?;
        let result = load_intent(&mut tx, id).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// A claimed creation never expires into a second POST. A lost response
    /// requires lookup/adoption of the original provider resource.
    pub(crate) async fn claim_cashu_treasury_issuance(&self, id: Uuid) -> Result<Option<Uuid>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let intent = load_intent(&mut tx, id)
            .await?
            .context("unknown treasury intent")?;
        require_epoch(&mut tx, intent.core_epoch).await?;
        let claim = Uuid::new_v4();
        let changed=sqlx::query("INSERT INTO cashu_treasury_claims(intent_id,claim_id,claimed_at) VALUES(?,?,unixepoch()) ON CONFLICT(intent_id) DO NOTHING")
            .bind(id.to_string()).bind(claim.to_string()).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        Ok((changed == 1).then_some(claim))
    }

    pub(crate) async fn cashu_treasury_issuance_claim(&self, id: Uuid) -> Result<Option<Uuid>> {
        sqlx::query_scalar::<_, String>(
            "SELECT claim_id FROM cashu_treasury_claims WHERE intent_id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|id| Uuid::parse_str(&id).map_err(Into::into))
        .transpose()
    }

    pub(crate) async fn record_cashu_treasury_invoice(
        &self,
        id: Uuid,
        claim: Uuid,
        request: &StoredStrikeReceiveRequest,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let intent = load_intent(&mut tx, id)
            .await?
            .context("unknown treasury intent")?;
        require_epoch(&mut tx, intent.core_epoch).await?;
        let expected = hex_digest(intent.digest()?);
        let actual_claim: String =
            sqlx::query_scalar("SELECT claim_id FROM cashu_treasury_claims WHERE intent_id=?")
                .bind(id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        ensure!(
            actual_claim == claim.to_string()
                && request.address_user == TREASURY_USER
                && request.credit_pool == TREASURY_USER
                && request.description_hash == expected
                && request.amount_msat
                    == intent
                        .deposit_sats
                        .checked_mul(1000)
                        .context("treasury msat overflow")?,
            "treasury invoice differs from original purpose/claim"
        );
        crate::bolt11::verify(
            &request.invoice,
            request.amount_msat,
            &request.payment_hash,
            &request.description_hash,
        )?;
        let collision:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM settled_payments WHERE payment_hash=? COLLATE NOCASE) OR EXISTS(SELECT 1 FROM private_contact_intents WHERE receive_request_id=?)")
            .bind(&request.payment_hash).bind(request.receive_request_id.to_string()).fetch_one(&mut *tx).await?;
        ensure!(
            !collision,
            "treasury invoice conflicts with a contribution/contact purpose"
        );
        super::super::strike::record_in_transaction(&mut tx, request, None).await?;
        if let Some(row)=sqlx::query("SELECT receive_request_id,payment_hash FROM cashu_treasury_issuances WHERE intent_id=?")
            .bind(id.to_string()).fetch_optional(&mut *tx).await? {
            ensure!(row.try_get::<String,_>("receive_request_id")?==request.receive_request_id.to_string()
                && row.try_get::<String,_>("payment_hash")?==request.payment_hash,"treasury intent already has another invoice");
        } else {
            sqlx::query("INSERT INTO cashu_treasury_issuances(intent_id,receive_request_id,payment_hash) VALUES(?,?,?)")
                .bind(id.to_string()).bind(request.receive_request_id.to_string()).bind(&request.payment_hash).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn cashu_treasury_invoice(&self, id: Uuid) -> Result<Option<Issued>> {
        let intent = self
            .cashu_treasury_intent(id)
            .await?
            .context("unknown treasury intent")?;
        let mut epoch_tx = self.pool.begin().await?;
        require_epoch(&mut epoch_tx, intent.core_epoch).await?;
        epoch_tx.commit().await?;
        let request: Option<String> = sqlx::query_scalar(
            "SELECT receive_request_id FROM cashu_treasury_issuances WHERE intent_id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(request) = request else {
            return Ok(None);
        };
        let stored = self
            .strike_receive_request(Uuid::parse_str(&request)?)
            .await?
            .context("missing treasury invoice")?;
        ensure!(
            stored.description_hash == hex_digest(intent.digest()?)
                && stored.amount_msat == intent.deposit_sats * 1000
                && stored.address_user == TREASURY_USER
                && stored.credit_pool == TREASURY_USER,
            "treasury invoice binding changed"
        );
        let bound_hash: String = sqlx::query_scalar(
            "SELECT payment_hash FROM cashu_treasury_issuances WHERE intent_id=?",
        )
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await?;
        ensure!(
            bound_hash == stored.payment_hash,
            "treasury invoice hash binding changed"
        );
        crate::bolt11::verify(
            &stored.invoice,
            stored.amount_msat,
            &stored.payment_hash,
            &stored.description_hash,
        )?;
        Ok(Some(Issued {
            intent,
            receive_request_id: stored.receive_request_id,
            payment_hash: stored.payment_hash,
            invoice: stored.invoice,
        }))
    }

    pub(crate) async fn cashu_treasury_for_receive(&self, request: Uuid) -> Result<Option<Intent>> {
        let id: Option<String> = sqlx::query_scalar(
            "SELECT intent_id FROM cashu_treasury_issuances WHERE receive_request_id=?",
        )
        .bind(request.to_string())
        .fetch_optional(&self.pool)
        .await?;
        match id {
            Some(id) => self.cashu_treasury_intent(Uuid::parse_str(&id)?).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn record_cashu_treasury_deposit(
        &self,
        intent: &Intent,
        receive: &crate::strike::ReconciledStrikeReceive,
    ) -> Result<bool> {
        let issued = self
            .cashu_treasury_invoice(intent.id)
            .await?
            .context("treasury invoice is not bound")?;
        ensure!(
            issued.intent == *intent
                && receive.receive_request_id == issued.receive_request_id
                && receive.payment_hash.as_deref() == Some(issued.payment_hash.as_str())
                && receive.amount_msat == intent.deposit_sats * 1000,
            "treasury receive differs from frozen invoice"
        );
        let evidence = receive.treasury_evidence()?;
        let encoded = serde_json::to_string(&evidence)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_epoch(&mut tx, intent.core_epoch).await?;
        let collision:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM settled_payments WHERE (source='strike' AND source_id=?) OR payment_hash=? COLLATE NOCASE) OR EXISTS(SELECT 1 FROM private_contact_fees WHERE receive_id=?)")
            .bind(receive.receive_id.to_string()).bind(&issued.payment_hash).bind(receive.receive_id.to_string()).fetch_one(&mut *tx).await?;
        ensure!(
            !collision,
            "internal treasury transfer collides with contribution/contact settlement"
        );
        let previous=sqlx::query("SELECT receive_id,payment_hash,deposit_sats,evidence_json FROM cashu_treasury_deposits WHERE intent_id=?")
            .bind(intent.id.to_string()).fetch_optional(&mut *tx).await?;
        let fresh = if let Some(row) = previous {
            ensure!(
                row.try_get::<String, _>("receive_id")? == receive.receive_id.to_string()
                    && row.try_get::<String, _>("payment_hash")? == issued.payment_hash
                    && row.try_get::<i64, _>("deposit_sats")?
                        == to_i64(intent.deposit_sats, "verified treasury deposit")?
                    && row.try_get::<String, _>("evidence_json")? == encoded,
                "conflicting completed treasury receive"
            );
            false
        } else {
            sqlx::query("INSERT INTO cashu_treasury_deposits(intent_id,receive_id,payment_hash,deposit_sats,evidence_json,verified_at)
                         VALUES(?,?,?,?,?,MAX(unixepoch(),
                             COALESCE((SELECT last_seen FROM cashu_treasury_clock WHERE singleton=1),0),
                             COALESCE((SELECT MAX(verified_at) FROM cashu_treasury_deposits),0)))")
                .bind(intent.id.to_string()).bind(receive.receive_id.to_string()).bind(&issued.payment_hash)
                .bind(to_i64(intent.deposit_sats,"verified treasury deposit")?).bind(encoded).execute(&mut *tx).await?;
            true
        };
        complete(&mut tx, intent).await?;
        tx.commit().await?;
        Ok(fresh)
    }

    pub async fn record_cashu_melt_result(
        &self,
        registration: &ReceiptPolicy,
        report: &PeerMelt,
    ) -> Result<bool> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_epoch(&mut tx, registration.core_epoch).await?;
        let intent = load_intent(&mut tx, report.intent_id())
            .await?
            .context("unknown treasury intent")?;
        let data = report.validate(registration, &intent)?;
        let digest = hex_digest(data.digest()?);
        if intent.destination.origin == "https://mint.lightning-goats.com/" {
            let id = intent.id.to_string();
            let changed = sqlx::query(
                "INSERT INTO breez_treasury_wallet_results(intent_id,result_digest,result_json,observed_at)
                 SELECT i.id,?,?,MAX(unixepoch(),?,COALESCE((SELECT MAX(observed_at) FROM breez_treasury_wallet_results),0))
                 FROM breez_project_invoices i WHERE i.id=? AND i.invoice IS NOT NULL
                   AND json_extract(i.intent,'$.purpose')='treasury_conversion'
                   AND json_extract(i.intent,'$.description_hash')=? AND json_extract(i.intent,'$.amount_sat')=?
                 ON CONFLICT(intent_id) DO NOTHING")
                .bind(&digest).bind(serde_json::to_string(data)?).bind(report.fee_recorded_at().unwrap_or(0))
                .bind(&id).bind(&data.intent_digest).bind(to_i64(intent.deposit_sats,"conversion amount")?)
                .execute(&mut *tx).await?;
            if changed.rows_affected() == 0 {
                let row = sqlx::query("SELECT result_digest,observed_at FROM breez_treasury_wallet_results WHERE intent_id=?")
                    .bind(&id).fetch_one(&mut *tx).await?;
                ensure!(
                    row.try_get::<String, _>("result_digest")? == digest
                        && report.fee_recorded_at().unwrap_or(0)
                            <= row.try_get::<i64, _>("observed_at")?,
                    "conflicting conversion wallet result"
                );
            }
            let ready: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM breez_treasury_source_results WHERE intent_id=?)",
            )
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(ready);
        }
        if let Some(previous) = sqlx::query_scalar::<_, String>(
            "SELECT result_digest FROM cashu_treasury_wallet_results WHERE intent_id=?",
        )
        .bind(intent.id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        {
            ensure!(
                previous == digest,
                "treasury melt result conflicts with original evidence"
            );
            if let Some(fee_time) = report.fee_recorded_at() {
                let observed: i64 = sqlx::query_scalar(
                    "SELECT observed_at FROM cashu_treasury_fee_observations WHERE intent_id=?",
                )
                .bind(intent.id.to_string())
                .fetch_one(&mut *tx)
                .await?;
                ensure!(
                    fee_time <= observed,
                    "wallet fee time moved past the retained observation"
                );
            }
        } else {
            sqlx::query("INSERT INTO cashu_treasury_wallet_results(intent_id,result_digest,result_json) VALUES(?,?,?)")
                .bind(intent.id.to_string()).bind(digest).bind(serde_json::to_string(data)?).execute(&mut *tx).await?;
            sqlx::query(
                "INSERT INTO cashu_treasury_fee_observations(intent_id,observed_at)
                 VALUES(?,MAX(unixepoch(),?,
                     COALESCE((SELECT last_seen FROM cashu_treasury_clock WHERE singleton=1),0),
                     COALESCE((SELECT MAX(observed_at) FROM cashu_treasury_fee_observations),0)))",
            )
            .bind(intent.id.to_string())
            .bind(report.fee_recorded_at().unwrap_or(0))
            .execute(&mut *tx)
            .await?;
        }
        let ready = complete(&mut tx, &intent).await?;
        tx.commit().await?;
        Ok(ready)
    }
}

pub(super) async fn require_epoch(tx: &mut Transaction<'_, Sqlite>, expected: Uuid) -> Result<()> {
    let actual: String =
        sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
            .fetch_one(&mut **tx)
            .await?;
    ensure!(
        !expected.is_nil() && actual == expected.to_string(),
        "treasury core epoch changed; recovery required"
    );
    Ok(())
}
pub(super) async fn load_intent(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
) -> Result<Option<Intent>> {
    let row=sqlx::query("SELECT intent_json,intent_digest,mint_id,input_sats,deposit_sats FROM cashu_treasury_intents WHERE id=?")
        .bind(id.to_string()).fetch_optional(&mut **tx).await?;
    row.map(|row| -> Result<Intent> {
        let intent: Intent = serde_json::from_str(&row.try_get::<String, _>("intent_json")?)
            .map_err(|_| anyhow::anyhow!("invalid private treasury intent"))?;
        ensure!(
            intent.id == id
                && row.try_get::<String, _>("intent_digest")? == hex_digest(intent.digest()?)
                && row.try_get::<String, _>("mint_id")? == intent.mint_id
                && row.try_get::<i64, _>("input_sats")?
                    == to_i64(intent.input_sats, "treasury input")?
                && row.try_get::<i64, _>("deposit_sats")?
                    == to_i64(intent.deposit_sats, "treasury deposit")?,
            "treasury intent index changed"
        );
        Ok(intent)
    })
    .transpose()
}
pub(super) async fn reserve_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    registration: &ReceiptPolicy,
    policy: &Policy,
    plan: &TreasuryPlan,
) -> Result<Intent> {
    require_epoch(tx, registration.core_epoch).await?;
    let mut parts = plan.sources.clone();
    parts.sort_by_key(|p| p.0);
    ensure!(
        !parts.is_empty() && parts.len() <= lightning_goats_cashu_receipt::treasury::MAX_SOURCES,
        "invalid treasury source bound"
    );
    let mut sources = Vec::new();
    let mut input = 0u64;
    for (ledger, sats) in parts {
        let row = sqlx::query("SELECT * FROM cashu_receipt_inbox WHERE ledger_entry_id=?")
            .bind(ledger)
            .fetch_one(&mut **tx)
            .await?;
        ensure!(
            row.try_get::<String, _>("wallet_instance")?
                == registration.wallet_instance.to_string()
                && row.try_get::<String, _>("wallet_epoch")?
                    == registration.wallet_epoch.to_string()
                && row.try_get::<String, _>("mint_id")? == plan.mint_id,
            "treasury sources cross wallet/mint authority"
        );
        super::lot_identity(tx, ledger).await?;
        input = input.checked_add(sats).context("treasury input overflow")?;
        sources.push(Source {
            ledger_id: ledger,
            acquisition_id: Uuid::parse_str(&row.try_get::<String, _>("acquisition_id")?)?,
            receipt_digest: row.try_get("receipt_digest")?,
            input_sats: sats,
        });
    }
    let intent = Intent {
        version: 1,
        purpose: PURPOSE.into(),
        id: plan.id,
        core_epoch: registration.core_epoch,
        wallet_instance: registration.wallet_instance,
        wallet_epoch: registration.wallet_epoch,
        mint_id: plan.mint_id.clone(),
        treasury_policy: policy.version.clone(),
        destination: policy.destination.clone(),
        input_sats: input,
        deposit_sats: plan.deposit_sats,
        maximum_fee_sats: plan.maximum_fee_sats,
        invoice_expiry_seconds: policy.invoice_expiry_seconds,
        sources,
    };
    let digest = hex_digest(intent.digest()?);
    if let Some(previous) = load_intent(tx, plan.id).await? {
        ensure!(
            previous == intent,
            "treasury ID conflicts with frozen intent"
        );
        return Ok(previous);
    }
    policy.check_new(&intent)?;
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut **tx)
        .await?;
    let previous: Option<i64> =
        sqlx::query_scalar("SELECT last_seen FROM cashu_treasury_clock WHERE singleton=1")
            .fetch_optional(&mut **tx)
            .await?;
    ensure!(
        now >= 0 && previous.is_none_or(|last| now >= last),
        "treasury clock regressed"
    );
    let usage=sqlx::query("SELECT COUNT(*) AS count,COALESCE(SUM(input_sats),0) AS sats FROM cashu_treasury_intents WHERE created_at>=?")
            .bind(now.saturating_sub(86_400)).fetch_one(&mut **tx).await?;
    let pending:i64=sqlx::query_scalar("SELECT COALESCE(SUM(i.input_sats),0) FROM cashu_treasury_intents i WHERE NOT EXISTS(SELECT 1 FROM cashu_treasury_completions c WHERE c.intent_id=i.id)")
            .fetch_one(&mut **tx).await?;
    let mint_pending:i64=sqlx::query_scalar("SELECT COALESCE(SUM(i.input_sats),0) FROM cashu_treasury_intents i WHERE i.mint_id=? AND NOT EXISTS(SELECT 1 FROM cashu_treasury_completions c WHERE c.intent_id=i.id)")
            .bind(&intent.mint_id).fetch_one(&mut **tx).await?;
    let mint = policy
        .approved_mints
        .get(&intent.mint_id)
        .context("treasury mint not approved")?;
    ensure!(
        input
            <= mint
                .maximum_in_flight_sats
                .saturating_sub(to_u64(mint_pending, "mint in-flight sats")?),
        "mint treasury exposure exhausted"
    );
    ensure!(
        policy
            .limits
            .daily_operation_allowed(to_u64(usage.try_get("count")?, "treasury rate count")?)
            && input
                <= policy
                    .limits
                    .daily_input_remaining(to_u64(usage.try_get("sats")?, "treasury rate sats")?)
            && input
                <= policy
                    .limits
                    .maximum_in_flight_sats
                    .saturating_sub(to_u64(pending, "treasury in-flight sats")?),
        "treasury rate/exposure exhausted"
    );
    let daily_fee_limit = policy
        .limits
        .fee_sats_per_24h
        .context("treasury daily fee limit missing")?;
    ensure!(
        intent.maximum_fee_sats <= daily_fee_remaining(tx, now, daily_fee_limit).await?,
        "treasury daily fee budget exhausted"
    );
    for source in &intent.sources {
        ensure!(
            source.input_sats <= available_custody(tx, source.ledger_id).await?,
            "treasury source custody already consumed or reserved"
        );
    }
    let encoded = serde_json::to_string(&intent)?;
    ensure!(
        encoded.len() <= lightning_goats_cashu_receipt::treasury::MAX_TREASURY_BYTES,
        "treasury intent exceeds bound"
    );
    sqlx::query("INSERT INTO cashu_treasury_intents(id,intent_digest,intent_json,mint_id,input_sats,deposit_sats,created_at) VALUES(?,?,?,?,?,?,?)")
            .bind(intent.id.to_string()).bind(digest).bind(encoded).bind(&intent.mint_id).bind(to_i64(input,"treasury input")?)
            .bind(to_i64(intent.deposit_sats,"treasury deposit")?).bind(now).execute(&mut **tx).await?;
    for source in &intent.sources {
        sqlx::query(
            "INSERT INTO cashu_treasury_sources(intent_id,ledger_id,input_sats) VALUES(?,?,?)",
        )
        .bind(intent.id.to_string())
        .bind(source.ledger_id)
        .bind(to_i64(source.input_sats, "treasury source")?)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query("INSERT INTO cashu_treasury_clock VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET last_seen=excluded.last_seen")
            .bind(now).execute(&mut **tx).await?;
    Ok(intent)
}

/// Pending intents can still charge their maximum, even across the boundary.
/// A completed fee ages from the later of wallet result acceptance and Strike
/// verification. A missing observation cannot release an existing reservation.
pub(super) async fn daily_fee_remaining(
    tx: &mut Transaction<'_, Sqlite>,
    now: i64,
    limit: u64,
) -> Result<u64> {
    let used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CASE WHEN c.intent_id IS NULL OR d.intent_id IS NULL OR o.intent_id IS NULL THEN
             CASE WHEN json_type(i.intent_json,'$.maximum_fee_sats')='integer'
                   AND json_extract(i.intent_json,'$.maximum_fee_sats') BETWEEN 0 AND i.input_sats-i.deposit_sats
                  THEN json_extract(i.intent_json,'$.maximum_fee_sats')
                  ELSE i.input_sats-i.deposit_sats END
             ELSE c.fee_sats END),0)
         FROM cashu_treasury_intents i
         LEFT JOIN cashu_treasury_completions c ON c.intent_id=i.id
         LEFT JOIN cashu_treasury_deposits d ON d.intent_id=i.id
         LEFT JOIN cashu_treasury_fee_observations o ON o.intent_id=i.id
         WHERE c.intent_id IS NULL OR d.intent_id IS NULL OR o.intent_id IS NULL
            OR MAX(i.created_at,d.verified_at,o.observed_at)>=?",
    )
    .bind(now.saturating_sub(86_400))
    .fetch_one(&mut **tx)
    .await?;
    Ok(limit.saturating_sub(to_u64(used, "treasury rolling fees")?))
}

pub(super) async fn available_custody(
    tx: &mut Transaction<'_, Sqlite>,
    ledger: i64,
) -> Result<u64> {
    let acquired: i64 =
        sqlx::query_scalar(
            "SELECT c.net_sats FROM cashu_receipt_inbox c WHERE c.ledger_entry_id=?
             AND NOT EXISTS(SELECT 1 FROM retained_cashu_lots r WHERE r.ledger_id=c.ledger_entry_id)",
        )
            .bind(ledger)
            .fetch_one(&mut **tx)
            .await?;
    // Missing completion details hold the whole reservation, never release it.
    let used:i64=sqlx::query_scalar("SELECT COALESCE(SUM(CASE WHEN c.intent_id IS NULL OR r.ledger_id IS NULL THEN s.input_sats ELSE r.input_sats-r.change_sats END),0) FROM cashu_treasury_sources s LEFT JOIN cashu_treasury_completions c ON c.intent_id=s.intent_id LEFT JOIN cashu_treasury_source_results r ON r.intent_id=s.intent_id AND r.ledger_id=s.ledger_id WHERE s.ledger_id=?")
        .bind(ledger).fetch_one(&mut **tx).await?;
    to_u64(acquired, "acquired custody")?
        .checked_sub(to_u64(used, "reserved/consumed custody")?)
        .context("treasury custody over-allocation")
}
async fn complete(tx: &mut Transaction<'_, Sqlite>, intent: &Intent) -> Result<bool> {
    let deposit: Option<i64> =
        sqlx::query_scalar("SELECT deposit_sats FROM cashu_treasury_deposits WHERE intent_id=?")
            .bind(intent.id.to_string())
            .fetch_optional(&mut **tx)
            .await?;
    let result: Option<String> = sqlx::query_scalar(
        "SELECT result_json FROM cashu_treasury_wallet_results WHERE intent_id=?",
    )
    .bind(intent.id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    let (Some(deposit), Some(encoded)) = (deposit, result) else {
        return Ok(false);
    };
    ensure!(
        to_u64(deposit, "verified deposit")? == intent.deposit_sats,
        "treasury deposit amount changed"
    );
    let data: MeltData = serde_json::from_str(&encoded)
        .map_err(|_| anyhow::anyhow!("invalid private melt result"))?;
    data.validate(intent)?;
    let existing: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cashu_treasury_completions WHERE intent_id=?)",
    )
    .bind(intent.id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if existing {
        return Ok(true);
    }
    let mut fees = 0u64;
    let mut change = 0u64;
    for part in &data.sources {
        fees = fees
            .checked_add(part.fee_sats)
            .context("treasury fee overflow")?;
        change = change
            .checked_add(part.change_sats)
            .context("treasury change overflow")?;
        sqlx::query("INSERT INTO cashu_treasury_source_results(intent_id,ledger_id,input_sats,deposit_sats,fee_sats,change_sats) VALUES(?,?,?,?,?,?)")
            .bind(intent.id.to_string()).bind(part.ledger_id).bind(to_i64(part.input_sats,"treasury input")?)
            .bind(to_i64(part.deposit_sats,"treasury deposit")?).bind(to_i64(part.fee_sats,"treasury fee")?)
            .bind(to_i64(part.change_sats,"treasury change")?).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO cashu_treasury_completions(intent_id,input_sats,deposit_sats,fee_sats,change_sats) VALUES(?,?,?,?,?)")
        .bind(intent.id.to_string()).bind(to_i64(intent.input_sats,"treasury input")?).bind(deposit)
        .bind(to_i64(fees,"treasury fees")?).bind(to_i64(change,"treasury change")?).execute(&mut **tx).await?;
    Ok(true)
}
