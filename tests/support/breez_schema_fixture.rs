//! Used only by disposable upgrade fixtures.
use anyhow::{Context, Result, ensure};

pub async fn strip_empty(transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<()> {
    let rows:i64=sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM breez_collected_claims)+(SELECT COUNT(*) FROM breez_project_invoices)").fetch_one(&mut **transaction).await?;
    ensure!(
        rows == 0,
        "fixture contains Breez invoice or custody history"
    );
    // Restore the original Strike-only receipt schema while retaining its rows.
    sqlx::raw_sql("DROP TRIGGER zap_receipt_requires_invoice;
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
