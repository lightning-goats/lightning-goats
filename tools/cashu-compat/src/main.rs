//! Offline compatibility spike; no receive, swap, melt, or public listener.
#![forbid(unsafe_code)]

use std::{str::FromStr, time::Duration};

use anyhow::Result;
use cdk::{cdk_database::WalletDatabase, mint_url::MintUrl};
use cdk_sqlite::WalletSqliteDatabase;
use lightning_goats_cashu_compat::{
    mint_policy::MintPolicy,
    wallet::{build_wallet, open_wallet_storage},
};

#[tokio::main]
async fn main() -> Result<()> {
    anyhow::ensure!(rusqlite::version() == "3.54.1", "unqualified SQLite engine");
    let directory = tempfile::Builder::new()
        .permissions(<std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700))
        .tempdir()?;
    let path = directory.path().join("wallet.sqlite");
    let mint = MintUrl::from_str("https://mint.invalid")?;
    {
        let database = open_wallet_storage(directory.path()).await?;
        database.add_mint(mint.clone(), None).await?;
        // Synthetic seed, disposable database, and .invalid host. Never fund this wallet.
        let policy = MintPolicy {
            id: "offline-fixture".into(),
            advertised_url: mint.to_string(),
            policy_version: "fixture-v1".into(),
            receive_cashu: false,
            receive_nutzap: false,
            bolt11_exit_qualified: false,
        };
        let _wallet = build_wallet(&policy, database, [0; 64], Duration::from_secs(2), 1024)?;
    }
    let reopened = WalletSqliteDatabase::new(path).await?;
    anyhow::ensure!(reopened.get_mints().await?.contains_key(&mint));
    println!("Offline CDK wallet construction and SQLite mint metadata reopen passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk::{
        Amount,
        nuts::{CurrencyUnit, Id, Proof, PublicKey, State},
        secret::Secret,
        wallet::types::ProofInfo,
    };

    // Synthetic storage records only. These are not mint-issued money.
    fn proof(secret: &str, mint: &str) -> Result<ProofInfo> {
        let proof = Proof::new(
            Amount::from(8),
            Id::from_str("00deadbeef123456")?,
            Secret::new(secret),
            PublicKey::from_hex(
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )?,
        );
        Ok(ProofInfo::new(
            proof,
            MintUrl::from_str(mint)?,
            State::Unspent,
            CurrencyUnit::Sat,
        )?)
    }

    #[test]
    fn compiled_sqlite_engine_is_the_reviewed_version() {
        assert_eq!(rusqlite::version(), "3.54.1");
    }

    #[tokio::test]
    async fn reservations_and_counters_survive_reopen_and_reject_second_owner() -> Result<()> {
        let directory = tempfile::Builder::new()
            .permissions(
                <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
            )
            .tempdir()?;
        let path = directory.path().join("wallet.sqlite");
        let owner = cdk::util::hex::encode([1; 16]);
        // Use the wallet's exported operation-ID type via parsing.
        let owner = owner.parse()?;
        let other = "00000000-0000-0000-0000-000000000002".parse()?;
        let record = proof("storage-fixture-one", "https://one.invalid")?;
        let y = record.proof.y()?;
        {
            let db = WalletSqliteDatabase::new(path.clone()).await?;
            db.update_proofs(vec![record], vec![]).await?;
            db.reserve_proofs(vec![y], &owner).await?;
            assert!(db.reserve_proofs(vec![y], &other).await.is_err());
            assert_eq!(db.increment_derivation_counter("fixture", 3).await?, 3);
        }
        let db = WalletSqliteDatabase::new(path).await?;
        assert_eq!(db.get_reserved_proofs(&owner).await?.len(), 1);
        assert!(db.reserve_proofs(vec![y], &other).await.is_err());
        db.release_proofs(&other).await?;
        assert_eq!(db.get_reserved_proofs(&owner).await?.len(), 1);
        assert_eq!(db.increment_derivation_counter("fixture", 2).await?, 5);
        db.release_proofs(&owner).await?;
        db.reserve_proofs(vec![y], &other).await?;
        assert_eq!(db.get_reserved_proofs(&other).await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn corrupt_database_is_rejected_without_reinitializing_it() -> Result<()> {
        let directory = tempfile::Builder::new()
            .permissions(
                <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
            )
            .tempdir()?;
        let path = directory.path().join("wallet.sqlite");
        let corrupt = b"not a sqlite database and must never become an empty wallet";
        std::fs::write(&path, corrupt)?;
        assert!(WalletSqliteDatabase::new(path.clone()).await.is_err());
        assert_eq!(std::fs::read(path)?, corrupt);
        Ok(())
    }
}
