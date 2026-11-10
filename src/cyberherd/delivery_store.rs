use super::{Store, load_catalog};
use crate::{
    cyberherd::{
        conversation::{MemberConversation, PublishedReference},
        delivery::{self, Rendered},
        targets::ConversationTarget,
    },
    nostr::SignedNostrEvent,
    nostr_event::VerifiedEvent,
};
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::{Row as _, Sqlite, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Unsigned {
    pub owner: String,
    pub kind: u64,
    pub created_at: u64,
    pub content: String,
    pub tags: Vec<Vec<String>>,
}
impl Unsigned {
    pub fn validate_signed(&self, event: &VerifiedEvent) -> Result<()> {
        let e = event.event();
        if e.pubkey != self.owner
            || e.kind != self.kind
            || e.created_at != self.created_at
            || e.content != self.content
            || e.tags != self.tags
        {
            bail!("signer changed frozen member message");
        }
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) struct Job {
    pub owner: String,
    pub identity: String,
    pub destination: String,
    pub lease: Uuid,
    pub unsigned: Unsigned,
    pub signed: Option<String>,
    pub relay: String,
}

impl Store {
    pub async fn initialize_delivery(&self, owner: &str, start_after: i64) -> Result<()> {
        crate::nostr_event::validate_public_key(owner)?;
        if start_after < 0 {
            bail!("negative delivery activation cursor");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let latest: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(seq),0) FROM domain_events WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?;
        if start_after > latest {
            bail!("delivery activation cursor exceeds retained history");
        }
        sqlx::query("INSERT INTO delivery_cursors(owner,start_after,through) VALUES(?,?,?) ON CONFLICT(owner) DO NOTHING")
            .bind(owner).bind(start_after).bind(start_after).execute(&mut *tx).await?;
        let old: i64 = sqlx::query_scalar("SELECT start_after FROM delivery_cursors WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
        if old != start_after {
            bail!("delivery activation boundary changed; explicit recovery required");
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn queue_next_delivery(
        &self,
        owner: &str,
        relays: &[String],
        now: i64,
    ) -> Result<bool> {
        let relays = crate::payout::invoice::receipt_relays(relays)?;
        if now < 0 {
            bail!("negative delivery time");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let cursor: i64 = sqlx::query_scalar("SELECT through FROM delivery_cursors WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
        let row =
            sqlx::query("SELECT * FROM domain_events WHERE owner=? AND seq>? ORDER BY seq LIMIT 1")
                .bind(owner)
                .bind(cursor)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(row) = row else { return Ok(false) };
        let event = super::StoredEvent {
            seq: row.try_get("seq")?,
            owner: row.try_get("owner")?,
            cycle: row.try_get("cycle")?,
            source_id: row.try_get("source_id")?,
            event_type: row.try_get("event_type")?,
            snapshot_json: row.try_get("snapshot_json")?,
        };
        let catalog = load_catalog(&mut tx, owner).await?;
        if let Some(message) = delivery::render(&event, &catalog)? {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM delivery_messages WHERE owner=? AND identity=?)",
            )
            .bind(owner)
            .bind(&message.identity)
            .fetch_one(&mut *tx)
            .await?;
            if !exists {
                let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM delivery_messages m JOIN delivery_overlay o ON o.owner=m.owner AND o.identity=m.identity WHERE m.owner=? AND (o.event_seq IS NULL OR EXISTS(SELECT 1 FROM delivery_destinations d WHERE d.owner=m.owner AND d.identity=m.identity AND (SELECT COUNT(*) FROM delivery_receipts r WHERE r.owner=d.owner AND r.identity=d.identity AND r.destination=d.destination)<json_array_length(m.relays_json)))")
                    .bind(owner).fetch_one(&mut *tx).await?;
                if pending >= 2048 {
                    bail!("member presentation backlog is full");
                }
                sqlx::query("INSERT INTO delivery_messages(owner,identity,source_seq,document_json,relays_json,created_at) VALUES(?,?,?,?,?,?)")
                    .bind(owner).bind(&message.identity).bind(event.seq).bind(serde_json::to_string(&message)?).bind(serde_json::to_string(&relays)?).bind(now).execute(&mut *tx).await?;
                insert_destinations(&mut tx, owner, &message, None).await?;
                sqlx::query("INSERT INTO delivery_overlay(owner,identity) VALUES(?,?)")
                    .bind(owner)
                    .bind(&message.identity)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        sqlx::query("UPDATE delivery_cursors SET through=? WHERE owner=?")
            .bind(event.seq)
            .bind(owner)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }
    pub(crate) async fn reserve_delivery(&self, owner: &str, now: i64) -> Result<Option<Job>> {
        if now < 0 {
            bail!("negative delivery observation");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        // A waiting branch is rotated; it cannot starve unrelated destinations.
        let rows=sqlx::query("SELECT w.*,m.document_json,m.relays_json,m.created_at,d.branch,d.target_json FROM delivery_work w JOIN delivery_messages m ON m.owner=w.owner AND m.identity=w.identity JOIN delivery_destinations d ON d.owner=w.owner AND d.identity=w.identity AND d.destination=w.destination WHERE w.owner=? AND w.lease_until<=? AND (SELECT COUNT(*) FROM delivery_receipts r WHERE r.owner=w.owner AND r.identity=w.identity AND r.destination=w.destination)<json_array_length(m.relays_json) ORDER BY w.attempted_at,m.source_seq,w.destination LIMIT 32")
            .bind(owner).bind(now).fetch_all(&mut *tx).await?;
        for row in rows {
            let identity: String = row.try_get("identity")?;
            let destination: String = row.try_get("destination")?;
            let message: Rendered =
                serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
            let relays: Vec<String> =
                serde_json::from_str(&row.try_get::<String, _>("relays_json")?)?;
            let mut relay = None;
            for candidate in relays {
                let done:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM delivery_receipts WHERE owner=? AND identity=? AND destination=? AND relay=?)")
                    .bind(owner).bind(&identity).bind(&destination).bind(&candidate).fetch_one(&mut *tx).await?;
                if !done {
                    relay = Some(candidate);
                    break;
                }
            }
            let Some(relay) = relay else { continue };
            let branch: Option<String> = row.try_get("branch")?;
            let reference = if let Some(branch) = &branch {
                let starter: Option<String> = sqlx::query_scalar(
                    "SELECT starter FROM delivery_branches WHERE owner=? AND branch=?",
                )
                .bind(owner)
                .bind(branch)
                .fetch_optional(&mut *tx)
                .await?;
                if starter.as_deref() == Some(&identity) && !message.requires_joining {
                    None
                } else {
                    let reference = joining_reference(&mut tx, owner, branch).await?;
                    if reference.is_none() {
                        sqlx::query("UPDATE delivery_work SET attempted_at=?,lease_until=? WHERE owner=? AND identity=? AND destination=?")
                            .bind(now).bind(now.saturating_add(2)).bind(owner).bind(&identity).bind(&destination).execute(&mut *tx).await?;
                        continue;
                    }
                    reference
                }
            } else {
                None
            };
            let saved:Option<String>=sqlx::query_scalar("SELECT document_json FROM delivery_unsigned WHERE owner=? AND identity=? AND destination=?")
                .bind(owner).bind(&identity).bind(&destination).fetch_optional(&mut *tx).await?;
            let unsigned = match saved {
                Some(raw) => serde_json::from_str(&raw)?,
                None => {
                    let target: ConversationTarget =
                        serde_json::from_str(&row.try_get::<String, _>("target_json")?)?;
                    let unsigned = unsigned(
                        owner,
                        &message,
                        &target,
                        reference.as_ref(),
                        row.try_get("created_at")?,
                    )?;
                    sqlx::query("INSERT INTO delivery_unsigned(owner,identity,destination,document_json) VALUES(?,?,?,?)")
                        .bind(owner).bind(&identity).bind(&destination).bind(serde_json::to_string(&unsigned)?).execute(&mut *tx).await?;
                    unsigned
                }
            };
            let signed=sqlx::query_scalar("SELECT event_json FROM delivery_signed WHERE owner=? AND identity=? AND destination=?")
                .bind(owner).bind(&identity).bind(&destination).fetch_optional(&mut *tx).await?;
            let lease = Uuid::new_v4();
            sqlx::query("UPDATE delivery_work SET lease_id=?,lease_until=?,attempted_at=? WHERE owner=? AND identity=? AND destination=?")
                .bind(lease.to_string()).bind(now.saturating_add(60)).bind(now).bind(owner).bind(&identity).bind(&destination).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok(Some(Job {
                owner: owner.into(),
                identity,
                destination,
                lease,
                unsigned,
                signed,
                relay,
            }));
        }
        tx.commit().await?;
        Ok(None)
    }
    pub(crate) async fn save_delivery_signature(
        &self,
        job: &Job,
        event: &VerifiedEvent,
    ) -> Result<String> {
        job.unsigned.validate_signed(event)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_job(&mut tx, job).await?;
        sqlx::query("INSERT INTO delivery_signed(owner,identity,destination,event_id,event_json) VALUES(?,?,?,?,?) ON CONFLICT(owner,identity,destination) DO NOTHING")
            .bind(&job.owner).bind(&job.identity).bind(&job.destination).bind(&event.event().id).bind(serde_json::to_string(event.event())?).execute(&mut *tx).await?;
        let raw: String = sqlx::query_scalar(
            "SELECT event_json FROM delivery_signed WHERE owner=? AND identity=? AND destination=?",
        )
        .bind(&job.owner)
        .bind(&job.identity)
        .bind(&job.destination)
        .fetch_one(&mut *tx)
        .await?;
        job.unsigned
            .validate_signed(&VerifiedEvent::from_json(&raw)?)?;
        tx.commit().await?;
        Ok(raw)
    }
    pub(crate) async fn finish_delivery(
        &self,
        job: &Job,
        ack: Option<&crate::cyberherd::delivery_worker::RelayAck>,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_job(&mut tx, job).await?;
        if let Some(ack) = ack {
            let raw:String=sqlx::query_scalar("SELECT event_json FROM delivery_signed WHERE owner=? AND identity=? AND destination=?")
                .bind(&job.owner).bind(&job.identity).bind(&job.destination).fetch_one(&mut *tx).await?;
            let event = VerifiedEvent::from_json(&raw)?;
            job.unsigned.validate_signed(&event)?;
            ack.check(&event, &job.relay)?;
            sqlx::query("INSERT INTO delivery_receipts(owner,identity,destination,relay,acknowledged_at) VALUES(?,?,?,?,?) ON CONFLICT(owner,identity,destination,relay) DO NOTHING")
                .bind(&job.owner).bind(&job.identity).bind(&job.destination).bind(&job.relay).bind(ack.at()).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE delivery_work SET lease_id=NULL,lease_until=? WHERE owner=? AND identity=? AND destination=?")
            .bind(now.saturating_add(if ack.is_some(){0}else{5})).bind(&job.owner).bind(&job.identity).bind(&job.destination).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn published_joining(
        &self,
        owner: &str,
        conversation: &MemberConversation,
        cycle: i64,
        member: &str,
    ) -> Result<Option<PublishedReference>> {
        conversation.validate(owner)?;
        let mut tx = self.pool.begin().await?;
        // Prefer the member's original note thread, then its saved stream.
        for target in conversation.context.targets() {
            let branch = branch_key(owner, cycle, member, &conversation.generation, target)?;
            if let Some(reference) = joining_reference(&mut tx, owner, &branch).await? {
                tx.commit().await?;
                return Ok(Some(reference));
            }
        }
        tx.commit().await?;
        Ok(None)
    }

    pub(crate) async fn resolve_legacy_delivery(&self, owner: &str) -> Result<bool> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let rows=sqlx::query("SELECT m.document_json FROM delivery_messages m WHERE m.owner=? AND json_extract(m.document_json,'$.requires_joining')=1 AND json_type(m.document_json,'$.conversation')='null' AND NOT EXISTS(SELECT 1 FROM delivery_expansions e WHERE e.owner=m.owner AND e.identity=m.identity) AND EXISTS(SELECT 1 FROM delivery_branches b JOIN delivery_destinations d ON d.owner=b.owner AND d.identity=b.starter AND d.branch=b.branch JOIN delivery_receipts r ON r.owner=d.owner AND r.identity=d.identity AND r.destination=d.destination WHERE b.owner=m.owner AND b.member=json_extract(m.document_json,'$.member') AND b.cycle=json_extract(m.document_json,'$.cycle')) ORDER BY m.source_seq LIMIT 32")
            .bind(owner).fetch_all(&mut *tx).await?;
        for row in rows {
            let message: Rendered =
                serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
            let branches=sqlx::query("SELECT b.* FROM delivery_branches b WHERE b.owner=? AND b.cycle=? AND b.member=? ORDER BY b.starter LIMIT 32")
                .bind(owner).bind(message.cycle).bind(&message.member).fetch_all(&mut *tx).await?;
            for branch in branches {
                let key: String = branch.try_get("branch")?;
                if joining_reference(&mut tx, owner, &key).await?.is_none() {
                    continue;
                }
                let target: ConversationTarget =
                    serde_json::from_str(&branch.try_get::<String, _>("target_json")?)?;
                let mut context = crate::cyberherd::conversation::Context::default();
                if target.kind == 1 {
                    context.note = Some(target);
                } else {
                    context.stream = Some(target);
                }
                let conversation = MemberConversation {
                    generation: branch.try_get("generation")?,
                    context,
                };
                conversation.validate(owner)?;
                sqlx::query("INSERT INTO delivery_expansions(owner,identity,conversation_json) VALUES(?,?,?)")
                    .bind(owner).bind(&message.identity).bind(serde_json::to_string(&conversation)?).execute(&mut *tx).await?;
                insert_destinations(&mut tx, owner, &message, Some(&conversation)).await?;
                tx.commit().await?;
                return Ok(true);
            }
        }
        tx.commit().await?;
        Ok(false)
    }
    pub(crate) async fn pending_member_overlay(
        &self,
        owner: &str,
    ) -> Result<Option<crate::cyberherd::overlay_delivery::Message>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let binding: String =
            sqlx::query_scalar("SELECT binding_json FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?;
        let binding: crate::cyberherd::ingress::CoreBinding = serde_json::from_str(&binding)?;
        let rows=sqlx::query("SELECT m.document_json FROM delivery_overlay o JOIN delivery_messages m ON m.owner=o.owner AND m.identity=o.identity WHERE o.owner=? AND o.event_seq IS NULL ORDER BY o.attempted_at,m.source_seq LIMIT 32")
            .bind(owner).fetch_all(&mut *tx).await?;
        for row in rows {
            let message: Rendered =
                serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
            if message.requires_joining {
                let branches:Vec<String>=sqlx::query_scalar("SELECT branch FROM delivery_destinations WHERE owner=? AND identity=? AND branch IS NOT NULL")
                    .bind(owner).bind(&message.identity).fetch_all(&mut *tx).await?;
                let mut ready = false;
                for branch in branches {
                    if joining_reference(&mut tx, owner, &branch).await?.is_some() {
                        ready = true;
                        break;
                    }
                }
                if !ready {
                    sqlx::query("UPDATE delivery_overlay SET attempted_at=MAX(attempted_at,?) WHERE owner=? AND identity=?")
                        .bind(crate::cyberherd::calendar::now()?).bind(owner).bind(&message.identity).execute(&mut *tx).await?;
                    continue;
                }
            }
            let value = crate::cyberherd::overlay_delivery::Message {
                epoch: binding.epoch,
                owner: owner.into(),
                identity: message.identity,
                category: message.category,
                message: message.overlay,
            };
            value.validate()?;
            tx.commit().await?;
            return Ok(Some(value));
        }
        tx.commit().await?;
        Ok(None)
    }
    pub(crate) async fn acknowledge_member_overlay(
        &self,
        message: &crate::cyberherd::overlay_delivery::Message,
        seq: u64,
    ) -> Result<()> {
        message.validate()?;
        if seq == 0 {
            bail!("invalid overlay receipt sequence");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row=sqlx::query("SELECT m.document_json,o.event_seq FROM delivery_messages m JOIN delivery_overlay o ON o.owner=m.owner AND o.identity=m.identity WHERE m.owner=? AND m.identity=?")
            .bind(&message.owner).bind(&message.identity).fetch_one(&mut *tx).await?;
        let original: Rendered = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
        if original.category != message.category
            || original.overlay != message.message
            || row
                .try_get::<Option<i64>, _>("event_seq")?
                .is_some_and(|n| n != i64::try_from(seq).unwrap_or(-1))
        {
            bail!("overlay receipt changed original message");
        }
        sqlx::query("UPDATE delivery_overlay SET event_seq=? WHERE owner=? AND identity=?")
            .bind(i64::try_from(seq)?)
            .bind(&message.owner)
            .bind(&message.identity)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

pub(super) async fn insert_destinations(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    message: &Rendered,
    override_conversation: Option<&MemberConversation>,
) -> Result<()> {
    let context = override_conversation
        .map(|c| &c.context)
        .unwrap_or(&message.context);
    for target in context.targets() {
        let destination = destination(target)?;
        let conversation = override_conversation.or(message.conversation.as_ref());
        let branch = if message.accepted || message.requires_joining {
            conversation
                .map(|c| branch_key(owner, message.cycle, &message.member, &c.generation, target))
                .transpose()?
        } else {
            None
        };
        if message.accepted {
            if let (Some(branch), Some(conversation)) = (&branch, conversation) {
                sqlx::query("INSERT INTO delivery_branches(owner,branch,cycle,member,generation,target_json,starter) VALUES(?,?,?,?,?,?,?) ON CONFLICT(owner,branch) DO NOTHING")
                    .bind(owner).bind(branch).bind(message.cycle).bind(&message.member).bind(&conversation.generation).bind(serde_json::to_string(target)?).bind(&message.identity).execute(&mut **tx).await?;
            }
        }
        // A treat never becomes a standalone post because its branch is absent.
        if message.requires_joining && branch.is_none() {
            continue;
        }
        sqlx::query("INSERT INTO delivery_destinations(owner,identity,destination,branch,target_json) VALUES(?,?,?,?,?) ON CONFLICT(owner,identity,destination) DO NOTHING")
            .bind(owner).bind(&message.identity).bind(&destination).bind(branch).bind(serde_json::to_string(target)?).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO delivery_work(owner,identity,destination) VALUES(?,?,?) ON CONFLICT(owner,identity,destination) DO NOTHING")
            .bind(owner).bind(&message.identity).bind(destination).execute(&mut **tx).await?;
    }
    Ok(())
}
fn destination(target: &ConversationTarget) -> Result<String> {
    match target.kind {
        1 => Ok(format!("note:{}", target.event_id)),
        30311 => Ok(format!(
            "stream:{}",
            target
                .coordinate
                .as_deref()
                .context("stream missing address")?
        )),
        _ => bail!("invalid public member destination"),
    }
}
fn branch_key(
    owner: &str,
    cycle: i64,
    member: &str,
    generation: &str,
    target: &ConversationTarget,
) -> Result<String> {
    super::super::ingress::hash(&(owner, cycle, member, generation, destination(target)?))
}
async fn joining_reference(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    branch: &str,
) -> Result<Option<PublishedReference>> {
    let row=sqlx::query("SELECT b.member,s.event_json,r.relay,r.acknowledged_at FROM delivery_branches b JOIN delivery_destinations d ON d.owner=b.owner AND d.identity=b.starter AND d.branch=b.branch JOIN delivery_signed s ON s.owner=d.owner AND s.identity=d.identity AND s.destination=d.destination JOIN delivery_receipts r ON r.owner=s.owner AND r.identity=s.identity AND r.destination=s.destination WHERE b.owner=? AND b.branch=? ORDER BY r.acknowledged_at,r.relay LIMIT 1")
        .bind(owner).bind(branch).fetch_optional(&mut **tx).await?;
    row.map(|row| -> Result<_> {
        let reference = PublishedReference {
            event: serde_json::from_str::<SignedNostrEvent>(
                &row.try_get::<String, _>("event_json")?,
            )?,
            relay: row.try_get("relay")?,
            acknowledged_at: row.try_get("acknowledged_at")?,
        };
        reference.validate(owner, &row.try_get::<String, _>("member")?)?;
        Ok(reference)
    })
    .transpose()
}
fn unsigned(
    owner: &str,
    message: &Rendered,
    target: &ConversationTarget,
    joining: Option<&PublishedReference>,
    now: i64,
) -> Result<Unsigned> {
    message.context.validate(owner)?;
    let mut tags = vec![
        vec!["t".into(), "CyberHerd".into()],
        vec!["p".into(), message.member.clone()],
    ];
    for member in &message.additional_members {
        crate::nostr_event::validate_public_key(member)?;
        if member != &message.member {
            tags.push(vec!["p".into(), member.clone()]);
        }
    }
    let kind = if target.kind == 30311 {
        tags.push(vec![
            "a".into(),
            target
                .coordinate
                .clone()
                .context("stream missing address")?,
            String::new(),
            "root".into(),
        ]);
        1311
    } else {
        tags.push(vec![
            "e".into(),
            target.event_id.clone(),
            String::new(),
            "root".into(),
        ]);
        1
    };
    if let Some(joining) = joining {
        joining.validate(owner, &message.member)?;
        tags.push(vec![
            "e".into(),
            joining.event.id.clone(),
            joining.relay.clone(),
            "reply".into(),
        ]);
    } else if kind == 1 {
        tags.push(vec![
            "e".into(),
            target.event_id.clone(),
            String::new(),
            "reply".into(),
        ]);
    }
    Ok(Unsigned {
        owner: owner.into(),
        kind,
        created_at: u64::try_from(now)?,
        content: message.nostr.clone(),
        tags,
    })
}
async fn require_job(tx: &mut Transaction<'_, Sqlite>, job: &Job) -> Result<()> {
    let row=sqlx::query("SELECT w.lease_id,u.document_json FROM delivery_work w JOIN delivery_unsigned u ON u.owner=w.owner AND u.identity=w.identity AND u.destination=w.destination WHERE w.owner=? AND w.identity=? AND w.destination=?")
        .bind(&job.owner).bind(&job.identity).bind(&job.destination).fetch_one(&mut **tx).await?;
    if row.try_get::<Option<String>, _>("lease_id")?.as_deref() != Some(&job.lease.to_string())
        || serde_json::from_str::<Unsigned>(&row.try_get::<String, _>("document_json")?)?
            != job.unsigned
    {
        bail!("delivery lease or frozen unsigned message changed");
    }
    Ok(())
}
