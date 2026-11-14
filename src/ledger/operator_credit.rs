use anyhow::{Result, bail};
use serde_json::json;
use sqlx::Row;

use super::{
    LedgerStore, events::append_event_in_transaction, feed::feed_credit_in_transaction, to_i64,
};

#[derive(Debug, Clone)]
pub struct OperatorCreditGrant {
    pub id: String,
    pub amount_sats: u64,
    pub address_user: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorCreditStatus {
    Preview,
    Granted,
    Duplicate,
}

impl OperatorCreditStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Granted => "granted",
            Self::Duplicate => "duplicate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorCreditResult {
    pub status: OperatorCreditStatus,
    pub feed_credit_sats: u64,
}

impl OperatorCreditGrant {
    fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            bail!(
                "grant id must contain 1 to 64 ASCII letters, digits, dots, hyphens or underscores"
            );
        }
        if self.amount_sats == 0 {
            bail!("operator credit must be positive");
        }
        to_i64(self.amount_sats, "operator amount_sats")?;
        super::validate_user(&self.address_user, "address_user")?;
        if self.reason.trim().is_empty()
            || self.reason.len() > 256
            || self.reason.chars().any(char::is_control)
        {
            bail!("reason must contain 1 to 256 bytes of non-control text");
        }
        Ok(())
    }
}

impl LedgerStore {
    /// Local operator capability only: never exposed as a public HTTP endpoint.
    /// Grant, ledger credit and presentation event commit atomically. A repeated
    /// ID with identical data is a no-op even after its credit has been consumed.
    pub async fn grant_operator_credit(
        &self,
        grant: &OperatorCreditGrant,
        dry_run: bool,
    ) -> Result<OperatorCreditResult> {
        grant.validate()?;
        let amount = to_i64(grant.amount_sats, "operator amount_sats")?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query("SELECT g.amount_sats,g.address_user,g.reason,l.entry_type,l.source_key,l.delta_sats,l.address_user AS ledger_user,l.credit_pool,e.event_type,e.payload_json FROM operator_credit_grants g JOIN ledger_entries l ON l.id=g.ledger_entry_id JOIN event_log e ON e.seq=g.event_seq WHERE g.grant_id=?")
            .bind(&grant.id).fetch_optional(&mut *tx).await?;
        let before = feed_credit_in_transaction(&mut tx).await?;
        if let Some(row) = existing {
            let expected_key = format!("operator-credit:{}", grant.id);
            let encoded: String = row.try_get("payload_json")?;
            let payload: serde_json::Value = serde_json::from_str(&encoded)?;
            if row.try_get::<i64, _>("amount_sats")? != amount
                || row.try_get::<String, _>("address_user")? != grant.address_user
                || row.try_get::<String, _>("reason")? != grant.reason
            {
                bail!("operator grant ID already exists with conflicting data");
            }
            if row.try_get::<String, _>("entry_type")? != "OPERATOR_CREDIT"
                || row.try_get::<String, _>("source_key")? != expected_key
                || row.try_get::<i64, _>("delta_sats")? != amount
                || row.try_get::<String, _>("ledger_user")? != grant.address_user
                || row.try_get::<String, _>("credit_pool")? != "herd"
                || row.try_get::<String, _>("event_type")? != "operator_credit_added"
                || payload["grant_id"] != grant.id
                || payload["amount_sats"] != grant.amount_sats
                || payload["address_user"] != grant.address_user
                || payload["credit_pool"] != "herd"
                || payload["source"] != "operator_credit"
            {
                bail!("operator grant audit linkage is inconsistent");
            }
            tx.commit().await?;
            return Ok(OperatorCreditResult {
                status: OperatorCreditStatus::Duplicate,
                feed_credit_sats: before,
            });
        }
        let after = super::feed_cap::capped_total(&mut tx, before, grant.amount_sats).await?;
        to_i64(after, "resulting feed credit")?;
        if dry_run {
            tx.commit().await?;
            return Ok(OperatorCreditResult {
                status: OperatorCreditStatus::Preview,
                feed_credit_sats: after,
            });
        }
        let ledger_id = sqlx::query("INSERT INTO ledger_entries (entry_type,source_key,delta_sats,address_user,credit_pool) VALUES ('OPERATOR_CREDIT',?,?,?,'herd')")
            .bind(format!("operator-credit:{}",grant.id)).bind(amount).bind(&grant.address_user)
            .execute(&mut *tx).await?.last_insert_rowid();
        let after = super::feed_cap::cap_new_credit(&mut tx, ledger_id, grant.amount_sats).await?;
        let seq = append_event_in_transaction(
            &mut tx,
            "operator_credit_added",
            &json!({
                "source":"operator_credit", "grant_id":grant.id, "address_user":grant.address_user,
                "credit_pool":"herd", "amount_sats":grant.amount_sats, "feed_credit_sats":after
            }),
        )
        .await?;
        sqlx::query("INSERT INTO operator_credit_grants (grant_id,amount_sats,address_user,credit_pool,reason,ledger_entry_id,event_seq) VALUES (?,?,?,'herd',?,?,?)")
            .bind(&grant.id).bind(amount).bind(&grant.address_user).bind(&grant.reason).bind(ledger_id).bind(to_i64(seq,"event sequence")?)
            .execute(&mut *tx).await?;
        super::reward_funding::prepare(&mut tx).await?;
        tx.commit().await?;
        Ok(OperatorCreditResult {
            status: OperatorCreditStatus::Granted,
            feed_credit_sats: after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn grant(id: &str, sats: u64) -> OperatorCreditGrant {
        OperatorCreditGrant {
            id: id.into(),
            amount_sats: sats,
            address_user: "herd".into(),
            reason: "Operator feeder check".into(),
        }
    }
    async fn store() -> (TempDir, String, LedgerStore) {
        let root = TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("ledger.db").display());
        let store = LedgerStore::connect(&url).await.unwrap();
        (root, url, store)
    }
    async fn count(store: &LedgerStore, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn grant_is_atomic_not_a_settlement_and_retry_survives_consumption_and_restart() {
        let (_root, url, store) = store().await;
        let grant = grant("obs-001", 2340);
        assert_eq!(
            store
                .grant_operator_credit(&grant, false)
                .await
                .unwrap()
                .status,
            OperatorCreditStatus::Granted
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 2340);
        assert_eq!(count(&store, "operator_credit_grants").await, 1);
        assert_eq!(count(&store, "ledger_entries").await, 1);
        assert_eq!(count(&store, "event_log").await, 1);
        assert_eq!(count(&store, "settled_payments").await, 0);
        assert_eq!(count(&store, "asset_receipts").await, 0);
        for _ in 0..2 {
            let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
            store.confirm_feed_attempt(id).await.unwrap();
        }
        drop(store);
        let reopened = LedgerStore::connect(&url).await.unwrap();
        let retry = reopened.grant_operator_credit(&grant, false).await.unwrap();
        assert_eq!(retry.status, OperatorCreditStatus::Duplicate);
        assert_eq!(retry.feed_credit_sats, 340);
        assert_eq!(count(&reopened, "operator_credit_grants").await, 1);
        assert_eq!(count(&reopened, "event_log").await, 3);
    }

    #[tokio::test]
    async fn duplicate_id_conflicts_fail_without_mutation() {
        let (_root, _url, store) = store().await;
        store
            .grant_operator_credit(&grant("same", 1000), false)
            .await
            .unwrap();
        for changed in [
            OperatorCreditGrant {
                amount_sats: 1001,
                ..grant("same", 1000)
            },
            OperatorCreditGrant {
                address_user: "dexter".into(),
                ..grant("same", 1000)
            },
            OperatorCreditGrant {
                reason: "Changed".into(),
                ..grant("same", 1000)
            },
        ] {
            assert!(store.grant_operator_credit(&changed, false).await.is_err());
        }
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(count(&store, "event_log").await, 1);
    }

    #[tokio::test]
    async fn concurrent_same_id_from_separate_connections_credits_once() {
        let (_root, url, first) = store().await;
        let second = LedgerStore::connect(&url).await.unwrap();
        let a = grant("concurrent", 1000);
        let b = a.clone();
        let (one, two) = tokio::join!(
            first.grant_operator_credit(&a, false),
            second.grant_operator_credit(&b, false)
        );
        let statuses = [one.unwrap().status, two.unwrap().status];
        assert_eq!(
            statuses
                .iter()
                .filter(|s| **s == OperatorCreditStatus::Granted)
                .count(),
            1
        );
        assert_eq!(
            statuses
                .iter()
                .filter(|s| **s == OperatorCreditStatus::Duplicate)
                .count(),
            1
        );
        assert_eq!(first.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(count(&first, "event_log").await, 1);
    }

    #[tokio::test]
    async fn preview_invalid_input_and_late_write_failure_leave_no_credit_or_event() {
        let (_root, _url, store) = store().await;
        let preview = store
            .grant_operator_credit(&grant("preview", 1000), true)
            .await
            .unwrap();
        assert_eq!(preview.status, OperatorCreditStatus::Preview);
        assert_eq!(preview.feed_credit_sats, 1000);
        for bad in [
            grant("zero", 0),
            grant("overflow", u64::MAX),
            grant("bad id", 10),
            OperatorCreditGrant {
                reason: "\n".into(),
                ..grant("reason", 10)
            },
        ] {
            assert!(store.grant_operator_credit(&bad, false).await.is_err());
        }
        sqlx::query("CREATE TRIGGER fail_grant BEFORE INSERT ON operator_credit_grants BEGIN SELECT RAISE(ABORT,'synthetic late grant failure'); END")
            .execute(&store.pool).await.unwrap();
        assert!(
            store
                .grant_operator_credit(&grant("late-failure", 1000), false)
                .await
                .is_err()
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
        assert_eq!(count(&store, "ledger_entries").await, 0);
        assert_eq!(count(&store, "event_log").await, 0);
        assert_eq!(count(&store, "operator_credit_grants").await, 0);
    }
}
