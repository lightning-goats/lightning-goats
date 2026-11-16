//! Durable local CDK cleanup checkpoint, separate from financial completion.
use super::*;
use crate::{melt_reconciliation::RecoveryContext, treasury::TreasuryBinding};
use cdk::nuts::{MeltQuoteBolt11Response, MeltRequest};

pub(crate) struct Context {
    pub recovery: RecoveryContext,
    pub request: MeltRequest<String>,
    pub paid: MeltQuoteBolt11Response<String>,
    pub report_digest: [u8; 32],
}
impl Journal {
    pub(crate) fn treasury_cleanup_context(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
    ) -> Result<Option<Context>> {
        ensure!(
            self.completed_treasury_report(binding, id)?.is_some(),
            "cleanup requires completed financial report"
        );
        let recovery = self.treasury_recovery_context(binding, id)?;
        let (report,paid,request):(Vec<u8>,Vec<u8>,Vec<u8>)=self.connection.query_row(
            "SELECT r.report,r.paid_evidence,m.request FROM treasury_results r JOIN treasury_melt_requests m ON m.intent_id=r.intent_id WHERE r.intent_id=? AND length(r.report)<=65536 AND length(r.paid_evidence)<=1048576 AND length(m.request)<=1048576",
            [id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let report_digest: [u8; 32] = Sha256::digest(&report).into();
        let checkpoint: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT operation_id,report_digest FROM treasury_cdk_cleanup WHERE intent_id=?",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((operation, digest)) = checkpoint {
            ensure!(
                operation == recovery.derivation.cdk_operation.to_string()
                    && digest == report_digest,
                "CDK cleanup checkpoint differs from original report"
            );
            return Ok(None);
        }
        let paid = serde_json::from_slice(&paid)?;
        crate::melt_reconciliation::validate_paid_quote(&recovery.issued, &recovery.quote, &paid)?;
        let request: MeltRequest<String> = serde_json::from_slice(&request)?;
        ensure!(
            request.quote_id() == &recovery.quote.id
                && !request.inputs().is_empty()
                && request.inputs().len() <= 1024
                && request.outputs().clone().unwrap_or_default() == recovery.derivation.outputs,
            "invalid original cleanup request"
        );
        Ok(Some(Context {
            recovery,
            request,
            paid,
            report_digest,
        }))
    }
    pub(crate) fn record_treasury_cleanup(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        operation: Uuid,
        digest: [u8; 32],
    ) -> Result<()> {
        self.treasury_instruction(binding, id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (report,retained):(Vec<u8>,String)=tx.query_row(
            "SELECT r.report,p.operation_id FROM treasury_results r JOIN treasury_instructions i ON i.id=r.intent_id JOIN treasury_prepared p ON p.intent_id=r.intent_id WHERE r.intent_id=? AND i.state='reconciled' AND length(r.report)<=65536",
            [id.to_string()],|r|Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(
            <[u8; 32]>::from(Sha256::digest(report)) == digest && retained == operation.to_string(),
            "financial report changed before cleanup checkpoint"
        );
        tx.execute("INSERT INTO treasury_cdk_cleanup(intent_id,operation_id,report_digest) VALUES(?,?,?) ON CONFLICT(intent_id) DO NOTHING",
            params![id.to_string(),operation.to_string(),digest.as_slice()])?;
        let stored: (String, Vec<u8>) = tx.query_row(
            "SELECT operation_id,report_digest FROM treasury_cdk_cleanup WHERE intent_id=?",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            stored.0 == operation.to_string() && stored.1 == digest,
            "conflicting cleanup checkpoint"
        );
        tx.commit()?;
        Ok(())
    }
}
