//! Explicitly provisioned replay identity, never completion authority.
use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use uuid::Uuid;

#[derive(Clone)]
pub struct OwnerWitness {
    pool: SqlitePool,
    generation: Uuid,
}

impl OwnerWitness {
    /// Open only an existing, explicitly migrated witness. No schema creation,
    /// repair, generation adoption or implicit initialization is permitted.
    pub async fn open(url: &str, generation: Uuid) -> Result<Self> {
        let options = crate::sqlite::durable_options(url)?.create_if_missing(false);
        let metadata = std::fs::symlink_metadata(options.get_filename())?;
        if !metadata.is_file() || metadata.len() == 0 {
            bail!("witness must be an existing nonempty regular file");
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
                        .fetch_one(&mut *connection)
                        .await?;
                    let sync: i64 = sqlx::query_scalar("PRAGMA synchronous")
                        .fetch_one(&mut *connection)
                        .await?;
                    if journal != "wal" || sync != 2 {
                        return Err(sqlx::Error::Protocol(
                            "witness durability unavailable".into(),
                        ));
                    }
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;
        let witness = Self { pool, generation };
        witness.identities().await?;
        Ok(witness)
    }

    async fn identities(&self) -> Result<Vec<(Uuid, bool)>> {
        let mut tx = self.pool.begin().await?;
        let metadata = sqlx::query("SELECT version, generation FROM owner_witness_metadata")
            .fetch_all(&mut *tx)
            .await?;
        if metadata.len() != 1
            || metadata[0].try_get::<i64, _>("version")? != 1
            || metadata[0].try_get::<String, _>("generation")? != self.generation.to_string()
        {
            bail!("witness generation/schema mismatch");
        }
        let rows = sqlx::query("SELECT request_id, gateway_required FROM owner_witness")
            .fetch_all(&mut *tx)
            .await?;
        let mut found = BTreeSet::new();
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            let raw: String = row.try_get("request_id")?;
            let id = Uuid::parse_str(&raw).context("invalid witness UUID")?;
            let required: i64 = row.try_get("gateway_required")?;
            if raw != id.to_string() || !found.insert(id) || ![0, 1].contains(&required) {
                bail!("invalid witness identity coverage");
            }
            result.push((id, required == 1));
        }
        tx.commit().await?;
        Ok(result)
    }

    pub(crate) async fn validate_coverage(&self, gateway: &[Uuid]) -> Result<()> {
        let required: BTreeSet<_> = self
            .identities()
            .await?
            .into_iter()
            .filter_map(|(id, required)| required.then_some(id))
            .collect();
        if required != gateway.iter().copied().collect() {
            bail!("gateway/witness coverage mismatch; explicit reconciliation required");
        }
        Ok(())
    }

    pub(crate) async fn contains(&self, id: Uuid) -> Result<bool> {
        Ok(self
            .identities()
            .await?
            .iter()
            .any(|(stored, _)| *stored == id))
    }

    /// Unique commit precedes any owner POST. A hit or error is not permission
    /// to retry, nor evidence of either completion or non-dispatch.
    pub(crate) async fn reserve(&self, id: Uuid) -> Result<bool> {
        self.identities().await?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let metadata: Vec<(i64, String)> =
            sqlx::query_as("SELECT version,generation FROM owner_witness_metadata")
                .fetch_all(&mut *tx)
                .await?;
        if metadata != vec![(1, self.generation.to_string())] {
            bail!("witness generation changed before reservation");
        }
        let inserted = sqlx::query("INSERT INTO owner_witness(request_id,gateway_required) VALUES (?,1) ON CONFLICT(request_id) DO NOTHING")
            .bind(id.to_string()).execute(&mut *tx).await?.rows_affected() == 1;
        tx.commit().await?;
        Ok(inserted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn fixture() -> (TempDir, String, Uuid) {
        let dir = TempDir::new().unwrap();
        let url = format!("sqlite://{}/witness.db", dir.path().display());
        let pool = crate::sqlite::connect_durable(&url, 1).await.unwrap();
        sqlx::query("CREATE TABLE owner_witness_metadata(version INTEGER NOT NULL, generation TEXT NOT NULL)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE owner_witness(request_id TEXT PRIMARY KEY, gateway_required INTEGER NOT NULL CHECK(gateway_required IN (0,1)))").execute(&pool).await.unwrap();
        let generation = Uuid::new_v4();
        sqlx::query("INSERT INTO owner_witness_metadata VALUES(1,?)")
            .bind(generation.to_string())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        (dir, url, generation)
    }

    #[tokio::test]
    async fn missing_empty_foreign_and_invalid_schema_never_initialize() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.db");
        let url = format!("sqlite://{}", path.display());
        assert!(OwnerWitness::open(&url, Uuid::new_v4()).await.is_err());
        assert!(!path.exists());
        std::fs::write(&path, []).unwrap();
        assert!(OwnerWitness::open(&url, Uuid::new_v4()).await.is_err());
        let (_dir, url, generation) = fixture().await;
        assert!(OwnerWitness::open(&url, Uuid::new_v4()).await.is_err());
        let witness = OwnerWitness::open(&url, generation).await.unwrap();
        sqlx::query("UPDATE owner_witness_metadata SET version=2")
            .execute(&witness.pool)
            .await
            .unwrap();
        assert!(witness.reserve(Uuid::new_v4()).await.is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM owner_witness")
                .fetch_one(&witness.pool)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn more_than_128_identities_survive_reopen_and_oldest_replay() {
        let (_dir, url, generation) = fixture().await;
        let witness = OwnerWitness::open(&url, generation).await.unwrap();
        let ids: Vec<_> = (0..160).map(|_| Uuid::new_v4()).collect();
        for id in &ids {
            assert!(witness.reserve(*id).await.unwrap());
        }
        witness.validate_coverage(&ids).await.unwrap();
        witness.pool.close().await;
        let reopened = OwnerWitness::open(&url, generation).await.unwrap();
        assert!(!reopened.reserve(ids[0]).await.unwrap());
        reopened.validate_coverage(&ids).await.unwrap();
        assert!(reopened.validate_coverage(&ids[..32]).await.is_err());
        let mut newer = ids.clone();
        newer.push(Uuid::new_v4());
        assert!(reopened.validate_coverage(&newer).await.is_err());
    }

    #[tokio::test]
    async fn concurrent_connections_cannot_reserve_same_identity_twice() {
        let (_dir, url, generation) = fixture().await;
        let a = OwnerWitness::open(&url, generation).await.unwrap();
        let b = OwnerWitness::open(&url, generation).await.unwrap();
        let id = Uuid::new_v4();
        let (a, b) = tokio::join!(a.reserve(id), b.reserve(id));
        assert_ne!(a.unwrap(), b.unwrap());
    }

    #[tokio::test]
    async fn owner_only_tombstone_blocks_reservation_without_claiming_gateway_history() {
        let (_dir, url, generation) = fixture().await;
        let witness = OwnerWitness::open(&url, generation).await.unwrap();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO owner_witness VALUES(?,0)")
            .bind(id.to_string())
            .execute(&witness.pool)
            .await
            .unwrap();
        witness.validate_coverage(&[]).await.unwrap();
        assert!(witness.contains(id).await.unwrap());
        assert!(!witness.reserve(id).await.unwrap());
        assert!(witness.validate_coverage(&[id]).await.is_err());
    }
}
