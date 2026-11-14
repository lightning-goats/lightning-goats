use super::{
    LedgerStore,
    reward_outcomes::{budget_time, validate_scope},
    to_u64,
};
use crate::payout::{
    cost_recovery::{ApprovedDecision, Decision, Receipt, hash},
    outcome::{Header, Packet, Reader, Row as OutcomeRow},
};
use anyhow::{Context as _, Result, bail};
use sqlx::{Row as _, Sqlite, Transaction};

impl LedgerStore {
    #[cfg(unix)]
    pub async fn connect_existing_recovery(database_url: &str) -> Result<Self> {
        crate::payout::cost_recovery::existing_private_database(database_url)?;
        let pool = crate::sqlite::connect_existing_durable(database_url, 1).await?;
        crate::sqlite::require_current_schema(&pool, &super::MIGRATOR).await?;
        Ok(Self { pool })
    }
    /// Produce correlation fields only. Null cost/evidence and false review
    /// acknowledgement make this deliberately unusable as an approval.
    pub async fn failed_cost_draft(
        &self,
        id: uuid::Uuid,
        member: &str,
    ) -> Result<serde_json::Value> {
        crate::nostr_event::validate_public_key(member)?;
        if id.is_nil() {
            bail!("reservation must be non-nil");
        }
        let mut tx = self.pool.begin().await?;
        let p = packet(&mut tx, id, member).await?;
        let binding = p
            .validate()?
            .context("failed cost lacks original execution")?;
        let now = budget_time(&mut tx, false).await?;
        let result = serde_json::json!({"version":1,"id":uuid::Uuid::new_v4(),"epoch":p.header.reservation.epoch,"reservation_id":id,"owner":p.header.reservation.owner,"member":member,"account_id":p.header.scope.account_id,"manifest_hash":p.header.final_hash,"payment_id":binding.payment_id,"invoice_hash":binding.quote.request.payment_hash,"gross_sats":p.row.gross_sats,"actual_cost_sats":null,"evidence_sha256":null,"statement_reference":"","reason":"","reviewed_failed_payment_cost":false,"approved_at":now,"expires_at":now.saturating_add(86400)});
        tx.commit().await?;
        Ok(result)
    }
    /// Offline preview validates exact retained scope; no provider or money write.
    pub async fn preview_failed_cost(&self, decision: &Decision) -> Result<Option<Receipt>> {
        let mut tx = self.pool.begin().await?;
        let packet = packet(&mut tx, decision.reservation_id, &decision.member).await?;
        decision.matches(&packet.header, &packet.row)?;
        let receipt = existing(&mut tx, decision).await?;
        if let Some(receipt) = &receipt {
            receipt.validate(&packet.header, &packet.row)?;
        }
        tx.commit().await?;
        Ok(receipt)
    }
    pub async fn reconcile_failed_cost(
        &self,
        approval: &ApprovedDecision,
        reader: &Reader,
    ) -> Result<Receipt> {
        let decision = approval.decision();
        decision.validate()?;
        let mut tx = self.pool.begin().await?;
        let original = packet(&mut tx, decision.reservation_id, &decision.member).await?;
        decision.matches(&original.header, &original.row)?;
        if let Some(done) = existing(&mut tx, decision).await? {
            done.validate(&original.header, &original.row)?;
            tx.commit().await?;
            return Ok(done);
        }
        let now = budget_time(&mut tx, false).await?;
        if now < decision.approved_at || now > decision.expires_at {
            bail!("failed cost approval is not current");
        }
        tx.commit().await?;
        // Fresh independent read of the SAME original failed payment. The API
        // confirms execution state, not the human-reviewed final refund/cost.
        let verified = reader.verify(&original).await?;
        if verified.held() != decision.gross_sats || verified.spent() != 0 {
            bail!("failed payment no longer agrees with retained hold");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = packet(&mut tx, decision.reservation_id, &decision.member).await?;
        if current != original {
            bail!("failed cost source changed during verification");
        }
        if let Some(done) = existing(&mut tx, decision).await? {
            done.validate(&current.header, &current.row)?;
            tx.commit().await?;
            return Ok(done);
        }
        let now = budget_time(&mut tx, true).await?;
        let receipt = Receipt {
            decision: decision.clone(),
            header_hash: hash(&current.header)?,
            row_hash: hash(&current.row)?,
            verified_at: now,
        };
        receipt.validate(&current.header, &current.row)?;
        sqlx::query("INSERT INTO payout_failed_costs(reservation_id,member,id,document_json,gross_sats,spent_sats,verified_at) VALUES(?,?,?,?,?,?,?)")
            .bind(decision.reservation_id.to_string()).bind(&decision.member).bind(decision.id.to_string()).bind(serde_json::to_string(&receipt)?).bind(i64::try_from(decision.gross_sats)?).bind(i64::try_from(decision.actual_cost_sats)?).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
    /// Payout-role read only. No RPC can approve, amend or release failed costs.
    pub(crate) async fn failed_cost_receipt(
        &self,
        header: &Header,
        member: &str,
    ) -> Result<Option<Receipt>> {
        crate::nostr_event::validate_public_key(member)?;
        let mut tx = self.pool.begin().await?;
        let p = packet(&mut tx, header.reservation.id, member).await?;
        if p.header != *header {
            bail!("failed cost read changed original manifest");
        }
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT document_json FROM payout_failed_costs WHERE reservation_id=? AND member=?",
        )
        .bind(header.reservation.id.to_string())
        .bind(member)
        .fetch_optional(&mut *tx)
        .await?;
        let result = raw
            .map(|raw| -> Result<_> {
                let r: Receipt = serde_json::from_str(&raw)?;
                r.validate(header, &p.row)?;
                Ok(r)
            })
            .transpose()?;
        tx.commit().await?;
        Ok(result)
    }
}
async fn packet(tx: &mut Transaction<'_, Sqlite>, id: uuid::Uuid, member: &str) -> Result<Packet> {
    let header: Header = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT header_json FROM reward_outcome_manifests WHERE reservation_id=?",
        )
        .bind(id.to_string())
        .fetch_one(&mut **tx)
        .await?,
    )?;
    validate_scope(tx, &header).await?;
    let final_json: String = sqlx::query_scalar(
        "SELECT progress_json FROM reward_outcome_finals WHERE reservation_id=?",
    )
    .bind(id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    let progress: crate::payout::outcome::Progress = serde_json::from_str(&final_json)?;
    progress.validate(&header)?;
    if !progress.finalized {
        bail!("failed costs require an exact finalized manifest");
    }
    let row = sqlx::query("SELECT * FROM reward_outcome_rows WHERE reservation_id=? AND member=?")
        .bind(id.to_string())
        .bind(member)
        .fetch_one(&mut **tx)
        .await?;
    let body: OutcomeRow = serde_json::from_str(&row.try_get::<String, _>("row_json")?)?;
    if row.try_get::<i64, _>("spent_sats")? != 0
        || to_u64(row.try_get("held_sats")?, "failed hold")? != body.gross_sats
    {
        bail!("cost row is not a full retained failed hold");
    }
    let packet = Packet {
        header,
        after: u16::try_from(row.try_get::<i64, _>("position")?)?,
        before_hash: row.try_get("before_hash")?,
        row: body,
    };
    if crate::payout::outcome::next_hash(&packet.before_hash, &packet.row)?
        != row.try_get::<String, _>("chain_hash")?
    {
        bail!("failed cost row chain changed");
    }
    packet
        .validate()?
        .context("failed cost lacks executed payment binding")?;
    super::payout_execution::validate_outcome(tx, &packet).await?;
    Ok(packet)
}
async fn existing(
    tx: &mut Transaction<'_, Sqlite>,
    decision: &Decision,
) -> Result<Option<Receipt>> {
    let rows=sqlx::query("SELECT document_json FROM payout_failed_costs WHERE id=? OR (reservation_id=? AND member=?)")
        .bind(decision.id.to_string()).bind(decision.reservation_id.to_string()).bind(&decision.member).fetch_all(&mut **tx).await?;
    if rows.len() > 1 {
        bail!("failed cost identity conflicts");
    }
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let receipt: Receipt = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
    if receipt.decision != *decision {
        bail!("failed cost approval changed");
    }
    Ok(Some(receipt))
}
