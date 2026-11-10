use super::{Store, bounded_roster, decode_snapshot, load_catalog};
use crate::cyberherd::{
    conversation::MemberConversation,
    http_auth::{Authorization, OperatorAuthorization},
    membership::{FeederAvailability, Member, Profile, Settings},
    operator::{self, Action, Command, Notice, Receipt},
};
use anyhow::{Context as _, Result, bail};
use sqlx::{Row as _, Sqlite, Transaction};

async fn consume(tx: &mut Transaction<'_, Sqlite>, auth: &Authorization, now: i64) -> Result<()> {
    if now < 0 || auth.expires <= now {
        return Err(crate::cyberherd::http_auth::Rejected.into());
    }
    sqlx::query("DELETE FROM http_auth_nonces WHERE expires<=?")
        .bind(now)
        .execute(&mut **tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM http_auth_nonces")
        .fetch_one(&mut **tx)
        .await?;
    if count >= 4096 {
        bail!("HTTP authentication capacity reached");
    }
    let inserted = sqlx::query(
        "INSERT INTO http_auth_nonces(id,expires) VALUES(?,?) ON CONFLICT(id) DO NOTHING",
    )
    .bind(&auth.id)
    .bind(auth.expires)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if inserted != 1 {
        return Err(crate::cyberherd::http_auth::Rejected.into());
    }
    Ok(())
}
impl Store {
    pub(crate) async fn consume_http_auth(&self, auth: &Authorization, now: i64) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        consume(&mut tx, auth, now).await?;
        tx.commit().await?;
        Ok(())
    }
    pub(crate) async fn operator_recorded(
        &self,
        owner: &str,
        authorization: &OperatorAuthorization,
        command: &Command,
    ) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operator_http_audit WHERE owner=? AND actor=? AND command_id=?)")
            .bind(owner).bind(&authorization.authorization().actor).bind(command.id.to_string()).fetch_one(&self.pool).await?)
    }
    pub(crate) async fn operator_command(
        &self,
        owner: &str,
        authorization: &OperatorAuthorization,
        command: &Command,
        environment: operator::Environment,
    ) -> Result<Receipt> {
        let operator::Environment {
            feeder,
            context,
            ordering,
            now,
        } = environment;
        let auth = authorization.authorization();
        command.validate()?;
        context.validate(owner)?;
        let input = serde_json::to_string(command)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        consume(&mut tx, auth, now).await?;
        if let Some(row)=sqlx::query("SELECT body_hash,input_json,result_json FROM operator_http_audit WHERE owner=? AND actor=? AND command_id=?")
            .bind(owner).bind(&auth.actor).bind(command.id.to_string()).fetch_optional(&mut *tx).await? {
            if row.try_get::<String,_>("input_json")?!=input || row.try_get::<String,_>("body_hash")?!=auth.body_hash {bail!("operator command identity changed");}
            let receipt=serde_json::from_str(&row.try_get::<String,_>("result_json")?)?;
            tx.commit().await?;return Ok(receipt);
        }
        if !matches!(command.action, Action::Template { .. }) {
            let row =
                sqlx::query("SELECT binding_json,received_through FROM core_streams WHERE owner=?")
                    .bind(owner)
                    .fetch_one(&mut *tx)
                    .await?;
            let cursor = crate::cyberherd::ingress::CoreCursor {
                binding: serde_json::from_str(&row.try_get::<String, _>("binding_json")?)?,
                received_through: u64::try_from(row.try_get::<i64, _>("received_through")?)?,
            };
            ordering
                .as_ref()
                .context("fresh core ordering observation required")?
                .validate(&cursor, now)?;
            let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ingress WHERE owner=? AND kind='feeding' AND state='pending')").bind(owner).fetch_one(&mut *tx).await?;
            if pending {
                bail!("operator edit awaits confirmed feeding snapshot");
            }
        }
        let mut snapshot = decode_snapshot(
            &sqlx::query("SELECT * FROM owners WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?,
        )?;
        if snapshot.revision != command.expected_revision {
            bail!("operator revision changed; refresh required");
        }
        let previous = snapshot.clone();
        let before_shares = snapshot.roster.shares(&snapshot.settings)?;
        let mut notice: Option<(String, Profile, &str, bool)> = None;
        match &command.action {
            Action::Settings {
                minimum_sats,
                max_members,
                member_percent,
            } => {
                if snapshot.roster.active_count() > usize::from(*max_members) {
                    bail!("capacity cannot evict active members implicitly");
                }
                snapshot.settings = Settings {
                    minimum_sats: operator::sats(minimum_sats)?,
                    max_members: *max_members,
                    member_percent: *member_percent,
                };
            }
            Action::Activate { pubkey, profile } => {
                if feeder != FeederAvailability::Ready {
                    bail!("activation requires confirmed unpaused feeder status");
                }
                if snapshot.roster.banned.contains(pubkey) {
                    bail!("member must be explicitly unbanned first");
                }
                let active = snapshot
                    .roster
                    .members
                    .get(pubkey)
                    .is_some_and(|m| m.active);
                if !active
                    && snapshot.roster.active_count() >= usize::from(snapshot.settings.max_members)
                {
                    bail!("operator activation exceeds capacity");
                }
                let profile = Profile::from(profile);
                let member = snapshot
                    .roster
                    .members
                    .entry(pubkey.clone())
                    .or_insert(Member {
                        profile: profile.clone(),
                        active: false,
                        amount_sats: 0,
                        lifetime_sats: 0,
                    });
                member.profile = profile.clone();
                member.active = true;
                snapshot
                    .roster
                    .awards
                    .entry(pubkey.clone())
                    .or_default()
                    .include(
                        snapshot
                            .roster
                            .evidence
                            .get(pubkey)
                            .copied()
                            .unwrap_or_default(),
                    );
                if !active {
                    notice = Some((pubkey.clone(), profile, "member_added", true));
                }
            }
            Action::Amount {
                pubkey,
                amount_sats,
            } => {
                if feeder != FeederAvailability::Ready {
                    bail!("score edits require confirmed unpaused feeder status");
                }
                let member = snapshot
                    .roster
                    .members
                    .get_mut(pubkey)
                    .filter(|m| m.active)
                    .context("score edits require an active member")?;
                let desired = operator::sats(amount_sats)?;
                if member.amount_sats != desired {
                    notice = Some((
                        pubkey.clone(),
                        member.profile.clone(),
                        "member_score_updated",
                        false,
                    ));
                }
                member.amount_sats = desired;
            }
            Action::Remove { pubkey } | Action::Ban { pubkey } => {
                if let Some(member) = snapshot.roster.members.get_mut(pubkey) {
                    if member.active {
                        notice = Some((
                            pubkey.clone(),
                            member.profile.clone(),
                            "member_removed",
                            false,
                        ));
                    }
                    member.active = false;
                    member.amount_sats = 0;
                }
                if matches!(command.action, Action::Ban { .. }) {
                    snapshot.roster.ban(pubkey)?;
                }
            }
            Action::Unban { pubkey } => {
                snapshot.roster.banned.remove(pubkey);
            }
            Action::Reset => {
                if now <= snapshot.cycle_start {
                    bail!("manual reset must advance cycle cutoff");
                }
                snapshot.roster.reset_cycle();
                snapshot.cycle = snapshot.cycle.checked_add(1).context("cycle overflow")?;
                snapshot.cycle_start = now;
                // Manual reset closes the pre-command observation boundary,
                // including same-second deferred work. Keep every source/input
                // receipt; never let it earn a new-cycle award on recovery.
                let discarded = serde_json::to_string(&super::Receipt {
                    status: super::Status::Discarded,
                    cycle: snapshot.cycle,
                    transition: None,
                    event_seq: None,
                })?;
                sqlx::query("UPDATE ingress SET state='done',result_json=? WHERE owner=? AND state='pending' AND kind IN ('social','payment')")
                    .bind(&discarded).bind(owner).execute(&mut *tx).await?;
                sqlx::query("UPDATE interactions SET status='discarded',result_json=? WHERE owner=? AND status='deferred'")
                    .bind(discarded).bind(owner).execute(&mut *tx).await?;
            }
            Action::Template { edit } => {
                edit_template(&mut tx, owner, edit).await?;
            }
        }
        snapshot.settings.validate()?;
        let changed = snapshot != previous || matches!(command.action, Action::Template { .. });
        if changed {
            snapshot.revision = snapshot
                .revision
                .checked_add(1)
                .context("revision overflow")?;
        }
        let allocation_changed = before_shares != snapshot.roster.shares(&snapshot.settings)?;
        let source = format!("operator:{}:{}", auth.actor, command.id);
        let mut notice_payload = None;
        if changed {
            if let Some((member, profile, category, activation)) = notice {
                let conversation = if activation && context.targets().next().is_some() {
                    let conversation = MemberConversation {
                        generation: crate::cyberherd::conversation::identity(
                            owner, &member, &source,
                        )?,
                        context: context.clone(),
                    };
                    sqlx::query("INSERT INTO member_conversations(owner,cycle,member,conversation_json) VALUES(?,?,?,?) ON CONFLICT(owner,cycle,member) DO UPDATE SET conversation_json=excluded.conversation_json")
                    .bind(owner).bind(snapshot.cycle).bind(&member).bind(serde_json::to_string(&conversation)?).execute(&mut *tx).await?;
                    Some(conversation)
                } else {
                    let raw:Option<String>=sqlx::query_scalar("SELECT conversation_json FROM member_conversations WHERE owner=? AND cycle=? AND member=?")
                    .bind(owner).bind(snapshot.cycle).bind(&member).fetch_optional(&mut *tx).await?;
                    raw.map(|raw| serde_json::from_str(&raw)).transpose()?
                };
                let actual_context = conversation
                    .as_ref()
                    .map(|c| c.context.clone())
                    .unwrap_or(context);
                notice_payload = Some(Notice {
                    member,
                    profile,
                    category: category.into(),
                    snapshot: snapshot.clone(),
                    allocation_changed,
                    context: actual_context,
                    conversation,
                    activation,
                });
            }
        }
        sqlx::query("UPDATE owners SET settings_json=?,roster_json=?,cycle=?,cycle_start=?,revision=? WHERE owner=?")
            .bind(serde_json::to_string(&snapshot.settings)?).bind(bounded_roster(&snapshot.roster)?).bind(snapshot.cycle).bind(snapshot.cycle_start).bind(snapshot.revision).bind(owner).execute(&mut *tx).await?;
        let (kind,payload)=match notice_payload {Some(notice)=>("operator_member",serde_json::to_string(&notice)?),None=>("operator_change",serde_json::json!({"actor":auth.actor,"command_id":command.id,"changed":changed,"revision":snapshot.revision}).to_string())};
        let event=sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,?,?)")
            .bind(owner).bind(snapshot.cycle).bind(source).bind(kind).bind(payload).execute(&mut *tx).await?;
        let receipt = Receipt {
            command_id: command.id,
            cycle: snapshot.cycle,
            revision: snapshot.revision,
            changed,
            event_seq: event.last_insert_rowid(),
        };
        sqlx::query("INSERT INTO operator_http_audit(owner,actor,command_id,event_id,body_hash,input_json,result_json,created_at) VALUES(?,?,?,?,?,?,?,?)")
            .bind(owner).bind(&auth.actor).bind(command.id.to_string()).bind(&auth.id).bind(&auth.body_hash).bind(input).bind(serde_json::to_string(&receipt)?).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}
async fn edit_template(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    edit: &super::TemplateEdit,
) -> Result<()> {
    use super::TemplateEdit::*;
    let (category, variant, text) = match edit {
        Set {
            category,
            variant,
            text,
        } => (category, Some(variant), Some(text)),
        Delete { category, variant } => (category, Some(variant), None),
        DisableCategory { category } | EnableCategory { category } => (category, None, None),
    };
    let variants = match variant {
        Some(v) => serde_json::json!({v:text.map(String::as_str).unwrap_or("deleted")}),
        None => serde_json::json!({}),
    };
    let catalog = crate::cyberherd::templates::Catalog::from_json(
        &serde_json::json!({category:variants}).to_string(),
    )?;
    sqlx::query("INSERT INTO template_categories(owner,category) VALUES(?,?) ON CONFLICT(owner,category) DO NOTHING").bind(owner).bind(category).execute(&mut **tx).await?;
    match edit {
        Set { variant, .. } => {
            let normalized = catalog
                .category(category)
                .and_then(|v| v.get(variant))
                .context("missing template")?;
            sqlx::query("INSERT INTO templates(owner,category,variant,text,deleted) VALUES(?,?,?,?,0) ON CONFLICT(owner,category,variant) DO UPDATE SET text=excluded.text,deleted=0")
                .bind(owner).bind(category).bind(variant).bind(normalized).execute(&mut **tx).await?;
        }
        Delete { variant, .. } => {
            sqlx::query("INSERT INTO templates(owner,category,variant,text,deleted) VALUES(?,?,?,NULL,1) ON CONFLICT(owner,category,variant) DO UPDATE SET text=NULL,deleted=1")
            .bind(owner).bind(category).bind(variant).execute(&mut **tx).await?;
        }
        DisableCategory { .. } | EnableCategory { .. } => {
            sqlx::query("UPDATE template_categories SET enabled=? WHERE owner=? AND category=?")
                .bind(matches!(edit, EnableCategory { .. }))
                .bind(owner)
                .bind(category)
                .execute(&mut **tx)
                .await?;
        }
    }
    let categories: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM template_categories WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut **tx)
            .await?;
    let variants: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE owner=? AND category=?")
            .bind(owner)
            .bind(category)
            .fetch_one(&mut **tx)
            .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(length(CAST(text AS BLOB))),0) FROM templates WHERE owner=?",
    )
    .bind(owner)
    .fetch_one(&mut **tx)
    .await?;
    if categories > 100 || variants > 1000 || total > 2_000_000 {
        bail!("template storage exceeds bound");
    }
    load_catalog(tx, owner).await?;
    Ok(())
}

impl Store {
    pub(crate) async fn operator_overview(
        &self,
        owner: &str,
        offset: usize,
        revision: Option<i64>,
    ) -> Result<serde_json::Value> {
        if offset > 100_000 {
            bail!("operator page exceeds bound");
        }
        let mut tx = self.pool.begin().await?;
        let snapshot = decode_snapshot(
            &sqlx::query("SELECT * FROM owners WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?,
        )?;
        if revision.is_some_and(|r| r != snapshot.revision) {
            bail!("operator revision changed");
        }
        let keys: std::collections::BTreeSet<_> = snapshot
            .roster
            .members
            .keys()
            .chain(snapshot.roster.banned.iter())
            .collect();
        let rows:Vec<_>=keys.iter().skip(offset).take(20).map(|key| {
            let member=snapshot.roster.members.get(*key);
            serde_json::json!({"pubkey":key,"active":member.is_some_and(|m|m.active),"banned":snapshot.roster.banned.contains(*key),"profile":member.map(|m|&m.profile),
                "amount_sats":member.map_or(0,|m|m.amount_sats).to_string(),"lifetime_sats":member.map_or(0,|m|m.lifetime_sats).to_string(),
                "awards":snapshot.roster.awards.get(*key).copied().unwrap_or_default()})
        }).collect();
        let categories:Vec<_>=sqlx::query("SELECT category,enabled FROM template_categories WHERE owner=? ORDER BY category LIMIT 101").bind(owner).fetch_all(&mut *tx).await?
            .into_iter().map(|r|Ok(serde_json::json!({"category":r.try_get::<String,_>("category")?,"enabled":r.try_get::<bool,_>("enabled")?}))).collect::<Result<_>>()?;
        if categories.len() > 100 {
            bail!("template category bound exceeded");
        }
        let result = serde_json::json!({"owner":owner,"revision":snapshot.revision,"cycle":snapshot.cycle,"cycle_start":snapshot.cycle_start,
            "settings":{"minimum_sats":snapshot.settings.minimum_sats.to_string(),"max_members":snapshot.settings.max_members,"member_percent":snapshot.settings.member_percent},
            "members":rows,"total_members":keys.len(),"next_offset":(offset+rows.len()<keys.len()).then_some(offset+rows.len()),"categories":categories});
        tx.commit().await?;
        Ok(result)
    }
    pub(crate) async fn operator_template_page(
        &self,
        owner: &str,
        category: &str,
        offset: usize,
        revision: Option<i64>,
    ) -> Result<serde_json::Value> {
        if category.is_empty() || category.len() > 128 || offset > 1000 {
            bail!("invalid template page");
        }
        let mut tx = self.pool.begin().await?;
        let current: i64 = sqlx::query_scalar("SELECT revision FROM owners WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
        if revision.is_some_and(|r| r != current) {
            bail!("operator revision changed");
        }
        let enabled: bool = sqlx::query_scalar(
            "SELECT enabled FROM template_categories WHERE owner=? AND category=?",
        )
        .bind(owner)
        .bind(category)
        .fetch_one(&mut *tx)
        .await?;
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE owner=? AND category=?")
                .bind(owner)
                .bind(category)
                .fetch_one(&mut *tx)
                .await?;
        let variants:Vec<_>=sqlx::query("SELECT variant,text,deleted FROM templates WHERE owner=? AND category=? ORDER BY variant LIMIT 5 OFFSET ?")
            .bind(owner).bind(category).bind(i64::try_from(offset)?).fetch_all(&mut *tx).await?.into_iter().map(|r|Ok(serde_json::json!({"variant":r.try_get::<String,_>("variant")?,"text":r.try_get::<Option<String>,_>("text")?,"deleted":r.try_get::<bool,_>("deleted")?}))).collect::<Result<_>>()?;
        let result = serde_json::json!({"revision":current,"category":category,"enabled":enabled,"variants":variants,"next_offset":((offset+variants.len())<(total as usize)).then_some(offset+variants.len())});
        tx.commit().await?;
        Ok(result)
    }
}
