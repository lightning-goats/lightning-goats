use super::{
    invoices::{touch_clock, write_lock},
    *,
};
use crate::{
    core_bridge::AuthenticatedOutcome,
    payout::{
        outcome::{Disposition, Header, Packet, Row as OutcomeRow, UnsubmittedReason, next_hash},
        provider::{BindingRecord, PaymentRecord, PaymentState},
    },
};
use sqlx::{Sqlite, Transaction};

impl Store {
    /// One bounded worker step. Resume from core's retained cursor instead of
    /// guessing whether the last reply was delivered. Recovery stays available
    /// while execution is disabled; it creates no payment or public message.
    #[cfg(unix)]
    pub async fn reconcile_outcomes(
        &self,
        core: &crate::core_bridge::unix::Client,
        grant: Uuid,
    ) -> Result<crate::payout::outcome::Progress> {
        let mut tx = self.pool.begin().await?;
        let (header, _) = load_manifest(&mut tx, grant)
            .await?
            .context("outcomes not frozen")?;
        tx.commit().await?;
        let mut ack = core.payout_outcome_progress(&header).await?;
        if !ack.progress().finalized {
            let packet = self.outcome_packet(grant, ack.progress().through).await?;
            ack = core.payout_outcome(&packet).await?;
        }
        if ack.progress().finalized {
            self.acknowledge_outcomes(grant, &ack, crate::cyberherd::calendar::now()?)
                .await?;
        }
        Ok(ack.progress().clone())
    }
    /// Durable no-execute attestation by the protected payout role. A timeout
    /// after execute intent is never eligible, even if the provider returns 404.
    pub async fn close_unsubmitted(
        &self,
        grant: Uuid,
        member: &str,
        reason: UnsubmittedReason,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let row=sqlx::query("SELECT g.admitted_at FROM payout_rows r JOIN payout_grants g ON g.id=r.grant_id WHERE r.grant_id=? AND r.member=?")
            .bind(grant.to_string()).bind(member).fetch_one(&mut *tx).await?;
        if now < row.try_get::<i64, _>("admitted_at")? {
            bail!("closure predates grant");
        }
        let reason = serde_json::to_string(&reason)?;
        if let Some(old) = sqlx::query_scalar::<_, String>(
            "SELECT reason FROM payout_unsubmitted WHERE grant_id=? AND member=?",
        )
        .bind(grant.to_string())
        .bind(member)
        .fetch_optional(&mut *tx)
        .await?
        {
            if old != reason {
                bail!("unsubmitted closure changed");
            }
            return Ok(());
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query(
            "INSERT INTO payout_unsubmitted(grant_id,member,reason,closed_at) VALUES(?,?,?,?)",
        )
        .bind(grant.to_string())
        .bind(member)
        .bind(reason)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Freeze all rows once, only after every row is terminal or durably closed.
    /// This is an attestation, not permission to release a single reserved sat.
    pub async fn freeze_outcomes(&self, grant: Uuid, now: i64) -> Result<Header> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        if let Some((header, _)) = load_manifest(&mut tx, grant).await? {
            return Ok(header);
        }
        let evidence: String =
            sqlx::query_scalar("SELECT qualification_json FROM payout_grants WHERE id=?")
                .bind(grant.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let evidence: serde_json::Value = serde_json::from_str(&evidence)?;
        let reservation: crate::reward_funding::Reservation =
            serde_json::from_value(evidence["reservation"].clone())?;
        if reservation.id != grant || now < reservation.created_at {
            bail!("invalid manifest reservation/time");
        }
        let rows = sqlx::query(
            "SELECT member,gross_sats FROM payout_rows WHERE grant_id=? ORDER BY member LIMIT 1001",
        )
        .bind(grant.to_string())
        .fetch_all(&mut *tx)
        .await?;
        if rows.is_empty() || rows.len() > 1000 {
            bail!("invalid manifest row count");
        }
        let mut outcomes = Vec::new();
        let mut gross = 0u64;
        for row in rows {
            let member: String = row.try_get("member")?;
            let gross_sats = u64::try_from(row.try_get::<i64, _>("gross_sats")?)?;
            let closure: Option<String> = sqlx::query_scalar(
                "SELECT reason FROM payout_unsubmitted WHERE grant_id=? AND member=?",
            )
            .bind(grant.to_string())
            .bind(&member)
            .fetch_optional(&mut *tx)
            .await?;
            let disposition = if let Some(reason) = closure {
                Disposition::Unsubmitted {
                    reason: serde_json::from_str(&reason)?,
                }
            } else {
                let binding:Option<String>=sqlx::query_scalar("SELECT b.document_json FROM payout_payment_bindings b JOIN payout_quote_intents q ON q.id=b.intent_id JOIN payout_invoice_intents i ON i.id=q.invoice_id WHERE i.grant_id=? AND i.member=?")
                    .bind(grant.to_string()).bind(&member).fetch_optional(&mut *tx).await?;
                let binding_json =
                    binding.context("outstanding/unknown row prevents manifest finalization")?;
                let binding: BindingRecord = serde_json::from_str(&binding_json)?;
                let conflict: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM payout_payment_conflicts WHERE payment_id=?)",
                )
                .bind(binding.payment_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
                if conflict {
                    bail!("provider conflict prevents finalization");
                }
                let observed:Option<String>=sqlx::query_scalar("SELECT document_json FROM payout_payment_records WHERE payment_id=? ORDER BY seq DESC LIMIT 1")
                    .bind(binding.payment_id.to_string()).fetch_optional(&mut *tx).await?;
                let observed: PaymentRecord =
                    serde_json::from_str(&observed.context("unobserved payment remains held")?)?;
                if observed.binding != binding || observed.state == PaymentState::Pending {
                    bail!("nonterminal/corrupt payment remains held");
                }
                Disposition::Payment {
                    binding_json,
                    state: observed.state,
                    fee_sats: observed.fee_sats,
                    total_sats: observed.total_sats,
                }
            };
            gross = gross
                .checked_add(gross_sats)
                .context("manifest gross overflow")?;
            outcomes.push(OutcomeRow {
                member,
                gross_sats,
                disposition,
            });
        }
        if gross != reservation.reserved_sats {
            bail!("manifest gross differs from reservation");
        }
        let mut header = Header {
            reservation,
            scope: self.policy.scope.provider.clone(),
            rows: outcomes.len().try_into()?,
            final_hash: "00".repeat(32),
        };
        let mut hash = header.seed()?;
        for (index, row) in outcomes.iter().enumerate() {
            Packet {
                header: header.clone(),
                after: index.try_into()?,
                before_hash: hash.clone(),
                row: row.clone(),
            }
            .validate()?;
            hash = next_hash(&hash, row)?;
        }
        header.final_hash = hash;
        header.validate()?;
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_outcome_manifests(grant_id,header_json,rows_json,created_at) VALUES(?,?,?,?)")
            .bind(grant.to_string()).bind(serde_json::to_string(&header)?).bind(serde_json::to_string(&outcomes)?)
            .bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(header)
    }

    pub async fn outcome_packet(&self, grant: Uuid, after: u16) -> Result<Packet> {
        let mut tx = self.pool.begin().await?;
        let (header, rows) = load_manifest(&mut tx, grant)
            .await?
            .context("outcomes not frozen")?;
        let row = rows
            .get(usize::from(after))
            .context("manifest cursor exhausted")?
            .clone();
        let mut before_hash = header.seed()?;
        for row in rows.iter().take(usize::from(after)) {
            before_hash = next_hash(&before_hash, row)?;
        }
        let packet = Packet {
            header,
            after,
            before_hash,
            row,
        };
        packet.validate()?;
        Ok(packet)
    }

    pub async fn acknowledge_outcomes(
        &self,
        grant: Uuid,
        ack: &AuthenticatedOutcome,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let (header, _) = load_manifest(&mut tx, grant)
            .await?
            .context("outcomes not frozen")?;
        let progress = ack.progress();
        progress.validate(&header)?;
        if ack.core_uid() != self.policy.scope.core_uid
            || !progress.finalized
            || now < progress.verified_at
        {
            bail!("not a final authenticated core outcome");
        }
        let json = serde_json::to_string(progress)?;
        if let Some(old) = sqlx::query_scalar::<_, String>(
            "SELECT progress_json FROM payout_outcome_acks WHERE grant_id=?",
        )
        .bind(grant.to_string())
        .fetch_optional(&mut *tx)
        .await?
        {
            if old != json {
                bail!("final core acknowledgement changed");
            }
            return Ok(());
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_outcome_acks(grant_id,progress_json,spent_sats,held_sats,accepted_at) VALUES(?,?,?,?,?)")
            .bind(grant.to_string()).bind(json).bind(i64::try_from(progress.spent_sats)?)
            .bind(i64::try_from(progress.held_sats)?).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

pub(super) async fn load_manifest(
    tx: &mut Transaction<'_, Sqlite>,
    grant: Uuid,
) -> Result<Option<(Header, Vec<OutcomeRow>)>> {
    let Some(row) =
        sqlx::query("SELECT header_json,rows_json FROM payout_outcome_manifests WHERE grant_id=?")
            .bind(grant.to_string())
            .fetch_optional(&mut **tx)
            .await?
    else {
        return Ok(None);
    };
    let header: Header = serde_json::from_str(&row.try_get::<String, _>("header_json")?)?;
    header.validate()?;
    let rows: Vec<OutcomeRow> = serde_json::from_str(&row.try_get::<String, _>("rows_json")?)?;
    if header.reservation.id != grant || rows.len() != usize::from(header.rows) {
        bail!("stored manifest scope/count changed");
    }
    let mut hash = header.seed()?;
    let mut gross = 0u64;
    let mut last = None;
    for row in &rows {
        if last.is_some_and(|last: &String| last >= &row.member) {
            bail!("manifest order changed");
        }
        last = Some(&row.member);
        hash = next_hash(&hash, row)?;
        gross = gross
            .checked_add(row.gross_sats)
            .context("manifest total overflow")?;
    }
    if hash != header.final_hash || gross != header.reservation.reserved_sats {
        bail!("stored manifest hash/amount changed");
    }
    Ok(Some((header, rows)))
}

pub(super) async fn usage(tx: &mut Transaction<'_, Sqlite>, now: i64) -> Result<u64> {
    let used:i64=sqlx::query_scalar("SELECT COALESCE((SELECT SUM(g.reserved_sats) FROM payout_grants g WHERE NOT EXISTS(SELECT 1 FROM payout_outcome_acks a WHERE a.grant_id=g.id)),0)+COALESCE((SELECT SUM(held_sats) FROM payout_outcome_acks),0)-COALESCE((SELECT SUM(gross_sats) FROM payout_cost_acks),0)+COALESCE((SELECT SUM(spent_sats) FROM payout_outcome_acks WHERE accepted_at>?),0)+COALESCE((SELECT SUM(spent_sats) FROM payout_cost_acks WHERE accepted_at>?),0)")
        .bind(now.saturating_sub(86400)).bind(now.saturating_sub(86400)).fetch_one(&mut **tx).await?;
    Ok(used.try_into()?)
}
pub(super) async fn ensure_open(
    tx: &mut Transaction<'_, Sqlite>,
    grant: Uuid,
    member: &str,
) -> Result<()> {
    let closed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_unsubmitted WHERE grant_id=? AND member=?) OR EXISTS(SELECT 1 FROM payout_outcome_manifests WHERE grant_id=?)")
        .bind(grant.to_string()).bind(member).bind(grant.to_string()).fetch_one(&mut **tx).await?;
    if closed {
        bail!("payout row is permanently closed");
    }
    Ok(())
}
pub(super) async fn ensure_invoice_open(
    tx: &mut Transaction<'_, Sqlite>,
    invoice: Uuid,
) -> Result<()> {
    let row = sqlx::query("SELECT grant_id,member FROM payout_invoice_intents WHERE id=?")
        .bind(invoice.to_string())
        .fetch_one(&mut **tx)
        .await?;
    ensure_open(
        tx,
        row.try_get::<String, _>("grant_id")?.parse()?,
        &row.try_get::<String, _>("member")?,
    )
    .await
}
