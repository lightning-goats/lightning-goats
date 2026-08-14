use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use serde_json::json;
use sqlx::Row;

use super::{
    LedgerStore, events::append_event_in_transaction, feed::feed_credit_in_transaction, to_i64,
    to_u64,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyPendingInvoice {
    pub payment_hash: String,
    pub checking_id: Option<String>,
    pub wallet_id: String,
    pub amount_sats: u64,
    pub created_at: Option<i64>,
    pub expiry_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyCutoverManifest {
    pub wallet_id: String,
    pub opening_credit_sats: u64,
    pub cutover_at: i64,
    pub snapshot_at: i64,
    pub pending_invoices: Vec<LegacyPendingInvoice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyManifestOutcome {
    Installed,
    AlreadyInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySettledInvoice {
    pub payment_hash: String,
    pub checking_id: Option<String>,
    pub wallet_id: String,
    pub amount_sats: u64,
    pub settled_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySettlementOutcome {
    Credited {
        amount_sats: u64,
        feed_credit_sats: u64,
    },
    AlreadyImported,
}

impl LedgerStore {
    pub async fn install_legacy_cutover_manifest(
        &self,
        manifest: &LegacyCutoverManifest,
    ) -> Result<LegacyManifestOutcome> {
        validate_manifest(manifest)?;
        let opening_credit = to_i64(manifest.opening_credit_sats, "legacy opening credit")?;
        let mut transaction = self.pool.begin().await?;

        if let Some(row) = sqlx::query(
            r#"
            SELECT legacy_wallet_id, amount_sats, cutover_at, snapshot_at
            FROM legacy_opening_credit
            WHERE singleton = 1
            "#,
        )
        .fetch_optional(&mut *transaction)
        .await?
        {
            let existing_wallet: String = row.try_get("legacy_wallet_id")?;
            let existing_amount: i64 = row.try_get("amount_sats")?;
            let existing_cutover: i64 = row.try_get("cutover_at")?;
            let existing_snapshot: i64 = row.try_get("snapshot_at")?;
            let existing_pending = load_pending_invoices(&mut transaction).await?;

            if existing_wallet == manifest.wallet_id
                && to_u64(existing_amount, "legacy opening credit")? == manifest.opening_credit_sats
                && existing_cutover == manifest.cutover_at
                && existing_snapshot == manifest.snapshot_at
                && pending_sets_equal(&existing_pending, &manifest.pending_invoices)
            {
                transaction.commit().await?;
                return Ok(LegacyManifestOutcome::AlreadyInstalled);
            }

            bail!("a different legacy LNbits cutover manifest is already installed");
        }

        let existing_imports: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_imports")
            .fetch_one(&mut *transaction)
            .await?;
        if existing_imports != 0 {
            bail!("legacy settlement imports exist before a cutover manifest was installed");
        }

        sqlx::query(
            r#"
            INSERT INTO legacy_opening_credit
                (singleton, legacy_wallet_id, amount_sats, cutover_at, snapshot_at)
            VALUES (1, ?, ?, ?, ?)
            "#,
        )
        .bind(&manifest.wallet_id)
        .bind(opening_credit)
        .bind(manifest.cutover_at)
        .bind(manifest.snapshot_at)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO ledger_entries (entry_type, source_key, delta_sats)
            VALUES ('LEGACY_OPENING_CREDIT', 'legacy:opening-credit', ?)
            "#,
        )
        .bind(opening_credit)
        .execute(&mut *transaction)
        .await?;

        for pending in &manifest.pending_invoices {
            sqlx::query(
                r#"
                INSERT INTO legacy_pending_invoices
                    (payment_hash, legacy_checking_id, legacy_wallet_id, amount_sats,
                     legacy_created_at, legacy_expiry_at, snapshot_at)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&pending.payment_hash)
            .bind(pending.checking_id.as_deref())
            .bind(&pending.wallet_id)
            .bind(to_i64(pending.amount_sats, "legacy pending amount")?)
            .bind(pending.created_at)
            .bind(pending.expiry_at)
            .bind(manifest.snapshot_at)
            .execute(&mut *transaction)
            .await?;
        }

        transaction.commit().await?;
        Ok(LegacyManifestOutcome::Installed)
    }

    pub async fn import_legacy_settlement(
        &self,
        settled: &LegacySettledInvoice,
    ) -> Result<LegacySettlementOutcome> {
        validate_hash(&settled.payment_hash)?;
        validate_wallet_id(&settled.wallet_id)?;
        validate_optional_checking_id(settled.checking_id.as_deref())?;
        if settled.amount_sats == 0 {
            bail!("legacy settled invoice amount must be greater than zero");
        }

        let mut transaction = self.pool.begin().await?;
        let opening =
            sqlx::query("SELECT legacy_wallet_id FROM legacy_opening_credit WHERE singleton = 1")
                .fetch_optional(&mut *transaction)
                .await?
                .context("legacy LNbits cutover manifest is not installed")?;
        let manifest_wallet: String = opening.try_get("legacy_wallet_id")?;
        if settled.wallet_id != manifest_wallet {
            bail!(
                "legacy settlement wallet {} does not match cutover wallet {}",
                settled.wallet_id,
                manifest_wallet
            );
        }

        let pending = sqlx::query(
            r#"
            SELECT legacy_checking_id, legacy_wallet_id, amount_sats,
                   legacy_created_at, imported_at
            FROM legacy_pending_invoices
            WHERE payment_hash = ?
            "#,
        )
        .bind(&settled.payment_hash)
        .fetch_optional(&mut *transaction)
        .await?
        .with_context(|| {
            format!(
                "payment hash {} was not in the stable pre-cutover pending-invoice allowlist",
                settled.payment_hash
            )
        })?;

        let expected_checking_id: Option<String> = pending.try_get("legacy_checking_id")?;
        let expected_wallet: String = pending.try_get("legacy_wallet_id")?;
        let expected_amount: i64 = pending.try_get("amount_sats")?;
        let legacy_created_at: Option<i64> = pending.try_get("legacy_created_at")?;
        let imported_at: Option<i64> = pending.try_get("imported_at")?;

        if expected_wallet != settled.wallet_id {
            bail!("legacy pending-invoice wallet does not match settlement wallet");
        }
        if to_u64(expected_amount, "legacy pending amount")? != settled.amount_sats {
            bail!(
                "legacy settlement amount does not match the cutover snapshot for payment {}",
                settled.payment_hash
            );
        }
        if let Some(expected) = expected_checking_id.as_deref() {
            if settled.checking_id.as_deref() != Some(expected) {
                bail!(
                    "legacy settlement checking_id does not match the cutover snapshot for payment {}",
                    settled.payment_hash
                );
            }
        }

        if imported_at.is_some() {
            verify_existing_import(&mut transaction, settled).await?;
            transaction.commit().await?;
            return Ok(LegacySettlementOutcome::AlreadyImported);
        }

        if let Some(row) =
            sqlx::query("SELECT credited_sats FROM settled_invoices WHERE payment_hash = ?")
                .bind(&settled.payment_hash)
                .fetch_optional(&mut *transaction)
                .await?
        {
            let credited_sats: i64 = row.try_get("credited_sats")?;
            if credited_sats > 0 {
                bail!(
                    "payment {} was already credited through the canonical CLN invoice path",
                    settled.payment_hash
                );
            }
        }

        sqlx::query(
            r#"
            INSERT INTO legacy_imports
                (payment_hash, legacy_checking_id, amount_sats, settled_at,
                 legacy_wallet_id, legacy_created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&settled.payment_hash)
        .bind(settled.checking_id.as_deref())
        .bind(to_i64(settled.amount_sats, "legacy settlement amount")?)
        .bind(settled.settled_at)
        .bind(&settled.wallet_id)
        .bind(legacy_created_at)
        .execute(&mut *transaction)
        .await?;

        let source_key = format!("legacy:{}", settled.payment_hash);
        sqlx::query(
            r#"
            INSERT INTO ledger_entries (entry_type, source_key, delta_sats)
            VALUES ('LEGACY_SETTLEMENT_IMPORT', ?, ?)
            "#,
        )
        .bind(source_key)
        .bind(to_i64(settled.amount_sats, "legacy settlement amount")?)
        .execute(&mut *transaction)
        .await?;

        let feed_credit_sats = feed_credit_in_transaction(&mut transaction).await?;
        append_event_in_transaction(
            &mut transaction,
            "payment_received",
            &json!({
                "source": "legacy_lnbits",
                "amount_sats": settled.amount_sats,
                "feed_credit_sats": feed_credit_sats
            }),
        )
        .await?;

        sqlx::query(
            "UPDATE legacy_pending_invoices SET imported_at = unixepoch() WHERE payment_hash = ? AND imported_at IS NULL",
        )
        .bind(&settled.payment_hash)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(LegacySettlementOutcome::Credited {
            amount_sats: settled.amount_sats,
            feed_credit_sats,
        })
    }
}

async fn load_pending_invoices(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<Vec<LegacyPendingInvoice>> {
    let rows = sqlx::query(
        r#"
        SELECT payment_hash, legacy_checking_id, legacy_wallet_id,
               amount_sats, legacy_created_at, legacy_expiry_at
        FROM legacy_pending_invoices
        ORDER BY payment_hash
        "#,
    )
    .fetch_all(&mut **transaction)
    .await?;

    rows.into_iter()
        .map(|row| {
            let amount: i64 = row.try_get("amount_sats")?;
            Ok(LegacyPendingInvoice {
                payment_hash: row.try_get("payment_hash")?,
                checking_id: row.try_get("legacy_checking_id")?,
                wallet_id: row.try_get("legacy_wallet_id")?,
                amount_sats: to_u64(amount, "legacy pending amount")?,
                created_at: row.try_get("legacy_created_at")?,
                expiry_at: row.try_get("legacy_expiry_at")?,
            })
        })
        .collect()
}

async fn verify_existing_import(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    settled: &LegacySettledInvoice,
) -> Result<()> {
    let row = sqlx::query(
        r#"
        SELECT legacy_checking_id, amount_sats, legacy_wallet_id
        FROM legacy_imports
        WHERE payment_hash = ?
        "#,
    )
    .bind(&settled.payment_hash)
    .fetch_optional(&mut **transaction)
    .await?
    .context("legacy pending invoice is marked imported but no import audit row exists")?;

    let checking_id: Option<String> = row.try_get("legacy_checking_id")?;
    let amount: i64 = row.try_get("amount_sats")?;
    let wallet_id: Option<String> = row.try_get("legacy_wallet_id")?;
    if checking_id != settled.checking_id
        || to_u64(amount, "legacy imported amount")? != settled.amount_sats
        || wallet_id.as_deref() != Some(settled.wallet_id.as_str())
    {
        bail!("existing legacy import does not match the requested settlement");
    }
    Ok(())
}

fn pending_sets_equal(left: &[LegacyPendingInvoice], right: &[LegacyPendingInvoice]) -> bool {
    let left: HashMap<_, _> = left
        .iter()
        .map(|invoice| (invoice.payment_hash.as_str(), invoice))
        .collect();
    let right: HashMap<_, _> = right
        .iter()
        .map(|invoice| (invoice.payment_hash.as_str(), invoice))
        .collect();
    left == right
}

fn validate_manifest(manifest: &LegacyCutoverManifest) -> Result<()> {
    validate_wallet_id(&manifest.wallet_id)?;
    if manifest.cutover_at <= 0 {
        bail!("legacy cutover_at must be a positive Unix timestamp");
    }
    if manifest.snapshot_at < manifest.cutover_at {
        bail!("legacy snapshot_at must be at or after cutover_at");
    }

    let mut hashes = HashSet::new();
    let mut checking_ids = HashSet::new();
    for pending in &manifest.pending_invoices {
        validate_hash(&pending.payment_hash)?;
        validate_wallet_id(&pending.wallet_id)?;
        validate_optional_checking_id(pending.checking_id.as_deref())?;
        if pending.wallet_id != manifest.wallet_id {
            bail!("all pending legacy invoices must belong to the cutover wallet");
        }
        if pending.amount_sats == 0 {
            bail!("pending legacy invoice amount must be greater than zero");
        }
        if pending
            .created_at
            .is_some_and(|created| created > manifest.snapshot_at)
        {
            bail!("pending legacy invoice was created after the stable cutover snapshot");
        }
        if let (Some(created), Some(expiry)) = (pending.created_at, pending.expiry_at) {
            if expiry < created {
                bail!("pending legacy invoice expiry precedes its creation time");
            }
        }
        if !hashes.insert(pending.payment_hash.as_str()) {
            bail!("duplicate payment hash in legacy cutover manifest");
        }
        if let Some(checking_id) = pending.checking_id.as_deref() {
            if !checking_ids.insert(checking_id) {
                bail!("duplicate checking_id in legacy cutover manifest");
            }
        }
    }
    Ok(())
}

fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || hash.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        bail!("legacy payment_hash must be canonical 64-character lowercase hex");
    }
    Ok(())
}

fn validate_wallet_id(wallet_id: &str) -> Result<()> {
    if wallet_id.trim().is_empty() || wallet_id.len() > 256 {
        bail!("legacy wallet_id must contain 1 to 256 characters");
    }
    Ok(())
}

fn validate_optional_checking_id(checking_id: Option<&str>) -> Result<()> {
    if let Some(checking_id) = checking_id {
        if checking_id.trim().is_empty() || checking_id.len() > 1024 {
            bail!("legacy checking_id must contain 1 to 1024 characters when present");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::ledger::{PaidInvoice, SettlementOutcome};

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        (directory, store)
    }

    fn hash(byte: &str) -> String {
        byte.repeat(64 / byte.len())
    }

    fn pending(payment_hash: String, amount_sats: u64) -> LegacyPendingInvoice {
        LegacyPendingInvoice {
            payment_hash,
            checking_id: Some("legacy-checking-1".to_owned()),
            wallet_id: "herd-wallet".to_owned(),
            amount_sats,
            created_at: Some(1_700_000_000),
            expiry_at: Some(1_700_003_600),
        }
    }

    fn manifest(pending_invoices: Vec<LegacyPendingInvoice>) -> LegacyCutoverManifest {
        LegacyCutoverManifest {
            wallet_id: "herd-wallet".to_owned(),
            opening_credit_sats: 620,
            cutover_at: 1_700_000_100,
            snapshot_at: 1_700_000_110,
            pending_invoices,
        }
    }

    #[tokio::test]
    async fn opening_manifest_is_atomic_idempotent_and_silent() {
        let (_directory, store) = store().await;
        let payment_hash = hash("ab");
        let manifest = manifest(vec![pending(payment_hash, 500)]);

        assert_eq!(
            store
                .install_legacy_cutover_manifest(&manifest)
                .await
                .unwrap(),
            LegacyManifestOutcome::Installed
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 620);
        assert!(store.events_after(0, 10).await.unwrap().is_empty());
        assert_eq!(
            store
                .install_legacy_cutover_manifest(&manifest)
                .await
                .unwrap(),
            LegacyManifestOutcome::AlreadyInstalled
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 620);
    }

    #[tokio::test]
    async fn a_changed_manifest_is_rejected_after_installation() {
        let (_directory, store) = store().await;
        let manifest = manifest(vec![]);
        store
            .install_legacy_cutover_manifest(&manifest)
            .await
            .unwrap();
        let mut changed = manifest;
        changed.opening_credit_sats += 1;
        assert!(
            store
                .install_legacy_cutover_manifest(&changed)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn unregistered_legacy_settlement_is_rejected() {
        let (_directory, store) = store().await;
        store
            .install_legacy_cutover_manifest(&manifest(vec![]))
            .await
            .unwrap();
        let settled = LegacySettledInvoice {
            payment_hash: hash("cd"),
            checking_id: Some("legacy-checking-2".to_owned()),
            wallet_id: "herd-wallet".to_owned(),
            amount_sats: 500,
            settled_at: Some(1_700_000_200),
        };
        assert!(store.import_legacy_settlement(&settled).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 620);
    }

    #[tokio::test]
    async fn late_settlement_credits_exactly_once_and_emits_one_event() {
        let (_directory, store) = store().await;
        let payment_hash = hash("ef");
        let pending = pending(payment_hash.clone(), 500);
        store
            .install_legacy_cutover_manifest(&manifest(vec![pending.clone()]))
            .await
            .unwrap();
        let settled = LegacySettledInvoice {
            payment_hash,
            checking_id: pending.checking_id,
            wallet_id: pending.wallet_id,
            amount_sats: 500,
            settled_at: Some(1_700_000_200),
        };

        assert_eq!(
            store.import_legacy_settlement(&settled).await.unwrap(),
            LegacySettlementOutcome::Credited {
                amount_sats: 500,
                feed_credit_sats: 1_120
            }
        );
        assert_eq!(
            store.import_legacy_settlement(&settled).await.unwrap(),
            LegacySettlementOutcome::AlreadyImported
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_120);
        let events = store.events_after(0, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "payment_received");
    }

    #[tokio::test]
    async fn cln_watcher_may_record_ignored_invoice_before_legacy_import() {
        let (_directory, store) = store().await;
        store.initialize_cursor(100).await.unwrap();
        let payment_hash = hash("12");
        let pending = pending(payment_hash.clone(), 500);
        store
            .install_legacy_cutover_manifest(&manifest(vec![pending.clone()]))
            .await
            .unwrap();

        let observed = PaidInvoice {
            pay_index: 101,
            payment_hash: payment_hash.clone(),
            label: Some("legacy-lnbits-label".to_owned()),
            amount_msat: 500_000,
            settled_at: Some(1_700_000_200),
        };
        assert_eq!(
            store.record_settlement(&observed, "herd").await.unwrap(),
            SettlementOutcome::Ignored
        );

        let settled = LegacySettledInvoice {
            payment_hash,
            checking_id: pending.checking_id,
            wallet_id: pending.wallet_id,
            amount_sats: 500,
            settled_at: Some(1_700_000_200),
        };
        assert!(matches!(
            store.import_legacy_settlement(&settled).await.unwrap(),
            LegacySettlementOutcome::Credited { .. }
        ));
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_120);
    }

    #[tokio::test]
    async fn canonical_cln_credit_collision_blocks_legacy_import() {
        let (_directory, store) = store().await;
        store.initialize_cursor(100).await.unwrap();
        let payment_hash = hash("34");
        let pending = pending(payment_hash.clone(), 500);
        store
            .install_legacy_cutover_manifest(&manifest(vec![pending.clone()]))
            .await
            .unwrap();

        let observed = PaidInvoice {
            pay_index: 101,
            payment_hash: payment_hash.clone(),
            label: Some("clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000".to_owned()),
            amount_msat: 500_000,
            settled_at: Some(1_700_000_200),
        };
        assert!(matches!(
            store.record_settlement(&observed, "herd").await.unwrap(),
            SettlementOutcome::Credited { .. }
        ));

        let settled = LegacySettledInvoice {
            payment_hash,
            checking_id: pending.checking_id,
            wallet_id: pending.wallet_id,
            amount_sats: 500,
            settled_at: Some(1_700_000_200),
        };
        assert!(store.import_legacy_settlement(&settled).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_120);
    }
}
