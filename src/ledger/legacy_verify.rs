use anyhow::{Context, Result, bail};
use sqlx::Row;

use super::{LedgerStore, LegacyCutoverManifest, to_u64};

impl LedgerStore {
    /// Verify that the supplied cutover manifest is exactly the manifest already
    /// installed in SQLite. This method never installs or mutates migration state.
    pub async fn verify_legacy_cutover_manifest(
        &self,
        manifest: &LegacyCutoverManifest,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        let opening = sqlx::query(
            r#"
            SELECT legacy_wallet_id, amount_sats, cutover_at, snapshot_at
            FROM legacy_opening_credit
            WHERE singleton = 1
            "#,
        )
        .fetch_optional(&mut *transaction)
        .await?
        .context("legacy LNbits cutover manifest is not installed")?;

        let wallet_id: String = opening.try_get("legacy_wallet_id")?;
        let amount_sats: i64 = opening.try_get("amount_sats")?;
        let cutover_at: i64 = opening.try_get("cutover_at")?;
        let snapshot_at: i64 = opening.try_get("snapshot_at")?;
        if wallet_id != manifest.wallet_id
            || to_u64(amount_sats, "legacy opening credit")? != manifest.opening_credit_sats
            || cutover_at != manifest.cutover_at
            || snapshot_at != manifest.snapshot_at
        {
            bail!("supplied legacy manifest does not match installed opening-credit boundary");
        }

        let database_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM legacy_pending_invoices")
                .fetch_one(&mut *transaction)
                .await?;
        if usize::try_from(database_count).context("legacy pending count is invalid")?
            != manifest.pending_invoices.len()
        {
            bail!("supplied legacy manifest pending-invoice count does not match SQLite");
        }

        for pending in &manifest.pending_invoices {
            let row = sqlx::query(
                r#"
                SELECT legacy_checking_id, legacy_wallet_id, amount_sats,
                       legacy_created_at, legacy_expiry_at, snapshot_at
                FROM legacy_pending_invoices
                WHERE payment_hash = ?
                "#,
            )
            .bind(&pending.payment_hash)
            .fetch_optional(&mut *transaction)
            .await?
            .with_context(|| {
                format!(
                    "legacy pending invoice {} is not installed in SQLite",
                    pending.payment_hash
                )
            })?;

            let checking_id: Option<String> = row.try_get("legacy_checking_id")?;
            let pending_wallet: String = row.try_get("legacy_wallet_id")?;
            let pending_amount: i64 = row.try_get("amount_sats")?;
            let created_at: Option<i64> = row.try_get("legacy_created_at")?;
            let expiry_at: Option<i64> = row.try_get("legacy_expiry_at")?;
            let pending_snapshot_at: i64 = row.try_get("snapshot_at")?;

            if checking_id != pending.checking_id
                || pending_wallet != pending.wallet_id
                || to_u64(pending_amount, "legacy pending amount")? != pending.amount_sats
                || created_at != pending.created_at
                || expiry_at != pending.expiry_at
                || pending_snapshot_at != manifest.snapshot_at
            {
                bail!(
                    "legacy pending invoice {} does not match installed SQLite state",
                    pending.payment_hash
                );
            }
        }

        transaction.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::ledger::{LegacyPendingInvoice, LegacyManifestOutcome};

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        (directory, store)
    }

    fn manifest() -> LegacyCutoverManifest {
        LegacyCutoverManifest {
            wallet_id: "herd-wallet".to_owned(),
            opening_credit_sats: 340,
            cutover_at: 1_700_000_100,
            snapshot_at: 1_700_000_110,
            pending_invoices: vec![LegacyPendingInvoice {
                payment_hash: "ab".repeat(32),
                checking_id: Some("checking".to_owned()),
                wallet_id: "herd-wallet".to_owned(),
                amount_sats: 500,
                created_at: None,
                expiry_at: None,
            }],
        }
    }

    #[tokio::test]
    async fn verification_requires_prior_exact_installation() {
        let (_directory, store) = store().await;
        let manifest = manifest();
        assert!(store.verify_legacy_cutover_manifest(&manifest).await.is_err());
        assert_eq!(
            store
                .install_legacy_cutover_manifest(&manifest)
                .await
                .unwrap(),
            LegacyManifestOutcome::Installed
        );
        store.verify_legacy_cutover_manifest(&manifest).await.unwrap();

        let mut changed = manifest;
        changed.opening_credit_sats += 1;
        assert!(store.verify_legacy_cutover_manifest(&changed).await.is_err());
    }
}
