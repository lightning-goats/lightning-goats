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
        sqlx::query("UPDATE payout_intake_binding SET through=? WHERE singleton=1")
            .bind(export.sequence)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub(crate) async fn take_work(&self, now: i64) -> Result<Option<Work>> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        touch_clock(&mut tx, now).await?;
        // Completed/failed rows are not repeatedly read, and a conflicted row
        // stays held for operator recovery. Other rows/grants continue fairly.
        let row=sqlx::query(r#"
WITH state AS (
 SELECT r.grant_id,r.member,g.admitted_at,
 EXISTS(SELECT 1 FROM payout_unsubmitted c WHERE c.grant_id=r.grant_id AND c.member=r.member) AS closed,
 EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_quote_intents q ON q.invoice_id=i.id JOIN payout_payment_bindings b ON b.intent_id=q.id JOIN payout_payment_records p ON p.payment_id=b.payment_id
 WHERE i.grant_id=r.grant_id AND i.member=r.member AND p.seq=(SELECT MAX(p2.seq) FROM payout_payment_records p2 WHERE p2.payment_id=b.payment_id) AND p.state IN ('COMPLETED','FAILED')) AS terminal,
 EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_quote_intents q ON q.invoice_id=i.id JOIN payout_payment_bindings b ON b.intent_id=q.id JOIN payout_payment_conflicts c ON c.payment_id=b.payment_id WHERE i.grant_id=r.grant_id AND i.member=r.member) AS conflicted
 FROM payout_rows r JOIN payout_grants g ON g.id=r.grant_id
), jobs AS (
 SELECT 'row:'||s.grant_id||':'||s.member AS work_key,s.grant_id,s.member,s.admitted_at FROM state s
 WHERE NOT s.closed AND NOT s.terminal AND NOT s.conflicted AND NOT EXISTS(SELECT 1 FROM payout_outcome_manifests m WHERE m.grant_id=s.grant_id)
 UNION ALL
 SELECT 'outcome:'||g.id,g.id,NULL,g.admitted_at FROM payout_grants g
 WHERE NOT EXISTS(SELECT 1 FROM payout_outcome_acks a WHERE a.grant_id=g.id)
 AND NOT EXISTS(SELECT 1 FROM state s WHERE s.grant_id=g.id AND (s.conflicted OR (NOT s.closed AND NOT s.terminal)))
)
SELECT j.* FROM jobs j LEFT JOIN payout_worker_attempts a ON a.work_key=j.work_key ORDER BY COALESCE(a.sequence,0),j.admitted_at,j.work_key LIMIT 1
"#).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        let old: i64 =
            sqlx::query_scalar("SELECT sequence FROM payout_worker_clock WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        let next = old.checked_add(1).context("worker sequence exhausted")?;
        sqlx::query("UPDATE payout_worker_clock SET sequence=? WHERE singleton=1")
            .bind(next)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO payout_worker_attempts(work_key,sequence) VALUES(?,?) ON CONFLICT(work_key) DO UPDATE SET sequence=excluded.sequence")
            .bind(row.try_get::<String,_>("work_key")?).bind(next).execute(&mut *tx).await?;
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
