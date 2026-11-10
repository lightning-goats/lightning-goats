//! Separate durable membership store. No core ledger, payment credential or
//! physical gateway capability is held here. Trusted ingress supplies verified
//! interactions; a relay's alleged zap receipt is never such an interaction.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use super::membership::{
    Action, Decision, FeederAvailability, Profile, Roster, Settings, Target, Transition,
    validate_pubkey,
};
use super::templates::Catalog;

#[path = "target_store.rs"]
mod target_state;

#[path = "ingress_store.rs"]
mod ingress_state;

#[path = "reward_store.rs"]
mod reward_state;

#[path = "result_store.rs"]
mod result_state;

#[path = "delivery_store.rs"]
#[cfg(unix)]
pub(crate) mod delivery_state;

#[path = "notice_store.rs"]
#[cfg(unix)]
pub(crate) mod notice_state;

#[path = "feeding_store.rs"]
#[cfg(unix)]
pub(crate) mod feeding_state;

#[path = "view_store.rs"]
mod view_state;

#[path = "operator_store.rs"]
mod operator_state;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./cyberherd-migrations");

#[derive(Debug, Clone)]
pub struct Store {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub owner: String,
    pub settings: Settings,
    pub roster: Roster,
    pub cycle: i64,
    pub cycle_start: i64,
    pub revision: i64,
}

/// Profiles are frozen at ingestion. Retrying a deferred source uses this same
/// immutable envelope; target freshness/override are re-evaluated separately.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interaction {
    pub source_id: String,
    pub source_hash: String,
    pub pubkey: String,
    pub profile: Profile,
    pub action: Action,
    pub occurred_at: i64,
}

impl Interaction {
    fn validate(&self) -> Result<()> {
        validate_pubkey(&self.pubkey)?;
        validate_pubkey(&self.source_hash)?;
        self.profile.validate()?;
        if self.occurred_at < 0
            || self.source_id.len() > 512
            || self.source_id.chars().any(char::is_control)
        {
            bail!("invalid CyberHerd source envelope");
        }
        match self.action {
            Action::Zap { amount_sats } => {
                if !self.source_id.starts_with("settlement:")
                    || self.source_id.len() <= 11
                    || amount_sats == 0
                    || amount_sats > i64::MAX as u64
                {
                    bail!("CyberHerd zaps require a positive authoritative settlement identity");
                }
            }
            Action::Repost | Action::Like => {
                let event_id = self
                    .source_id
                    .strip_prefix("nostr:")
                    .context("social interaction requires its Nostr event identity")?;
                validate_pubkey(event_id)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Deferred,
    Applied,
    Discarded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    pub status: Status,
    pub cycle: i64,
    pub transition: Option<Transition>,
    pub event_seq: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: i64,
    pub owner: String,
    pub cycle: i64,
    pub source_id: String,
    pub event_type: String,
    pub snapshot_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub enum TemplateEdit {
    Set {
        category: String,
        variant: String,
        text: String,
    },
    Delete {
        category: String,
        variant: String,
    },
    DisableCategory {
        category: String,
    },
    EnableCategory {
        category: String,
    },
}

impl Store {
    pub(crate) async fn close_after_initialization(self) {
        self.pool.close().await;
    }
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = crate::sqlite::connect_durable(database_url, 5).await?;
        MIGRATOR
            .run(&pool)
            .await
            .context("failed applying separate CyberHerd migrations")?;
        Ok(Self { pool })
    }

    pub async fn initialize(
        &self,
        owner: &str,
        settings: &Settings,
        cycle_start: i64,
    ) -> Result<()> {
        validate_pubkey(owner)?;
        settings.validate()?;
        if cycle_start < 0 {
            bail!("invalid CyberHerd cycle start");
        }
        sqlx::query("INSERT INTO owners(owner,settings_json,roster_json,cycle,cycle_start) VALUES(?,?,?,1,?) ON CONFLICT(owner) DO NOTHING")
            .bind(owner).bind(serde_json::to_string(settings)?).bind(serde_json::to_string(&Roster::default())?).bind(cycle_start).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn snapshot(&self, owner: &str) -> Result<Snapshot> {
        validate_pubkey(owner)?;
        let row = sqlx::query("SELECT * FROM owners WHERE owner=?")
            .bind(owner)
            .fetch_one(&self.pool)
            .await?;
        decode_snapshot(&row)
    }

    pub async fn process(
        &self,
        owner: &str,
        interaction: &Interaction,
        target: Target,
        feeder: FeederAvailability,
    ) -> Result<Receipt> {
        self.process_with_context(owner, interaction, target, feeder, None)
            .await
    }

    pub(crate) async fn process_with_context(
        &self,
        owner: &str,
        interaction: &Interaction,
        target: Target,
        feeder: FeederAvailability,
        context: Option<&super::targets::InteractionContext>,
    ) -> Result<Receipt> {
        validate_pubkey(owner)?;
        interaction.validate()?;
        let input_json = serde_json::to_string(interaction)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut was_deferred = false;
        if let Some(row) = sqlx::query("SELECT source_hash,input_json,status,result_json FROM interactions WHERE owner=? AND source_id=?").bind(owner).bind(&interaction.source_id).fetch_optional(&mut *tx).await? {
            if row.try_get::<String,_>("source_hash")? != interaction.source_hash || row.try_get::<String,_>("input_json")? != input_json {
                bail!("CyberHerd source identity conflicts with its immutable interaction");
            }
            let receipt: Receipt = serde_json::from_str(&row.try_get::<String,_>("result_json")?)?;
            if receipt.status != Status::Deferred {
                tx.commit().await?;
                return Ok(receipt);
            }
            was_deferred = true;
        }
        let row = sqlx::query("SELECT * FROM owners WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
        let mut snapshot = decode_snapshot(&row)?;
        let previous_shares = snapshot.roster.shares(&snapshot.settings)?;
        let previous_members = snapshot.roster.members.clone();
        let stale_target = feeder == FeederAvailability::Ready
            && (target == Target::Other
                || (target == Target::ProjectContent
                    && (was_deferred
                        || !(matches!(interaction.action, Action::Zap { .. })
                            && snapshot
                                .roster
                                .members
                                .get(&interaction.pubkey)
                                .is_some_and(|member| member.active)))));
        let receipt = if interaction.occurred_at < snapshot.cycle_start || stale_target {
            Receipt {
                status: Status::Discarded,
                cycle: snapshot.cycle,
                transition: None,
                event_seq: None,
            }
        } else {
            let transition = snapshot.roster.apply(
                &snapshot.settings,
                &interaction.pubkey,
                &interaction.profile,
                interaction.action,
                target,
                feeder,
            )?;
            let status = if transition.decision == Decision::Deferred {
                Status::Deferred
            } else {
                Status::Applied
            };
            let mut receipt = Receipt {
                status,
                cycle: snapshot.cycle,
                transition: Some(transition),
                event_seq: None,
            };
            if receipt.status == Status::Applied {
                let roster_json = bounded_roster(&snapshot.roster)?;
                let revision = snapshot
                    .revision
                    .checked_add(1)
                    .context("CyberHerd revision overflow")?;
                sqlx::query("UPDATE owners SET roster_json=?,revision=? WHERE owner=?")
                    .bind(roster_json)
                    .bind(revision)
                    .bind(owner)
                    .execute(&mut *tx)
                    .await?;
                // The entire post-transition state and outcome are frozen together.
                // Message adapters must not look up mutable scores on retries.
                snapshot.revision = revision;
                let mut conversation = None;
                if let Some(context) = context {
                    if matches!(
                        receipt.transition.as_ref().map(|t| &t.decision),
                        Some(
                            Decision::Joined
                                | Decision::Displaced { .. }
                                | Decision::Increased
                                | Decision::SocialAwarded
                        )
                    ) {
                        let candidate = super::conversation::MemberConversation {
                            generation: super::conversation::identity(
                                owner,
                                &interaction.pubkey,
                                &interaction.source_id,
                            )?,
                            context: super::conversation::Context::from_interaction(context),
                        };
                        candidate.validate(owner)?;
                        let replace = matches!(
                            receipt.transition.as_ref().map(|t| &t.decision),
                            Some(Decision::Joined | Decision::Displaced { .. })
                        );
                        sqlx::query("INSERT INTO member_conversations(owner,cycle,member,conversation_json) VALUES(?,?,?,?) ON CONFLICT(owner,cycle,member) DO UPDATE SET conversation_json=excluded.conversation_json WHERE ?")
                            .bind(owner).bind(snapshot.cycle).bind(&interaction.pubkey).bind(serde_json::to_string(&candidate)?).bind(replace)
                            .execute(&mut *tx).await?;
                        let raw:String=sqlx::query_scalar("SELECT conversation_json FROM member_conversations WHERE owner=? AND cycle=? AND member=?")
                            .bind(owner).bind(snapshot.cycle).bind(&interaction.pubkey).fetch_one(&mut *tx).await?;
                        conversation = Some(serde_json::from_str::<
                            super::conversation::MemberConversation,
                        >(&raw)?);
                    }
                }
                let allocation_changed =
                    previous_shares != snapshot.roster.shares(&snapshot.settings)?;
                let victim = receipt.transition.as_ref().and_then(|t| match &t.decision {
                    Decision::Displaced { victim } => previous_members
                        .get(victim)
                        .map(|m| (victim.clone(), m.profile.clone(), m.amount_sats)),
                    _ => None,
                });
                let payload = serde_json::json!({"interaction":interaction,"transition":receipt.transition,"snapshot":snapshot,"context":context,"conversation":conversation,"allocation_changed":allocation_changed,"victim":victim});
                let event = sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,'interaction',?)")
                    .bind(owner).bind(snapshot.cycle).bind(&interaction.source_id).bind(serde_json::to_string(&payload)?).execute(&mut *tx).await?;
                receipt.event_seq = Some(event.last_insert_rowid());
            }
            receipt
        };
        let status = match receipt.status {
            Status::Deferred => "deferred",
            Status::Applied => "applied",
            Status::Discarded => "discarded",
        };
        sqlx::query("INSERT INTO interactions(owner,source_id,source_hash,input_json,occurred_at,status,result_json) VALUES(?,?,?,?,?,?,?) ON CONFLICT(owner,source_id) DO UPDATE SET status=excluded.status,result_json=excluded.result_json")
            .bind(owner).bind(&interaction.source_id).bind(&interaction.source_hash).bind(input_json).bind(interaction.occurred_at).bind(status).bind(serde_json::to_string(&receipt)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn deferred(&self, owner: &str, limit: u16) -> Result<Vec<Interaction>> {
        validate_pubkey(owner)?;
        if !(1..=1000).contains(&limit) {
            bail!("invalid CyberHerd recovery batch size");
        }
        sqlx::query_scalar::<_,String>("SELECT input_json FROM interactions WHERE owner=? AND status='deferred' ORDER BY occurred_at,source_id LIMIT ?")
            .bind(owner).bind(i64::from(limit)).fetch_all(&self.pool).await?.into_iter().map(|json| Ok(serde_json::from_str(&json)?)).collect()
    }

    /// Manual and scheduled resets share this audited idempotent transaction.
    /// No source receipt, lifetime total or ban is deleted. Public daily notices
    /// are a separate purpose; manual resets never manufacture such a notice.
    pub async fn reset(&self, owner: &str, command_id: &str, cycle_start: i64) -> Result<Snapshot> {
        validate_pubkey(owner)?;
        validate_command_id(command_id)?;
        let input_json = serde_json::json!({"kind":"reset","cycle_start":cycle_start}).to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row) = sqlx::query(
            "SELECT input_json,result_json FROM operator_commands WHERE owner=? AND command_id=?",
        )
        .bind(owner)
        .bind(command_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if row.try_get::<String, _>("input_json")? != input_json {
                bail!("operator command conflicts with existing intent");
            }
            let result = serde_json::from_str(&row.try_get::<String, _>("result_json")?)?;
            tx.commit().await?;
            return Ok(result);
        }
        let row = sqlx::query("SELECT * FROM owners WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
        let mut snapshot = decode_snapshot(&row)?;
        if cycle_start <= snapshot.cycle_start {
            bail!("reset must advance the cycle cutoff");
        }
        snapshot.roster.reset_cycle();
        snapshot.cycle = snapshot
            .cycle
            .checked_add(1)
            .context("CyberHerd cycle overflow")?;
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .context("CyberHerd revision overflow")?;
        snapshot.cycle_start = cycle_start;
        let result_json = serde_json::to_string(&snapshot)?;
        sqlx::query(
            "UPDATE owners SET roster_json=?,cycle=?,cycle_start=?,revision=? WHERE owner=?",
        )
        .bind(bounded_roster(&snapshot.roster)?)
        .bind(snapshot.cycle)
        .bind(snapshot.cycle_start)
        .bind(snapshot.revision)
        .bind(owner)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO operator_commands(owner,command_id,input_json,result_json) VALUES(?,?,?,?)").bind(owner).bind(command_id).bind(input_json).bind(&result_json).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,'cycle_reset',?)").bind(owner).bind(snapshot.cycle).bind(format!("operator:{command_id}")).bind(result_json).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    /// Seed only previously unseen keys. Operator edits, per-variant tombstones
    /// and disabled categories survive upgrades and repeated initializations.
    pub async fn seed_templates(&self, owner: &str, catalog: &Catalog) -> Result<()> {
        validate_pubkey(owner)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        for (category, variants) in catalog.categories() {
            sqlx::query("INSERT INTO template_categories(owner,category) VALUES(?,?) ON CONFLICT(owner,category) DO NOTHING")
                .bind(owner).bind(category).execute(&mut *tx).await?;
            for (variant, text) in variants {
                sqlx::query("INSERT INTO templates(owner,category,variant,text) VALUES(?,?,?,?) ON CONFLICT(owner,category,variant) DO NOTHING")
                    .bind(owner).bind(category).bind(variant).bind(text).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn templates(&self, owner: &str) -> Result<Catalog> {
        validate_pubkey(owner)?;
        let mut tx = self.pool.begin().await?;
        let result = load_catalog(&mut tx, owner).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// This is an operator capability, not a public profile or relay operation.
    /// Explicit re-enable restores a category; seeding can never do so.
    pub async fn edit_template(
        &self,
        owner: &str,
        command_id: &str,
        edit: &TemplateEdit,
    ) -> Result<()> {
        validate_pubkey(owner)?;
        validate_command_id(command_id)?;
        let (category, variant, text) = match edit {
            TemplateEdit::Set {
                category,
                variant,
                text,
            } => (category, Some(variant), Some(text)),
            TemplateEdit::Delete { category, variant } => (category, Some(variant), None),
            TemplateEdit::DisableCategory { category }
            | TemplateEdit::EnableCategory { category } => (category, None, None),
        };
        // Use the same catalog parser for names, bounds and placeholder safety.
        let variants = match variant {
            Some(variant) => {
                serde_json::json!({variant:text.map(String::as_str).unwrap_or("deleted")})
            }
            None => serde_json::json!({}),
        };
        let checked = Catalog::from_json(&serde_json::json!({category:variants}).to_string())?;
        let normalized = variant.and_then(|variant| {
            checked
                .category(category)
                .and_then(|values| values.get(variant))
        });
        let input_json = serde_json::to_string(edit)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(previous) = sqlx::query_scalar::<_, String>(
            "SELECT input_json FROM operator_commands WHERE owner=? AND command_id=?",
        )
        .bind(owner)
        .bind(command_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if previous != input_json {
                bail!("operator command conflicts with existing intent");
            }
            tx.commit().await?;
            return Ok(());
        }
        sqlx::query("INSERT INTO template_categories(owner,category) VALUES(?,?) ON CONFLICT(owner,category) DO NOTHING").bind(owner).bind(category).execute(&mut *tx).await?;
        match edit {
            TemplateEdit::Set { variant, .. } => {
                sqlx::query("INSERT INTO templates(owner,category,variant,text,deleted) VALUES(?,?,?,?,0) ON CONFLICT(owner,category,variant) DO UPDATE SET text=excluded.text,deleted=0")
                    .bind(owner).bind(category).bind(variant).bind(normalized.context("missing normalized template")?).execute(&mut *tx).await?;
            }
            TemplateEdit::Delete { variant, .. } => {
                sqlx::query("INSERT INTO templates(owner,category,variant,text,deleted) VALUES(?,?,?,NULL,1) ON CONFLICT(owner,category,variant) DO UPDATE SET text=NULL,deleted=1")
                    .bind(owner).bind(category).bind(variant).execute(&mut *tx).await?;
            }
            TemplateEdit::DisableCategory { .. } | TemplateEdit::EnableCategory { .. } => {
                sqlx::query(
                    "UPDATE template_categories SET enabled=? WHERE owner=? AND category=?",
                )
                .bind(i64::from(matches!(
                    edit,
                    TemplateEdit::EnableCategory { .. }
                )))
                .bind(owner)
                .bind(category)
                .execute(&mut *tx)
                .await?;
            }
        }
        sqlx::query("INSERT INTO operator_commands(owner,command_id,input_json,result_json) VALUES(?,?,?,'null')").bind(owner).bind(command_id).bind(&input_json).execute(&mut *tx).await?;
        let cycle: i64 = sqlx::query_scalar("SELECT cycle FROM owners WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,'template_edit',?)").bind(owner).bind(cycle).bind(format!("operator:{command_id}")).bind(input_json).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn events_after(
        &self,
        owner: &str,
        cursor: i64,
        limit: u16,
    ) -> Result<Vec<StoredEvent>> {
        validate_pubkey(owner)?;
        if cursor < 0 || !(1..=1000).contains(&limit) {
            bail!("invalid CyberHerd event range");
        }
        let rows =
            sqlx::query("SELECT * FROM domain_events WHERE owner=? AND seq>? ORDER BY seq LIMIT ?")
                .bind(owner)
                .bind(cursor)
                .bind(i64::from(limit))
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|row| {
                Ok(StoredEvent {
                    seq: row.try_get("seq")?,
                    owner: row.try_get("owner")?,
                    cycle: row.try_get("cycle")?,
                    source_id: row.try_get("source_id")?,
                    event_type: row.try_get("event_type")?,
                    snapshot_json: row.try_get("snapshot_json")?,
                })
            })
            .collect()
    }
}

pub(super) async fn load_catalog(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    owner: &str,
) -> Result<Catalog> {
    let rows = sqlx::query("SELECT t.category,t.variant,t.text FROM templates t JOIN template_categories c ON t.owner=c.owner AND t.category=c.category WHERE t.owner=? AND c.enabled=1 AND t.deleted=0 ORDER BY t.category,t.variant")
        .bind(owner).fetch_all(&mut **tx).await?;
    let mut categories =
        std::collections::BTreeMap::<String, std::collections::BTreeMap<String, String>>::new();
    for row in rows {
        categories
            .entry(row.try_get("category")?)
            .or_default()
            .insert(row.try_get("variant")?, row.try_get("text")?);
    }
    Catalog::from_json(&serde_json::to_string(&categories)?)
}

fn decode_snapshot(row: &sqlx::sqlite::SqliteRow) -> Result<Snapshot> {
    Ok(Snapshot {
        owner: row.try_get("owner")?,
        settings: serde_json::from_str(&row.try_get::<String, _>("settings_json")?)?,
        roster: serde_json::from_str(&row.try_get::<String, _>("roster_json")?)?,
        cycle: row.try_get("cycle")?,
        cycle_start: row.try_get("cycle_start")?,
        revision: row.try_get("revision")?,
    })
}

fn bounded_roster(roster: &Roster) -> Result<String> {
    let json = serde_json::to_string(roster)?;
    if json.len() > 32_000_000 {
        bail!("CyberHerd roster storage bound reached");
    }
    Ok(json)
}

fn validate_command_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
    {
        bail!("invalid CyberHerd operator command identifier");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn key(n: u8) -> String {
        format!("{n:064x}")
    }
    fn interaction(n: u8, id: &str, at: i64, action: Action) -> Interaction {
        Interaction {
            source_id: id.into(),
            source_hash: key(n),
            pubkey: key(n),
            profile: Profile {
                name: format!("Member {n}"),
                picture: None,
                lud16: Some(format!("member{n}@example.com")),
            },
            action,
            occurred_at: at,
        }
    }
    async fn fixture() -> (TempDir, String, Store) {
        let dir = TempDir::new().unwrap();
        let url = format!("sqlite://{}/cyberherd.db", dir.path().display());
        let store = Store::connect(&url).await.unwrap();
        store
            .initialize(
                &key(100),
                &Settings {
                    minimum_sats: 10,
                    max_members: 2,
                    member_percent: 10,
                },
                100,
            )
            .await
            .unwrap();
        (dir, url, store)
    }

    #[tokio::test]
    async fn concurrent_duplicates_restart_and_reset_preserve_one_source_receipt() {
        let (_dir, url, store) = fixture().await;
        let owner = key(100);
        let input = interaction(
            1,
            "settlement:strike:one",
            101,
            Action::Zap { amount_sats: 10 },
        );
        let (a, b) = tokio::join!(
            store.process(&owner, &input, Target::Current, FeederAvailability::Ready),
            store.process(&owner, &input, Target::Current, FeederAvailability::Ready)
        );
        assert_eq!(a.unwrap(), b.unwrap());
        assert_eq!(
            store.snapshot(&owner).await.unwrap().roster.members[&key(1)].amount_sats,
            10
        );
        drop(store);
        let store = Store::connect(&url).await.unwrap();
        let original = store
            .process(&owner, &input, Target::Current, FeederAvailability::Ready)
            .await
            .unwrap();
        let reset = store.reset(&owner, "manual-1", 200).await.unwrap();
        assert_eq!(store.reset(&owner, "manual-1", 200).await.unwrap(), reset);
        assert!(store.reset(&owner, "manual-1", 201).await.is_err());
        assert_eq!(
            store
                .process(&owner, &input, Target::Current, FeederAvailability::Ready)
                .await
                .unwrap(),
            original
        );
        let current = store.snapshot(&owner).await.unwrap();
        assert_eq!(current.cycle, 2);
        assert_eq!(current.roster.active_count(), 0);
        assert_eq!(current.roster.members[&key(1)].lifetime_sats, 10);
        assert_eq!(store.events_after(&owner, 0, 100).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn deferred_work_waits_then_applies_or_discards_without_double_awards() {
        let (_dir, _url, store) = fixture().await;
        let owner = key(100);
        let input = interaction(1, &format!("nostr:{}", key(42)), 101, Action::Repost);
        assert_eq!(
            store
                .process(&owner, &input, Target::Current, FeederAvailability::Paused)
                .await
                .unwrap()
                .status,
            Status::Deferred
        );
        assert!(store.events_after(&owner, 0, 10).await.unwrap().is_empty());
        assert_eq!(
            store.deferred(&owner, 10).await.unwrap(),
            vec![input.clone()]
        );
        assert_eq!(
            store
                .process(
                    &owner,
                    &input,
                    Target::Unavailable,
                    FeederAvailability::Ready
                )
                .await
                .unwrap()
                .status,
            Status::Deferred
        );
        let applied = store
            .process(&owner, &input, Target::Current, FeederAvailability::Ready)
            .await
            .unwrap();
        assert_eq!(applied.transition.unwrap().social_points, 3);
        assert!(store.deferred(&owner, 10).await.unwrap().is_empty());
        let stale = interaction(2, &format!("nostr:{}", key(43)), 102, Action::Like);
        store
            .process(&owner, &stale, Target::Current, FeederAvailability::Unknown)
            .await
            .unwrap();
        store.reset(&owner, "daily-2", 200).await.unwrap();
        assert_eq!(
            store
                .process(&owner, &stale, Target::Current, FeederAvailability::Ready)
                .await
                .unwrap()
                .status,
            Status::Discarded
        );
        assert_eq!(
            store.snapshot(&owner).await.unwrap().roster.active_count(),
            0
        );
    }

    #[tokio::test]
    async fn conflicting_inputs_roll_back_and_distinct_settlements_count_separately() {
        let (_dir, _url, store) = fixture().await;
        let owner = key(100);
        let mut input = interaction(
            1,
            "settlement:strike:one",
            101,
            Action::Zap { amount_sats: 10 },
        );
        store
            .process(&owner, &input, Target::Current, FeederAvailability::Ready)
            .await
            .unwrap();
        input.action = Action::Zap { amount_sats: 11 };
        assert!(
            store
                .process(&owner, &input, Target::Current, FeederAvailability::Ready)
                .await
                .is_err()
        );
        assert_eq!(
            store.snapshot(&owner).await.unwrap().roster.members[&key(1)].amount_sats,
            10
        );
        input.source_id = "settlement:strike:two".into();
        store
            .process(
                &owner,
                &input,
                Target::ProjectContent,
                FeederAvailability::Ready,
            )
            .await
            .unwrap();
        assert_eq!(
            store.snapshot(&owner).await.unwrap().roster.members[&key(1)].amount_sats,
            21
        );
        assert_eq!(store.events_after(&owner, 0, 10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn owners_are_isolated_and_invalid_sources_cannot_become_settlements() {
        let (_dir, _url, store) = fixture().await;
        let a = key(100);
        let b = key(101);
        store
            .initialize(
                &b,
                &Settings {
                    minimum_sats: 10,
                    max_members: 2,
                    member_percent: 0,
                },
                100,
            )
            .await
            .unwrap();
        let input = interaction(
            1,
            "settlement:strike:one",
            101,
            Action::Zap { amount_sats: 10 },
        );
        store
            .process(&a, &input, Target::Current, FeederAvailability::Ready)
            .await
            .unwrap();
        assert_eq!(store.snapshot(&b).await.unwrap().roster.active_count(), 0);
        let mut invalid = input.clone();
        invalid.source_id = format!("nostr:{}", key(44));
        assert!(
            store
                .process(&b, &invalid, Target::Current, FeederAvailability::Ready)
                .await
                .is_err()
        );
        store
            .process(&b, &input, Target::Current, FeederAvailability::Ready)
            .await
            .unwrap();
        assert_eq!(
            store
                .snapshot(&b)
                .await
                .unwrap()
                .roster
                .shares(&store.snapshot(&b).await.unwrap().settings)
                .unwrap()
                .values()
                .sum::<u64>(),
            0
        );
    }

    #[tokio::test]
    async fn refreshed_stale_target_gets_only_a_minimal_discard_receipt() {
        let (_dir, _url, store) = fixture().await;
        let owner = key(100);
        let input = interaction(1, &format!("nostr:{}", key(42)), 101, Action::Repost);
        store
            .process(&owner, &input, Target::Current, FeederAvailability::Paused)
            .await
            .unwrap();
        let stale = store
            .process(&owner, &input, Target::Other, FeederAvailability::Ready)
            .await
            .unwrap();
        assert_eq!(stale.status, Status::Discarded);
        assert!(stale.transition.is_none());
        assert!(stale.event_seq.is_none());
        assert!(store.events_after(&owner, 0, 10).await.unwrap().is_empty());
        assert_eq!(
            store.snapshot(&owner).await.unwrap().roster,
            Roster::default()
        );
        assert_eq!(
            store
                .process(&owner, &input, Target::Current, FeederAvailability::Ready)
                .await
                .unwrap(),
            stale
        );
    }

    #[tokio::test]
    async fn imported_wording_edits_and_opt_out_survive_reseed_restart_and_command_retry() {
        let (_dir, url, store) = fixture().await;
        let owner = key(100);
        let legacy = Catalog::legacy().unwrap();
        store.seed_templates(&owner, &legacy).await.unwrap();
        assert_eq!(
            store
                .templates(&owner)
                .await
                .unwrap()
                .category_templates("sats_received")
                .len(),
            150
        );
        let edit = TemplateEdit::Set {
            category: "member_added".into(),
            variant: "0".into(),
            text: "Welcome {name}, keep our custom words.".into(),
        };
        store
            .edit_template(&owner, "customize-1", &edit)
            .await
            .unwrap();
        store
            .edit_template(&owner, "customize-1", &edit)
            .await
            .unwrap();
        store
            .edit_template(
                &owner,
                "delete-1",
                &TemplateEdit::Delete {
                    category: "sats_received".into(),
                    variant: "0".into(),
                },
            )
            .await
            .unwrap();
        store
            .edit_template(
                &owner,
                "disable-1",
                &TemplateEdit::DisableCategory {
                    category: "cyber_herd_invitation".into(),
                },
            )
            .await
            .unwrap();
        let unsafe_edit = TemplateEdit::Set {
            category: "member_added".into(),
            variant: "0".into(),
            text: "{name.__class__}".into(),
        };
        assert!(
            store
                .edit_template(&owner, "unsafe-1", &unsafe_edit)
                .await
                .is_err()
        );
        assert!(
            store
                .edit_template(
                    &owner,
                    "customize-1",
                    &TemplateEdit::DisableCategory {
                        category: "member_added".into()
                    }
                )
                .await
                .is_err()
        );
        drop(store);
        let store = Store::connect(&url).await.unwrap();
        store.seed_templates(&owner, &legacy).await.unwrap();
        let current = store.templates(&owner).await.unwrap();
        assert_eq!(
            current.category("member_added").unwrap()["0"],
            "Welcome {name}, keep our custom words."
        );
        assert_eq!(current.category_templates("sats_received").len(), 149);
        assert!(current.category("cyber_herd_invitation").is_none());
        assert!(current.category("cyber_herd_pause").is_none());
        store
            .edit_template(
                &owner,
                "enable-1",
                &TemplateEdit::EnableCategory {
                    category: "cyber_herd_invitation".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .templates(&owner)
                .await
                .unwrap()
                .category_templates("cyber_herd_invitation")
                .len(),
            8
        );
        let events = store.events_after(&owner, 0, 100).await.unwrap();
        assert_eq!(events.len(), 4);
        assert!(
            events
                .iter()
                .all(|event| event.event_type == "template_edit")
        );
    }
}
