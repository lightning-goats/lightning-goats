//! Explicit root-reviewed failed-payment costs, never inferred refunds or delivery.
use super::{
    outcome::{Disposition, Header, Row},
    provider::{BindingRecord, PaymentState},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub version: u8,
    pub id: Uuid,
    pub epoch: Uuid,
    pub reservation_id: Uuid,
    pub owner: String,
    pub member: String,
    pub account_id: Uuid,
    pub manifest_hash: String,
    pub payment_id: Uuid,
    pub invoice_hash: String,
    pub gross_sats: u64,
    pub actual_cost_sats: u64,
    pub evidence_sha256: String,
    pub statement_reference: String,
    pub reason: String,
    pub reviewed_failed_payment_cost: bool,
    pub approved_at: i64,
    pub expires_at: i64,
}
impl Decision {
    pub fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_public_key(&self.member)?;
        for hash in [
            &self.manifest_hash,
            &self.invoice_hash,
            &self.evidence_sha256,
        ] {
            crate::nostr_event::validate_hex(hash, 32)?;
        }
        if self.version != 1
            || [
                self.id,
                self.epoch,
                self.reservation_id,
                self.account_id,
                self.payment_id,
            ]
            .iter()
            .any(Uuid::is_nil)
            || self.owner == self.member
            || self.gross_sats == 0
            || self.gross_sats > i64::MAX as u64
            || self.actual_cost_sats > self.gross_sats
            || !self.reviewed_failed_payment_cost
            || self.approved_at < 0
            || self.expires_at <= self.approved_at
            || self.expires_at.saturating_sub(self.approved_at) > 86400
        {
            bail!("invalid reviewed failed-payment cost decision");
        }
        for (text, max) in [(&self.statement_reference, 256), (&self.reason, 1024)] {
            if text.trim().is_empty() || text.len() > max || text.chars().any(char::is_control) {
                bail!("invalid private cost review explanation");
            }
        }
        Ok(())
    }
    pub(crate) fn matches(&self, header: &Header, row: &Row) -> Result<()> {
        self.validate()?;
        header.validate()?;
        let Disposition::Payment {
            binding_json,
            state,
            ..
        } = &row.disposition
        else {
            bail!("cost review requires an original failed execution");
        };
        let binding: BindingRecord = serde_json::from_str(binding_json)?;
        binding.quote.validate()?;
        if *state != PaymentState::Failed
            || self.epoch != header.reservation.epoch
            || self.reservation_id != header.reservation.id
            || self.owner != header.reservation.owner
            || self.member != row.member
            || self.account_id != header.scope.account_id
            || self.manifest_hash != header.final_hash
            || self.gross_sats != row.gross_sats
            || self.payment_id != binding.payment_id
            || self.invoice_hash != binding.quote.request.payment_hash
        {
            bail!("cost decision changed original verified execution scope");
        }
        Ok(())
    }
}
pub fn hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

/// Only protected root-owned approval AND matching protected evidence create it.
/// This is an operator financial attestation, not a provider refund proof.
pub struct ApprovedDecision(Decision);
impl ApprovedDecision {
    #[cfg(unix)]
    pub fn load(approval: &std::path::Path, evidence: &std::path::Path) -> Result<Self> {
        let raw = protected_bytes(approval, 8192)?;
        let decision: Decision = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid cost approval document"))?;
        decision.validate()?;
        let evidence = protected_bytes(evidence, 4 * 1024 * 1024)?;
        if evidence.is_empty() || hex::encode(Sha256::digest(&evidence)) != decision.evidence_sha256
        {
            bail!("reviewed evidence does not match protected approval");
        }
        Ok(Self(decision))
    }
    pub fn decision(&self) -> &Decision {
        &self.0
    }
    #[cfg(test)]
    pub(crate) fn fixture(decision: Decision) -> Self {
        decision.validate().unwrap();
        Self(decision)
    }
}
#[cfg(unix)]
fn protected_bytes(path: &std::path::Path, max: usize) -> Result<Vec<u8>> {
    use std::{io::Read as _, os::unix::fs::MetadataExt as _};
    crate::private_ipc::protected_config(path)?;
    let file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    if meta.mode() & 0o007 != 0 || meta.len() > max as u64 {
        bail!("financial review files must be private and bounded");
    }
    let mut bytes = vec![];
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        bail!("financial review file exceeds bound");
    }
    Ok(bytes)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub decision: Decision,
    pub header_hash: String,
    pub row_hash: String,
    pub verified_at: i64,
}
impl Receipt {
    pub fn validate(&self, header: &Header, row: &Row) -> Result<()> {
        self.decision.matches(header, row)?;
        if self.header_hash != hash(header)?
            || self.row_hash != hash(row)?
            || self.verified_at < self.decision.approved_at
            || self.verified_at > self.decision.expires_at
            || self.verified_at < header.reservation.created_at
        {
            bail!("invalid immutable failed-cost receipt");
        }
        Ok(())
    }
    pub fn released_sats(&self) -> u64 {
        self.decision.gross_sats - self.decision.actual_cost_sats
    }
}

#[cfg(unix)]
pub fn existing_private_database(database_url: &str) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let options = crate::sqlite::durable_options(database_url)?;
    let path = options.get_filename();
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("database has no parent"))?;
    crate::private_ipc::private_state(parent)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !path.is_absolute()
        || path.canonicalize()? != path
        || !meta.is_file()
        || meta.uid() != crate::private_ipc::uid()?
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        bail!(
            "recovery requires an existing private database owned by its non-admin runtime identity"
        );
    }
    Ok(())
}

#[cfg(unix)]
pub fn require_protected_configuration(path: &std::path::Path) -> Result<()> {
    crate::private_ipc::protected_config(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    fn sample() -> Decision {
        let now = crate::cyberherd::calendar::now().unwrap();
        Decision {
            version: 1,
            id: Uuid::new_v4(),
            epoch: Uuid::new_v4(),
            reservation_id: Uuid::new_v4(),
            owner: crate::nostr_event::test_event(1, 0, 100, "", vec![]).pubkey,
            member: crate::nostr_event::test_event(2, 0, 100, "", vec![]).pubkey,
            account_id: Uuid::new_v4(),
            manifest_hash: "11".repeat(32),
            payment_id: Uuid::new_v4(),
            invoice_hash: "22".repeat(32),
            gross_sats: 100,
            actual_cost_sats: 3,
            evidence_sha256: hex::encode(Sha256::digest(b"synthetic reviewed statement")),
            statement_reference: "Synthetic fixture only".into(),
            reason: "Reviewed original failed payment".into(),
            reviewed_failed_payment_cost: true,
            approved_at: now,
            expires_at: now + 3600,
        }
    }
    #[tokio::test]
    async fn root_owned_approval_and_evidence_checks_are_real_filesystem_authority() {
        if crate::private_ipc::uid().unwrap() != 0 {
            let root = tempfile::TempDir::new().unwrap();
            let path = root.path().join("decision.json");
            std::fs::write(&path, serde_json::to_vec(&sample()).unwrap()).unwrap();
            assert!(ApprovedDecision::load(&path, &path).is_err());
            return;
        }
        // Also invoked explicitly as root in an isolated CI network namespace.
        let root = tempfile::Builder::new()
            .prefix("lg-cost-review-")
            .tempdir_in("/run")
            .unwrap();
        let approval = root.path().join("decision.json");
        let evidence = root.path().join("statement.txt");
        std::fs::write(&approval, serde_json::to_vec(&sample()).unwrap()).unwrap();
        std::fs::write(&evidence, b"synthetic reviewed statement").unwrap();
        for path in [&approval, &evidence] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        assert!(ApprovedDecision::load(&approval, &evidence).is_ok());
        std::fs::write(&evidence, b"different evidence").unwrap();
        assert!(ApprovedDecision::load(&approval, &evidence).is_err());
        std::fs::write(&evidence, b"synthetic reviewed statement").unwrap();
        std::fs::set_permissions(&evidence, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(ApprovedDecision::load(&approval, &evidence).is_err());
        std::fs::set_permissions(&evidence, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::fs::set_permissions(&approval, std::fs::Permissions::from_mode(0o660)).unwrap();
        assert!(ApprovedDecision::load(&approval, &evidence).is_err());
        std::fs::set_permissions(&approval, std::fs::Permissions::from_mode(0o640)).unwrap();
        let alias = root.path().join("alias.json");
        std::os::unix::fs::symlink(&approval, &alias).unwrap();
        assert!(ApprovedDecision::load(&alias, &evidence).is_err());
        let mut raw = serde_json::to_value(sample()).unwrap();
        raw["api_key"] = serde_json::json!("synthetic-forbidden");
        std::fs::write(&approval, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(ApprovedDecision::load(&approval, &evidence).is_err());
        println!("ROOT_OWNERSHIP_FIXTURE_VERIFIED");
    }
    #[test]
    fn reviewed_cost_decision_rejects_implicit_waiver_bad_bounds_and_unsupported_fields() {
        let good = sample();
        good.validate().unwrap();
        let mut bad = good.clone();
        bad.reviewed_failed_payment_cost = false;
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.actual_cost_sats = 101;
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.expires_at = bad.approved_at + 86401;
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.reason = "".into();
        assert!(bad.validate().is_err());
        let mut raw = serde_json::to_value(good).unwrap();
        raw["execute_payment"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Decision>(raw).is_err());
    }
}

#[cfg(all(test, unix))]
mod recovery_schema_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    #[tokio::test]
    async fn recovery_never_initializes_a_missing_or_empty_database_or_upgrades_an_old_schema() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("core.db");
        let url = format!("sqlite://{}", path.display());
        assert!(
            crate::ledger::LedgerStore::connect_existing_recovery(&url)
                .await
                .is_err()
        );
        assert!(!path.exists());
        let empty = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            crate::ledger::LedgerStore::connect_existing_recovery(&url)
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
                .fetch_one(&empty)
                .await
                .unwrap(),
            0
        );
        empty.close().await;
        let _ledger = crate::ledger::LedgerStore::connect(&url).await.unwrap();
        if crate::private_ipc::uid().unwrap() == 0 {
            assert!(
                crate::ledger::LedgerStore::connect_existing_recovery(&url)
                    .await
                    .is_err()
            );
            return;
        }
        crate::ledger::LedgerStore::connect_existing_recovery(&url)
            .await
            .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            crate::ledger::LedgerStore::connect_existing_recovery(&url)
                .await
                .is_err()
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let alias = root.path().join("alias.db");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(existing_private_database(&format!("sqlite://{}", alias.display())).is_err());
        let db = sqlx::SqlitePool::connect(&url).await.unwrap();
        let tables: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
                .fetch_one(&db)
                .await
                .unwrap();
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version=(SELECT MAX(version) FROM _sqlx_migrations)").execute(&db).await.unwrap();
        assert!(
            crate::ledger::LedgerStore::connect_existing_recovery(&url)
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
                .fetch_one(&db)
                .await
                .unwrap(),
            tables
        );
    }
}
