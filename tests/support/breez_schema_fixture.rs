//! Used only by disposable upgrade fixtures.
use anyhow::{Context, Result, ensure};

pub async fn strip_empty(transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<()> {
    let conversions: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='feed_conversion_proceeds')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    if conversions {
        let empty: bool =
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM feed_conversion_proceeds)")
                .fetch_one(&mut **transaction)
                .await?;
        ensure!(empty, "fixture contains confirmed conversion proceeds");
        sqlx::query("DROP TABLE feed_conversion_proceeds")
            .execute(&mut **transaction)
            .await?;
        let removed = sqlx::query("DELETE FROM _sqlx_migrations WHERE version=35")
            .execute(&mut **transaction)
            .await?;
        ensure!(
            removed.rows_affected() == 1,
            "missing conversion proceeds migration"
        );
    }
    let has_ranges: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cashu_funding_ranges')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    if has_ranges {
        // A populated Strike-era fixture can reconstruct these derived rows from
        // its retained source results. Refuse to strip any additional history.
        let recoverable: bool = sqlx::query_scalar("WITH legacy AS (
            SELECT g.ledger_id,g.intent_id,g.start_sats,g.input_sats,g.deposit_sats,r.change_sats
            FROM cashu_settled_ranges g JOIN cashu_treasury_source_results r USING(ledger_id,intent_id))
            SELECT NOT EXISTS(SELECT * FROM cashu_funding_ranges EXCEPT SELECT * FROM legacy)
               AND NOT EXISTS(SELECT * FROM legacy EXCEPT SELECT * FROM cashu_funding_ranges)")
            .fetch_one(&mut **transaction).await?;
        ensure!(recoverable, "fixture contains non-legacy settlement ranges");
        sqlx::raw_sql("DROP TRIGGER freeze_legacy_funding_range; DROP TRIGGER freeze_conversion_range_on_wallet;
            DROP TRIGGER freeze_conversion_range_on_proof; DROP TABLE cashu_funding_ranges;").execute(&mut **transaction).await?;
        let removed = sqlx::query("DELETE FROM _sqlx_migrations WHERE version=34")
            .execute(&mut **transaction)
            .await?;
        ensure!(
            removed.rows_affected() == 1,
            "missing funding range migration"
        );
    }
    let has_inspections: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='breez_treasury_inspections')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    if has_inspections {
        let empty: bool =
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM breez_treasury_inspections)")
                .fetch_one(&mut **transaction)
                .await?;
        ensure!(empty, "fixture contains conversion inspection history");
        sqlx::query("DROP TABLE breez_treasury_inspections")
            .execute(&mut **transaction)
            .await?;
        let removed = sqlx::query("DELETE FROM _sqlx_migrations WHERE version=33")
            .execute(&mut **transaction)
            .await?;
        ensure!(
            removed.rows_affected() == 1,
            "missing conversion inspection migration"
        );
    }
    let has_conversions: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='breez_treasury_wallet_results')")
        .fetch_one(&mut **transaction).await?;
    if has_conversions {
        let empty: bool =
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM breez_treasury_wallet_results)")
                .fetch_one(&mut **transaction)
                .await?;
        ensure!(empty, "fixture contains conversion settlement history");
        sqlx::raw_sql(
            "DROP VIEW breez_treasury_source_results; DROP TABLE breez_treasury_wallet_results;",
        )
        .execute(&mut **transaction)
        .await?;
        let removed = sqlx::query("DELETE FROM _sqlx_migrations WHERE version=32")
            .execute(&mut **transaction)
            .await?;
        ensure!(
            removed.rows_affected() == 1,
            "missing conversion fixture migration"
        );
    }
    let has_claims: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cashu_payout_execution_claims')")
        .fetch_one(&mut **transaction).await?;
    if has_claims {
        let empty: bool =
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM cashu_payout_execution_claims)")
                .fetch_one(&mut **transaction)
                .await?;
        ensure!(empty, "fixture contains Cashu payout execution history");
        sqlx::raw_sql("DROP TRIGGER strike_execution_provider_conflict; DROP TABLE cashu_payout_execution_claims;").execute(&mut **transaction).await?;
        let removed = sqlx::query("DELETE FROM _sqlx_migrations WHERE version=31")
            .execute(&mut **transaction)
            .await?;
        ensure!(
            removed.rows_affected() == 1,
            "missing Cashu execution fixture migration"
        );
    }
    let rows:i64=sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM retained_cashu_lots)+(SELECT COUNT(*) FROM breez_collected_claims)+(SELECT COUNT(*) FROM breez_project_invoices)").fetch_one(&mut **transaction).await?;
    ensure!(
        rows == 0,
        "fixture contains Breez invoice or custody history"
    );
    // Restore the original Strike-only receipt schema while retaining its rows.
    sqlx::raw_sql("DROP TRIGGER retained_cashu_no_strike_sweep; DROP TABLE retained_cashu_lots; DROP TRIGGER zap_receipt_requires_invoice;
        DROP TRIGGER strike_invoice_id_conflict; DROP TRIGGER strike_invoice_hash_conflict;
        DROP VIEW zap_invoice_terms; DROP TABLE breez_collected_claims; DROP TABLE breez_project_invoices;
        ALTER TABLE zap_receipt_relays RENAME TO fixture_relays;
        ALTER TABLE zap_receipt_jobs RENAME TO fixture_jobs;
        DROP TRIGGER zap_receipt_identity_immutable; DROP TRIGGER zap_receipt_signed_immutable;")
        .execute(&mut **transaction).await?;
    let schema = include_str!("../../migrations/0014_nip57_receive_context.sql");
    let start = schema
        .find("CREATE TABLE zap_receipt_jobs")
        .context("missing original receipt schema")?;
    sqlx::raw_sql(&schema[start..])
        .execute(&mut **transaction)
        .await?;
    sqlx::raw_sql(
        "INSERT INTO zap_receipt_jobs SELECT * FROM fixture_jobs;
        INSERT INTO zap_receipt_relays SELECT * FROM fixture_relays;
        DROP TABLE fixture_relays; DROP TABLE fixture_jobs;",
    )
    .execute(&mut **transaction)
    .await?;
    let result = sqlx::query("DELETE FROM _sqlx_migrations WHERE version=30")
        .execute(&mut **transaction)
        .await?;
    ensure!(result.rows_affected() == 1, "missing Breez fixture schema");
    Ok(())
}
