//! Durable CDK namespaces. A pending namespace never becomes a blank restart.
use super::*;
use crate::mint_policy::MintPolicy;

impl Journal {
    pub(crate) fn require_cdk_namespaces(&mut self, legacy_cdk_empty: bool) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS cdk_storage_layout(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL CHECK(version=1));
            CREATE TABLE IF NOT EXISTS cdk_mint_namespaces(mint_id TEXT PRIMARY KEY,mint_url TEXT NOT NULL,ready INTEGER NOT NULL CHECK(ready IN (0,1)));
            CREATE TRIGGER IF NOT EXISTS cdk_namespace_identity BEFORE UPDATE ON cdk_mint_namespaces WHEN NEW.mint_id IS NOT OLD.mint_id OR NEW.mint_url IS NOT OLD.mint_url OR NEW.ready<OLD.ready BEGIN SELECT RAISE(ABORT,'CDK namespace is retained'); END;
            CREATE TRIGGER IF NOT EXISTS cdk_namespace_retained BEFORE DELETE ON cdk_mint_namespaces BEGIN SELECT RAISE(ABORT,'CDK namespace is retained'); END;")?;
        let version: Option<i64> = tx
            .query_row(
                "SELECT version FROM cdk_storage_layout WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(
            legacy_cdk_empty,
            "legacy shared CDK custody requires explicit migration and reconciliation"
        );
        if version.is_some() {
            let pending: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM cdk_mint_namespaces WHERE ready=0)",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                !pending,
                "incomplete CDK namespace initialization requires reconciliation"
            );
        }
        if version.is_none() {
            let work: i64 = tx.query_row("SELECT (SELECT COUNT(*) FROM acquisitions)+(SELECT COUNT(*) FROM treasury_instructions)", [], |r|r.get(0))?;
            ensure!(
                legacy_cdk_empty && work == 0,
                "legacy shared CDK custody requires explicit migration and reconciliation"
            );
            tx.execute("INSERT INTO cdk_storage_layout VALUES(1,1)", [])?;
        } else {
            ensure!(version == Some(1), "unrecognized CDK namespace layout");
        }
        tx.commit()?;
        Ok(())
    }

    /// True only for a newly retained namespace; existing pending initialization refuses.
    pub(crate) fn prepare_cdk_namespace(&mut self, policy: &MintPolicy) -> Result<bool> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row(
            "SELECT version FROM cdk_storage_layout WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(version == 1, "CDK namespace layout not registered");
        let retained: Option<(String, i64)> = tx
            .query_row(
                "SELECT mint_url,ready FROM cdk_mint_namespaces WHERE mint_id=?",
                [&policy.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((url, ready)) = retained {
            ensure!(
                url == policy.advertised_url && ready == 1,
                "retained mint namespace changed or needs initialization reconciliation"
            );
            tx.commit()?;
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO cdk_mint_namespaces VALUES(?,?,0)",
            params![policy.id, policy.advertised_url],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn complete_cdk_namespace(&mut self, policy: &MintPolicy) -> Result<()> {
        ensure!(
            self.connection.execute(
                "UPDATE cdk_mint_namespaces SET ready=1 WHERE mint_id=? AND mint_url=? AND ready=0",
                params![policy.id, policy.advertised_url],
            )? == 1,
            "CDK namespace initialization binding changed"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> MintPolicy {
        MintPolicy {
            id: "first".into(),
            advertised_url: "https://first.example".into(),
            policy_version: "v1".into(),
            receive_cashu: true,
            receive_nutzap: false,
            bolt11_exit_qualified: true,
        }
    }

    #[test]
    fn namespace_binding_is_retained_and_partial_initialization_fences_restart() -> Result<()> {
        let directory = crate::test_directory()?;
        let epoch = Uuid::new_v4();
        let path = directory.path().join("journal.sqlite");
        let mint = policy();
        let mut journal = Journal::open(&path, epoch, 16)?;
        assert!(journal.require_cdk_namespaces(false).is_err());
        journal.require_cdk_namespaces(true)?;
        assert!(journal.prepare_cdk_namespace(&mint)?);
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 16)?;
        assert!(journal.require_cdk_namespaces(true).is_err());
        assert!(journal.prepare_cdk_namespace(&mint).is_err());
        journal.complete_cdk_namespace(&mint)?;
        journal.require_cdk_namespaces(true)?;
        assert!(!journal.prepare_cdk_namespace(&mint)?);
        let mut changed = mint.clone();
        changed.advertised_url = "https://other.example".into();
        assert!(journal.prepare_cdk_namespace(&changed).is_err());
        assert!(
            journal
                .connection
                .execute("DELETE FROM cdk_mint_namespaces", [])
                .is_err()
        );
        assert!(
            journal
                .connection
                .execute("UPDATE cdk_mint_namespaces SET ready=0", [])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn old_financial_work_cannot_be_reclassified_as_empty_namespaced_custody() -> Result<()> {
        let directory = crate::test_directory()?;
        let mut journal =
            Journal::open(&directory.path().join("journal.sqlite"), Uuid::new_v4(), 16)?;
        let proofs = [crate::intake::tests::proof("old-financial-input".into())];
        journal.admit(&ValidatedIntake {
            mint_id: "first",
            policy_version: "v1",
            transport: Transport::Cashu,
            source_ref: "old-delivery",
            source_author: None,
            proofs: &proofs,
        })?;
        assert!(journal.require_cdk_namespaces(true).is_err());
        let count: i64 =
            journal
                .connection
                .query_row("SELECT COUNT(*) FROM acquisitions", [], |r| r.get(0))?;
        assert_eq!(count, 1);
        Ok(())
    }
}
