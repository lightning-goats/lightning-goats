//! Accept exactly the append-only audited initial reward disposition, nothing else.
use super::{
    Manifest, Operator, Policy, Role, contact_upgrade, database, files,
    legacy::rows,
    reward_cutover::{Preview, derive_history},
};
use crate::cyberherd::{
    ingress::CoreBinding,
    reward_cutover::{COMMAND, PREFIX},
};
use anyhow::{Result, ensure};
use sqlx::{Row as _, SqlitePool};
use std::path::Path;

async fn read_preview(
    operator: &Operator,
    source: &Manifest,
    bundle: &Path,
    preview: &Path,
    expected: &str,
) -> Result<(Preview, Vec<u8>)> {
    files::private_bundle(preview)?;
    ensure!(
        std::fs::symlink_metadata(preview.join("INCOMPLETE"))
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "cutover preview is incomplete or inaccessible"
    );
    crate::nostr_event::validate_hex(expected, 32)?;
    let raw = files::read_private(&preview.join("PREVIEW.json"), 4 * 1024 * 1024 + 65536)?;
    ensure!(
        files::hash_bytes(&raw) == expected,
        "independently reviewed cutoff preview digest required"
    );
    let value: Preview = serde_json::from_slice(&raw)
        .map_err(|_| anyhow::anyhow!("invalid private cutoff preview"))?;
    value.decision.validate()?;
    ensure!(
        value.version == 1
            && value.id == value.decision.id
            && value.owner == operator.policy.owner
            && value.bundle_id == source.id
            && value.source_policy_sha256 == operator.policy_hash
            && value.decision.epoch == operator.policy.epoch
            && !value.cutover_applied
            && !value.activation_authorized
            && value.decision.decided_at >= source.created_at
            && value.decision.decided_at <= crate::cyberherd::calendar::now()?
            && value.decision.backup_manifest_sha256
                == files::hash_bytes(&files::read_private(&bundle.join("MANIFEST.json"), 65536)?),
        "cutoff preview differs from accepted source/scope"
    );
    ensure!(
        source.report.pending_feeds == 0
            && source.report.execution_claims == 0
            && source.report.unresolved_provider_payments == 0
            && source.report.payout_held_sats == 0,
        "initial cutoff source retains unresolved physical/financial work"
    );
    for (role, table) in [
        (Role::Core, "reward_reservations"),
        (Role::Payout, "payout_grants"),
        (Role::Signer, "signer_intents"),
    ] {
        ensure!(
            source.report.rows.get(&role).and_then(|r| r.get(table)) == Some(&0),
            "initial cutoff source retains financial/signing history"
        );
    }
    let core = rows::open_named(bundle, "core.db").await?;
    let member = match rows::open_named(bundle, "membership.db").await {
        Ok(p) => p,
        Err(e) => {
            core.close().await;
            return Err(e);
        }
    };
    let checked:Result<()>=async {
        let existing:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operator_commands WHERE owner=? AND command_id LIKE 'initial-reward-cutover%')")
            .bind(&operator.policy.owner).fetch_one(&member).await?;
        ensure!(!existing,"initial cutoff source already has a disposition");
        let conflict:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reward_plans WHERE owner=? AND (state!='no_payout' OR reservation_json IS NOT NULL))")
            .bind(&operator.policy.owner).fetch_one(&member).await?;
        ensure!(!conflict,"initial cutoff source retains nonterminal plans");
        let raw:String=sqlx::query_scalar("SELECT binding_json FROM core_streams WHERE owner=?").bind(&operator.policy.owner).fetch_one(&member).await?;
        let binding:CoreBinding=serde_json::from_str(&raw)?;
        ensure!(binding.epoch==operator.policy.epoch && binding.core_uid==operator.policy.components[&Role::Core].uid,"cutoff preview binding changed");
        ensure!(derive_history(&core,&binding,value.decision.through).await?==value.decision.historical,"preview omitted or changed core confirmations");
        Ok(())
    }.await;
    core.close().await;
    member.close().await;
    checked?;
    Ok((value, raw))
}

async fn compare_audits(
    old: &SqlitePool,
    new: &SqlitePool,
    columns: &[String],
    owner: &str,
    preview: &Preview,
) -> Result<()> {
    ensure!(
        columns
            .iter()
            .map(String::as_str)
            .eq(["owner", "command_id", "input_json", "result_json"]),
        "unknown operator audit schema"
    );
    let through: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(rowid),0) FROM operator_commands")
        .fetch_one(old)
        .await?;
    ensure!(through >= 0, "unsupported original audit row identity");
    let original = rows::digest(old, "operator_commands", columns).await?;
    ensure!(
        rows::audit_prefix_digest(new, columns, through).await? == original,
        "cutoff changed original audit bytes or row IDs"
    );
    let added = preview
        .decision
        .historical
        .len()
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("cutoff audit count overflow"))?;
    let after = rows::digest(new, "operator_commands", columns).await?;
    ensure!(
        after.rows == original.rows + u64::try_from(added)?,
        "unexpected extra/missing cutoff audit rows"
    );
    let result = serde_json::to_string(&preview.decision.receipt())?;
    let mut expected = vec![(
        COMMAND.to_owned(),
        serde_json::to_string(&preview.decision)?,
    )];
    for item in &preview.decision.historical {
        expected.push((
            format!("{PREFIX}{}", item.request_id),
            serde_json::to_string(item)?,
        ));
    }
    let actual=sqlx::query("SELECT rowid,owner,command_id,input_json,result_json,typeof(owner) AS owner_type,typeof(command_id) AS command_type,typeof(input_json) AS input_type,typeof(result_json) AS result_type FROM operator_commands WHERE rowid>? ORDER BY rowid")
        .bind(through).fetch_all(new).await?;
    ensure!(
        actual.len() == expected.len(),
        "cutoff audit append count differs"
    );
    for (offset, (row, (key, input))) in actual.iter().zip(expected).enumerate() {
        let id = through
            .checked_add(i64::try_from(offset)? + 1)
            .ok_or_else(|| anyhow::anyhow!("cutoff audit row ID overflow"))?;
        ensure!(
            row.try_get::<i64, _>("rowid")? == id
                && row.try_get::<String, _>("owner")? == owner
                && row.try_get::<String, _>("command_id")? == key
                && row.try_get::<String, _>("input_json")? == input
                && row.try_get::<String, _>("result_json")? == result,
            "cutoff appended unexpected audit identity/content"
        );
        for field in ["owner_type", "command_type", "input_type", "result_type"] {
            ensure!(
                row.try_get::<String, _>(field)? == "text",
                "cutoff audit storage class changed"
            );
        }
    }
    Ok(())
}

pub(super) async fn compare_images(
    before: &Path,
    after: &Path,
    policy: &Policy,
    preview: &Preview,
) -> Result<()> {
    database::inspect(before, policy).await?;
    database::inspect(after, policy).await?;
    for role in Role::ALL {
        let old = rows::open_named(before, &role.file()).await?;
        let new = match rows::open_named(after, &role.file()).await {
            Ok(p) => p,
            Err(e) => {
                old.close().await;
                return Err(e);
            }
        };
        let result: Result<()> = async {
            let migration_columns = rows::columns(&old, "_sqlx_migrations").await?;
            ensure!(
                rows::columns(&new, "_sqlx_migrations").await? == migration_columns
                    && rows::digest(&old, "_sqlx_migrations", &migration_columns).await?
                        == rows::digest(&new, "_sqlx_migrations", &migration_columns).await?,
                "cutoff changed original migration history"
            );
            let names = rows::names(&old).await?;
            ensure!(
                rows::names(&new).await? == names
                    && contact_upgrade::catalog(&old).await?
                        == contact_upgrade::catalog(&new).await?,
                "cutoff changed journal schema"
            );
            for table in names {
                let columns = rows::columns(&old, &table).await?;
                ensure!(
                    rows::columns(&new, &table).await? == columns,
                    "cutoff changed original columns"
                );
                if role == Role::Membership && table == "operator_commands" {
                    compare_audits(&old, &new, &columns, &policy.owner, preview).await?;
                } else {
                    ensure!(
                        rows::digest(&old, &table, &columns).await?
                            == rows::digest(&new, &table, &columns).await?,
                        "cutoff changed retained original rows"
                    );
                }
            }
            Ok(())
        }
        .await;
        old.close().await;
        new.close().await;
        result?;
    }
    Ok(())
}

impl Operator {
    /// Compare only a separate already-disposed four-journal copy. It does not
    /// apply a disposition, replace a live database or start/enable any worker.
    pub async fn compare_initial_reward_cutover(
        &self,
        bundle: &Path,
        preview: &Path,
        preview_sha256: &str,
        candidate: &Path,
    ) -> Result<Manifest> {
        ensure!(
            !candidate.starts_with(bundle)
                && !bundle.starts_with(candidate)
                && !candidate.starts_with(preview)
                && !preview.starts_with(candidate),
            "cutoff candidate must be separate from its source and preview"
        );
        let manifest = self.verify(bundle).await?;
        let (decision, raw) =
            read_preview(self, &manifest, bundle, preview, preview_sha256).await?;
        files::private_bundle(candidate)?;
        ensure!(
            std::fs::symlink_metadata(candidate.join("INCOMPLETE"))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "cutoff candidate is incomplete or inaccessible"
        );
        let mut images = Vec::new();
        for role in Role::ALL {
            for name in [role.file(), format!("{}-wal", role.file())] {
                match std::fs::symlink_metadata(candidate.join(&name)) {
                    Ok(_) => images.push(files::private_image(
                        &candidate.join(&name),
                        self.policy.max_component_bytes,
                    )?),
                    Err(e)
                        if e.kind() == std::io::ErrorKind::NotFound && name.ends_with("-wal") => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        files::check_images(candidate, &images, self.policy.max_component_bytes)?;
        compare_images(bundle, candidate, &self.policy, &decision).await?;
        files::check_images(candidate, &images, self.policy.max_component_bytes)?;
        ensure!(
            self.verify(bundle).await? == manifest
                && files::read_private(&preview.join("PREVIEW.json"), 4 * 1024 * 1024 + 65536)?
                    == raw,
            "cutoff comparison source changed"
        );
        Ok(manifest)
    }
}

#[cfg(test)]
#[path = "reward_cutover_compare_tests.rs"]
mod tests;
