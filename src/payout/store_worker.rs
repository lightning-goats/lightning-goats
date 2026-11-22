//! Durable work selection and authenticated intake checkpoint. Scheduling is
//! advisory only; every external action still requires its journal capability.
use super::{
    invoices::{touch_clock, write_lock},
    *,
};
use crate::{
    cyberherd::payout_bridge::AuthenticatedExport,
    payout::provider::{QuoteDocument, QuoteIntent, QuoteRecord},
};

pub(crate) struct Work {
    pub grant: Uuid,
    pub member: Option<String>,
}
impl Store {
    pub async fn saved_invoice_intent(
        &self,
        grant: Uuid,
        member: &str,
    ) -> Result<Option<super::super::invoice::Intent>> {
        let mut tx = self.pool.begin().await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM payout_invoice_intents WHERE grant_id=? AND member=?",
        )
        .bind(grant.to_string())
        .bind(member)
        .fetch_optional(&mut *tx)
        .await?;
        let intent = match id {
            Some(id) => {
                Some(super::invoices::load_intent(&mut tx, &id, &self.policy.scope.owner).await?)
            }
            None => None,
        };
        tx.commit().await?;
        Ok(intent)
    }
    pub(crate) async fn row_uses_nostr(&self, grant: Uuid, member: &str) -> Result<bool> {
        let row = sqlx::query(
            "SELECT address,recipient_json FROM payout_rows WHERE grant_id=? AND member=?",
        )
        .bind(grant.to_string())
        .bind(member)
        .fetch_one(&self.pool)
        .await?;
        let recipient: super::super::recipient::Recipient =
            serde_json::from_str(&row.try_get::<String, _>("recipient_json")?)?;
        recipient.validate(&row.try_get::<String, _>("address")?)?;
        Ok(recipient.receipt_pubkey.is_some())
    }
    pub fn policy(&self) -> &Policy {
        &self.policy
    }
    pub async fn initialize_intake(&self, peer_uid: u32) -> Result<()> {
        if peer_uid == 0 || peer_uid == self.policy.scope.core_uid {
            bail!("intake must be a separate non-root identity");
        }
        sqlx::query("INSERT INTO payout_intake_binding(singleton,peer_uid,through) VALUES(1,?,0) ON CONFLICT(singleton) DO NOTHING")
            .bind(i64::from(peer_uid)).execute(&self.pool).await?;
        let old: i64 =
            sqlx::query_scalar("SELECT peer_uid FROM payout_intake_binding WHERE singleton=1")
                .fetch_one(&self.pool)
                .await?;
        if old != i64::from(peer_uid) {
            bail!("payout intake identity changed; explicit recovery required");
        }
        Ok(())
    }
    pub async fn intake_cursor(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT through FROM payout_intake_binding WHERE singleton=1")
                .fetch_one(&self.pool)
                .await?,
        )
    }
    pub async fn acknowledge_intake(&self, received: &AuthenticatedExport) -> Result<()> {
        let export = received.export();
        export.validate()?;
        if export.plan.owner != self.policy.scope.owner
            || export.plan.epoch != self.policy.scope.epoch
            || export.plan.core_uid != self.policy.scope.core_uid
        {
            bail!("intake export belongs to another payout scope");
        }
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let row =
            sqlx::query("SELECT peer_uid,through FROM payout_intake_binding WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if row.try_get::<i64, _>("peer_uid")? != i64::from(received.peer_uid()) {
            bail!("intake export has wrong kernel peer");
        }
        if let Some(row) = sqlx::query("SELECT * FROM payout_intake_receipts WHERE sequence=?")
            .bind(export.sequence)
            .fetch_optional(&mut *tx)
            .await?
        {
            if row.try_get::<String, _>("plan_hash")? != export.hash
                || row.try_get::<String, _>("request_id")? != export.plan.request_id.to_string()
                || row.try_get::<Option<String>, _>("grant_id")?
                    != export.reservation.as_ref().map(|r| r.id.to_string())
            {
                bail!("intake retry changed receipt");
            }
            return Ok(());
        }
        if export.sequence <= row.try_get::<i64, _>("through")? {
            bail!("intake cannot rewind its cursor");
        }
        let grant = if let Some(r) = &export.reservation {
            let row = sqlx::query(
                "SELECT plan_json,plan_hash,qualification_json FROM payout_grants WHERE id=?",
            )
            .bind(r.id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            let qualified: serde_json::Value =
                serde_json::from_str(&row.try_get::<String, _>("qualification_json")?)?;
            let saved: crate::reward_funding::Reservation =
                serde_json::from_value(qualified["reservation"].clone())?;
            if saved != *r
                || row.try_get::<String, _>("plan_hash")? != export.hash
                || row.try_get::<String, _>("plan_json")? != serde_json::to_string(&export.plan)?
            {
                bail!("export has no matching admitted grant");
            }
            Some(r.id.to_string())
        } else {
            None
        };
        sqlx::query("INSERT INTO payout_intake_receipts(sequence,plan_hash,request_id,grant_id) VALUES(?,?,?,?)")
            .bind(export.sequence).bind(&export.hash).bind(export.plan.request_id.to_string()).bind(grant).execute(&mut *tx).await?;
        let plan = &export.plan;
        if plan.version == 2 && !plan.funding_source.is_strike() && plan.operations_sats > 0 {
            let funding_hash = plan
                .funding_hash
                .as_deref()
                .context("missing operator funding proof")?;
            crate::nostr_event::validate_hex(funding_hash, 32)?;
            sqlx::query("INSERT INTO payout_operator_allocations(sequence,funding_hash,collected_sats,member_reserved_sats,operator_sats) VALUES(?,?,?,?,?)")
                .bind(export.sequence)
                .bind(funding_hash)
                .bind(i64::try_from(plan.real_sats)?)
                .bind(i64::try_from(plan.reserved_sats)?)
                .bind(i64::try_from(plan.operations_sats)?)
                .execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE payout_intake_binding SET through=? WHERE singleton=1")
            .bind(export.sequence)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn operator_allocation(
        &self,
        sequence: i64,
    ) -> Result<Option<crate::reward_funding::OperatorAllocation>> {
        let row = sqlx::query("SELECT r.request_id,r.plan_hash,r.grant_id,a.funding_hash,a.collected_sats,a.member_reserved_sats,a.operator_sats FROM payout_operator_allocations a JOIN payout_intake_receipts r USING(sequence) WHERE sequence=?")
            .bind(sequence).fetch_optional(&self.pool).await?;
        row.map(|r| -> Result<_> {
            Ok(crate::reward_funding::OperatorAllocation {
                epoch: self.policy.scope.epoch,
                owner: self.policy.scope.owner.clone(),
                core_uid: self.policy.scope.core_uid,
                request_id: r.try_get::<String, _>("request_id")?.parse()?,
                plan_hash: r.try_get("plan_hash")?,
                funding_hash: r.try_get("funding_hash")?,
                member_grant: r
                    .try_get::<Option<String>, _>("grant_id")?
                    .map(|id| id.parse())
                    .transpose()?,
                collected_sats: u64::try_from(r.try_get::<i64, _>("collected_sats")?)?,
                member_reserved_sats: u64::try_from(r.try_get::<i64, _>("member_reserved_sats")?)?,
                operator_sats: u64::try_from(r.try_get::<i64, _>("operator_sats")?)?,
            })
        })
        .transpose()
    }

    async fn record_attempt(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        work_key: &str,
    ) -> Result<()> {
        let old: i64 =
            sqlx::query_scalar("SELECT sequence FROM payout_worker_clock WHERE singleton=1")
                .fetch_one(&mut **tx)
                .await?;
        let next = old.checked_add(1).context("worker sequence exhausted")?;
        sqlx::query("UPDATE payout_worker_clock SET sequence=? WHERE singleton=1")
            .bind(next)
            .execute(&mut **tx)
            .await?;
        sqlx::query("INSERT INTO payout_worker_attempts(work_key,sequence) VALUES(?,?) ON CONFLICT(work_key) DO UPDATE SET sequence=excluded.sequence")
            .bind(work_key).bind(next).execute(&mut **tx).await?;
        Ok(())
    }

    /// The first validated recipient invoice is final for this feeding, even
    /// across expiry or a lost collector response.
    pub async fn bind_operator_invoice(
        &self,
        sequence: i64,
        address: &str,
        invoice: &super::super::invoice::ValidatedInvoice,
        maximum_fee_sat: u64,
    ) -> Result<()> {
        super::super::recipient::address_url(address)?;
        let amount = i64::try_from(invoice.amount_sats())?;
        let fee = i64::try_from(maximum_fee_sat)?;
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let feed: String =
            sqlx::query_scalar("SELECT request_id FROM payout_intake_receipts WHERE sequence=?")
                .bind(sequence)
                .fetch_one(&mut *tx)
                .await?;
        if feed != invoice.intent_id().to_string() || invoice.nip57() {
            bail!("operator invoice is not bound to this feeding");
        }
        let saved: Option<(String,String,String,i64,i64)> = sqlx::query_as(
            "SELECT address,invoice,payment_hash,amount_sat,maximum_fee_sat FROM payout_operator_invoices WHERE sequence=?"
        ).bind(sequence).fetch_optional(&mut *tx).await?;
        if let Some(saved) = saved {
            if saved
                != (
                    address.to_owned(),
                    invoice.encoded().to_owned(),
                    invoice.payment_hash().to_owned(),
                    amount,
                    fee,
                )
            {
                bail!("operator invoice differs from frozen terms");
            }
        } else {
            sqlx::query("INSERT INTO payout_operator_invoices(sequence,address,invoice,payment_hash,amount_sat,maximum_fee_sat) VALUES(?,?,?,?,?,?)")
                .bind(sequence).bind(address).bind(invoice.encoded()).bind(invoice.payment_hash())
                .bind(amount).bind(fee).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn settle_operator_invoice(
        &self,
        sequence: i64,
        result: &super::super::cashu::CollectorResult,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let (hash, amount, limit): (String, i64, i64) = sqlx::query_as(
            "SELECT payment_hash,amount_sat,maximum_fee_sat FROM payout_operator_invoices WHERE sequence=?"
        ).bind(sequence).fetch_one(&mut *tx).await?;
        let fee = result
            .verified_fee(&hash, amount.try_into()?, limit.try_into()?)?
            .context("operator payment is unsettled")?;
        let super::super::cashu::CollectorResult::Paid {
            payment_preimage, ..
        } = result
        else {
            bail!("missing operator settlement preimage");
        };
        let fee = i64::try_from(fee)?;
        let old: Option<(String, i64)> = sqlx::query_as(
            "SELECT payment_preimage,fee_sat FROM payout_operator_receipts WHERE sequence=?",
        )
        .bind(sequence)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((preimage, old_fee)) = old {
            if preimage != *payment_preimage || old_fee != fee {
                bail!("operator receipt differs from prior settlement");
            }
        } else {
            sqlx::query("INSERT INTO payout_operator_receipts(sequence,payment_preimage,fee_sat) VALUES(?,?,?)")
                .bind(sequence).bind(payment_preimage).bind(fee).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn operator_method(
        &self,
        sequence: i64,
        choice: Option<(&str, String)>,
    ) -> Result<Option<(String, String)>> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let existing = sqlx::query_as(
            "SELECT method,destination FROM payout_operator_methods WHERE sequence=?",
        )
        .bind(sequence)
        .fetch_optional(&mut *tx)
        .await?;
        if existing.is_some() {
            return Ok(existing);
        }
        let Some((method, destination)) = choice else {
            return Ok(None);
        };
        sqlx::query("INSERT INTO payout_operator_methods VALUES(?,?,?)")
            .bind(sequence)
            .bind(method)
            .bind(&destination)
            .execute(&mut *tx)
            .await?;
        if method == "p2pk" {
            sqlx::query(
                "INSERT INTO payout_operator_ecash_status(sequence,state) VALUES(?,'pending')",
            )
            .bind(sequence)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(Some((method.to_owned(), destination)))
    }

    pub(crate) async fn begin_direct_attempt(&self, sequence: i64) -> Result<()> {
        sqlx::query("UPDATE payout_operator_ecash_status SET state='pending',reason=NULL WHERE sequence=? AND state='held'")
            .bind(sequence).execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) async fn record_direct_status(
        &self,
        sequence: i64,
        terms: &super::super::operator::DirectExecution,
        status: super::super::operator::DirectStatus,
    ) -> Result<()> {
        use super::super::operator::DirectStatus;
        match status {
            DirectStatus::Ready {
                amount_sat,
                fee_sat,
            } => {
                if amount_sat == 0
                    || amount_sat.checked_add(fee_sat) != Some(terms.allocation.operator_sats)
                    || fee_sat > terms.fee_limit
                {
                    bail!("direct payout amount or fee differs");
                }
                sqlx::query("UPDATE payout_operator_ecash_status SET state='ready',reason=NULL,amount_sat=?,fee_sat=? WHERE sequence=? AND state!='ready'")
                    .bind(i64::try_from(amount_sat)?).bind(i64::try_from(fee_sat)?).bind(sequence).execute(&self.pool).await?;
            }
            DirectStatus::Held { reason } => {
                if !matches!(
                    reason.as_str(),
                    "no_payable_amount" | "fee_limit" | "nut11_unsupported"
                ) {
                    bail!("unknown direct payout status");
                }
                sqlx::query("UPDATE payout_operator_ecash_status SET state='held',reason=? WHERE sequence=? AND state!='ready'").bind(reason).bind(sequence).execute(&self.pool).await?;
            }
        }
        Ok(())
    }
    pub async fn next_operator_allocation(&self) -> Result<Option<i64>> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let sequence: Option<i64> = sqlx::query_scalar(
            "SELECT a.sequence FROM payout_operator_allocations a
             LEFT JOIN payout_worker_attempts w ON w.work_key='operator:'||a.sequence
             LEFT JOIN payout_operator_receipts r ON r.sequence=a.sequence
             WHERE r.sequence IS NULL AND NOT EXISTS (SELECT 1 FROM payout_operator_ecash_status s WHERE s.sequence=a.sequence AND s.state='ready') ORDER BY COALESCE(w.sequence,0),a.sequence LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = sequence {
            Self::record_attempt(&mut tx, &format!("operator:{id}")).await?;
        }
        tx.commit().await?;
        Ok(sequence)
    }

    pub(crate) async fn saved_operator_execution(
        &self,
        sequence: i64,
        allocation: crate::reward_funding::OperatorAllocation,
        lots: Vec<super::super::funding::RetainedAllocation>,
    ) -> Result<Option<super::super::operator::Execution>> {
        let row = sqlx::query("SELECT address,invoice,payment_hash,amount_sat,maximum_fee_sat FROM payout_operator_invoices WHERE sequence=?")
            .bind(sequence).fetch_optional(&self.pool).await?;
        row.map(|r| {
            Ok(super::super::operator::Execution {
                allocation,
                lots,
                address: r.try_get("address")?,
                invoice: r.try_get("invoice")?,
                payment_hash: r.try_get("payment_hash")?,
                amount_sat: r.try_get::<i64, _>("amount_sat")?.try_into()?,
                maximum_fee_sat: r.try_get::<i64, _>("maximum_fee_sat")?.try_into()?,
            })
        })
        .transpose()
    }

    /// A lost response can be recovered with the feed UUID and exact frozen invoice.
    /// The collector lookup accepts neither a spending allowance nor proof inputs.
    pub(crate) async fn reconcile_operator(
        &self,
        sequence: i64,
        socket: &std::path::Path,
        collector_uid: u32,
    ) -> Result<bool> {
        let saved: Option<(String, String)> = sqlx::query_as(
            "SELECT r.request_id,i.invoice FROM payout_operator_invoices i
             JOIN payout_intake_receipts r USING(sequence) WHERE i.sequence=?",
        )
        .bind(sequence)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((feed, invoice)) = saved {
            let result: super::super::cashu::CollectorResult = crate::private_ipc::request(
                socket,
                collector_uid,
                &serde_json::json!({"reconcile_operator":feed,"invoice":invoice}),
                65536,
                30,
            )
            .await?;
            if matches!(result, super::super::cashu::CollectorResult::Paid { .. }) {
                self.settle_operator_invoice(sequence, &result).await?;
            }
            return Ok(matches!(
                result,
                super::super::cashu::CollectorResult::Pending { .. }
                    | super::super::cashu::CollectorResult::Paid { .. }
            ));
        }
        Ok(false)
    }

    pub(crate) async fn take_work(&self, now: i64) -> Result<Option<Work>> {
        self.take_provider_work(now, "strike").await
    }

    pub(crate) async fn take_provider_work(
        &self,
        now: i64,
        provider: &str,
    ) -> Result<Option<Work>> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        touch_clock(&mut tx, now).await?;
        // Completed/failed rows are not repeatedly read, and a conflicted row
        // stays held for operator recovery. Other rows/grants continue fairly.
        let row=sqlx::query(r#"
WITH grants AS (
 SELECT * FROM payout_grants WHERE COALESCE(json_extract(qualification_json,'$.execution_provider'),'strike')=?1
), state AS (
 SELECT r.grant_id,r.member,g.admitted_at,
 EXISTS(SELECT 1 FROM payout_unsubmitted c WHERE c.grant_id=r.grant_id AND c.member=r.member) AS closed,
 EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_quote_intents q ON q.invoice_id=i.id JOIN payout_payment_bindings b ON b.intent_id=q.id JOIN payout_payment_records p ON p.payment_id=b.payment_id
 WHERE i.grant_id=r.grant_id AND i.member=r.member AND p.seq=(SELECT MAX(p2.seq) FROM payout_payment_records p2 WHERE p2.payment_id=b.payment_id) AND p.state IN ('COMPLETED','FAILED')) AS terminal,
 EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_quote_intents q ON q.invoice_id=i.id JOIN payout_payment_bindings b ON b.intent_id=q.id JOIN payout_payment_conflicts c ON c.payment_id=b.payment_id WHERE i.grant_id=r.grant_id AND i.member=r.member) AS conflicted
 FROM payout_rows r JOIN grants g ON g.id=r.grant_id
), jobs AS (
 SELECT 'row:'||s.grant_id||':'||s.member AS work_key,s.grant_id,s.member,s.admitted_at FROM state s
 WHERE NOT s.closed AND NOT s.terminal AND NOT s.conflicted AND NOT EXISTS(SELECT 1 FROM payout_outcome_manifests m WHERE m.grant_id=s.grant_id)
 UNION ALL
 SELECT 'outcome:'||g.id,g.id,NULL,g.admitted_at FROM grants g
 WHERE NOT EXISTS(SELECT 1 FROM payout_outcome_acks a WHERE a.grant_id=g.id)
 AND (EXISTS(SELECT 1 FROM payout_outcome_manifests m WHERE m.grant_id=g.id) OR NOT EXISTS(SELECT 1 FROM state s WHERE s.grant_id=g.id AND (s.conflicted OR (NOT s.closed AND NOT s.terminal))))
)
SELECT j.* FROM jobs j LEFT JOIN payout_worker_attempts a ON a.work_key=j.work_key ORDER BY COALESCE(a.sequence,0),j.admitted_at,j.work_key LIMIT 1
"#).bind(provider).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        Self::record_attempt(&mut tx, &row.try_get::<String, _>("work_key")?).await?;
        let work = Work {
            grant: row.try_get::<String, _>("grant_id")?.parse()?,
            member: row.try_get("member")?,
        };
        tx.commit().await?;
        Ok(Some(work))
    }
    pub async fn current_quote(&self, invoice: Uuid) -> Result<Option<QuoteIntent>> {
        let raw:Option<String>=sqlx::query_scalar("SELECT document_json FROM payout_quote_intents WHERE invoice_id=? AND retired_at IS NULL")
            .bind(invoice.to_string()).fetch_optional(&self.pool).await?;
        raw.map(|raw| -> Result<_> {
            let doc: QuoteDocument = serde_json::from_str(&raw)?;
            doc.validate()?;
            if doc.scope != self.policy.scope.provider || doc.invoice_id != invoice {
                bail!("current quote scope changed");
            }
            Ok(QuoteIntent(doc))
        })
        .transpose()
    }
    pub(crate) async fn quote_terms(&self, id: Uuid) -> Result<Option<QuoteRecord>> {
        let raw: Option<String> =
            sqlx::query_scalar("SELECT document_json FROM payout_quote_records WHERE intent_id=?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        raw.map(|raw| -> Result<_> {
            let record: QuoteRecord = serde_json::from_str(&raw)?;
            record.validate()?;
            if record.request.id != id || record.request.scope != self.policy.scope.provider {
                bail!("quote terms scope changed");
            }
            Ok(record)
        })
        .transpose()
    }
    pub async fn callback_request(
        &self,
        id: Uuid,
    ) -> Result<Option<super::super::invoice::Request>> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM payout_callback_requests WHERE intent_id=?)",
        )
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await?;
        if !exists {
            return Ok(None);
        }
        Ok(Some(self.invoice_request(id).await?))
    }
}
