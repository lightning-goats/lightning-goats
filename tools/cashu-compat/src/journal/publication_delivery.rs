//! Frozen destinations and independent positive acknowledgments for exact signed bytes.
use super::*;
use crate::{publication::Intent, publication_relay::Discovery};
use lightning_goats_nostr_event::VerifiedEvent;
use std::collections::BTreeSet;

pub struct Delivery {
    pub publication: String,
    pub relay: String,
    pub event: String,
    pub event_id: String,
}
fn target(raw: &str) -> Result<String> {
    let url = crate::relay::validate_relay(raw)?;
    ensure!(
        url.query().is_none(),
        "publication relay cannot contain a query"
    );
    Ok(url.to_string())
}
impl Journal {
    pub fn claim_publication_routing(
        &mut self,
        now: i64,
        maximum_attempts: u32,
        announcements: bool,
        redemptions: bool,
    ) -> Result<Option<String>> {
        ensure!(
            now >= 0 && maximum_attempts <= 10_000,
            "invalid routing retry budget"
        );
        if maximum_attempts == 0 || (!announcements && !redemptions) {
            return Ok(None);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row:Option<(String,u32)>=tx.query_row("SELECT p.id,COALESCE(s.attempts,0) FROM nostr_publications p LEFT JOIN publication_routing_schedule s ON s.publication_id=p.id WHERE p.signed_event IS NOT NULL AND p.published=0 AND ((? AND p.id LIKE 'announcement:%') OR (? AND p.id LIKE 'redemption:%')) AND NOT EXISTS(SELECT 1 FROM publication_routes r WHERE r.publication_id=p.id) AND COALESCE(s.attempts,0)<? AND COALESCE(s.next_attempt,0)<=? ORDER BY COALESCE(s.next_attempt,0),p.rowid LIMIT 1",params![announcements,redemptions,maximum_attempts,now],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((id, attempts)) = row else {
            return Ok(None);
        };
        let delay = (30_i64 << attempts.min(7)).min(3600);
        let next = now
            .checked_add(delay)
            .ok_or_else(|| anyhow::anyhow!("routing retry clock overflow"))?;
        tx.execute("INSERT INTO publication_routing_schedule(publication_id,attempts,next_attempt) VALUES(?,?,?) ON CONFLICT(publication_id) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt",params![id,attempts+1,next])?;
        tx.commit()?;
        Ok(Some(id))
    }
    pub fn next_unrouted_publication(&self) -> Result<Option<String>> {
        self.connection.query_row("SELECT p.id FROM nostr_publications p WHERE p.signed_event IS NOT NULL AND p.published=0 AND NOT EXISTS(SELECT 1 FROM publication_routes r WHERE r.publication_id=p.id) ORDER BY p.rowid LIMIT 1",[],|r|r.get(0)).optional().map_err(Into::into)
    }
    /// Freeze destinations only after native verification of the signed event and sender list.
    pub fn freeze_publication_routes(
        &mut self,
        id: &str,
        base: &[String],
        discovery: &Discovery,
        now: i64,
    ) -> Result<()> {
        ensure!(
            now >= 0 && !base.is_empty() && base.len() <= 16,
            "invalid publication route plan"
        );
        let intent = self.publication_intent(id)?;
        ensure!(
            self.signed_publication(id)?.is_some(),
            "publication has no verified signature"
        );
        let mut targets = base
            .iter()
            .map(|raw| target(raw))
            .collect::<Result<BTreeSet<_>>>()?;
        match &intent {
            Intent::Announcement { .. } => ensure!(
                discovery.signed_list.is_none() && discovery.read_relays.is_empty(),
                "announcement cannot adopt a sender relay list"
            ),
            Intent::Redemption { sender, .. } => {
                let reads = match &discovery.signed_list {
                    Some(raw) => crate::publication_relay::sender_reads(
                        &VerifiedEvent::from_json(raw)?,
                        sender,
                        now,
                    )?,
                    None => vec![],
                };
                ensure!(
                    reads == discovery.read_relays,
                    "sender routes differ from signed relay-list evidence"
                );
                for relay in reads {
                    targets.insert(target(&relay)?);
                }
            }
        }
        ensure!(targets.len() <= 24, "publication route set exceeds bound");
        let targets = targets.into_iter().collect::<Vec<_>>();
        let encoded = serde_json::to_vec(&targets)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(Vec<u8>, Option<String>)> = tx
            .query_row(
                "SELECT targets,signed_list FROM publication_routes WHERE publication_id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((old, evidence)) = existing {
            ensure!(
                old == encoded && evidence == discovery.signed_list,
                "publication destinations changed after freeze"
            );
        } else {
            tx.execute("INSERT INTO publication_routes(publication_id,targets,signed_list,planned_at) VALUES(?,?,?,?)",params![id,encoded,discovery.signed_list,now])?;
            for relay in targets {
                tx.execute(
                    "INSERT INTO publication_delivery(publication_id,relay) VALUES(?,?)",
                    params![id, relay],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn claim_publication_delivery(
        &mut self,
        now: i64,
        maximum_attempts: u32,
    ) -> Result<Option<Delivery>> {
        self.claim_publication_delivery_for(now, maximum_attempts, true, true)
    }
    pub fn claim_publication_delivery_for(
        &mut self,
        now: i64,
        maximum_attempts: u32,
        announcements: bool,
        redemptions: bool,
    ) -> Result<Option<Delivery>> {
        ensure!(
            now >= 0 && maximum_attempts <= 10_000,
            "invalid relay retry budget"
        );
        if maximum_attempts == 0 {
            return Ok(None);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row:Option<(String,String,u32)>=tx.query_row("SELECT d.publication_id,d.relay,d.attempts FROM publication_delivery d JOIN nostr_publications p ON p.id=d.publication_id WHERE d.acknowledged=0 AND p.published=0 AND ((? AND p.id LIKE 'announcement:%') OR (? AND p.id LIKE 'redemption:%')) AND d.attempts<? AND d.next_attempt<=? ORDER BY d.next_attempt,d.rowid LIMIT 1",params![announcements,redemptions,maximum_attempts,now],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((id, relay, attempts)) = row else {
            return Ok(None);
        };
        let delay = (30_i64 << attempts.min(7)).min(3600);
        let next = now
            .checked_add(delay)
            .ok_or_else(|| anyhow::anyhow!("relay retry clock overflow"))?;
        tx.execute("UPDATE publication_delivery SET attempts=attempts+1,next_attempt=? WHERE publication_id=? AND relay=?",params![next,id,relay])?;
        tx.commit()?;
        let event = self
            .signed_publication(&id)?
            .ok_or_else(|| anyhow::anyhow!("signed publication missing"))?;
        let event_id = VerifiedEvent::from_json(&event)?.event().id.clone();
        target(&relay)?;
        Ok(Some(Delivery {
            publication: id,
            relay,
            event,
            event_id,
        }))
    }
    /// Positive relay acknowledgment is not proof of sender visibility or another credit.
    pub fn acknowledge_publication_relay(
        &mut self,
        id: &str,
        relay: &str,
        event_id: &str,
    ) -> Result<()> {
        let raw = self
            .signed_publication(id)?
            .ok_or_else(|| anyhow::anyhow!("signed publication missing"))?;
        ensure!(
            VerifiedEvent::from_json(&raw)?.event().id == event_id,
            "relay acknowledged a different event"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed=tx.execute("UPDATE publication_delivery SET acknowledged=1 WHERE publication_id=? AND relay=? AND attempts>0",params![id,relay])?;
        ensure!(changed == 1, "relay acknowledgment has no original attempt");
        tx.execute("UPDATE nostr_publications SET published=1 WHERE id=? AND EXISTS(SELECT 1 FROM publication_delivery d WHERE d.publication_id=nostr_publications.id) AND NOT EXISTS(SELECT 1 FROM publication_delivery d WHERE d.publication_id=nostr_publications.id AND d.acknowledged=0)",[id])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::tests::{announcement, sign};
    #[tokio::test]
    async fn partial_ack_restart_and_lost_reply_keep_original_event_and_destinations() -> Result<()>
    {
        let dir = crate::test_directory()?;
        let epoch = Uuid::new_v4();
        let path = dir.path().join("journal.sqlite");
        let mut journal = Journal::open(&path, epoch, 8)?;
        let id = Uuid::new_v4();
        let operation = format!("announcement:{id}");
        journal.prepare_announcement(id, &announcement())?;
        let base = vec!["wss://a.example/".into(), "wss://b.example/".into()];
        let discovery = Discovery {
            read_relays: vec![],
            signed_list: None,
        };
        assert!(
            journal
                .freeze_publication_routes(&operation, &base, &discovery, 100)
                .is_err()
        );
        let raw = format!(" {}\n", sign(journal.publication_unsigned(&operation)?)?);
        journal.freeze_signed_publication(&operation, &raw)?;
        journal.freeze_publication_routes(&operation, &base, &discovery, 100)?;
        assert!(journal.next_unrouted_publication()?.is_none());
        let first = journal.claim_publication_delivery(100, 2)?.unwrap();
        assert_eq!(first.relay, base[0]);
        assert_eq!(first.event, raw);
        assert!(
            journal
                .acknowledge_publication_relay(&operation, &first.relay, &"ff".repeat(32))
                .is_err()
        );
        journal.acknowledge_publication_relay(&operation, &first.relay, &first.event_id)?;
        let second = journal.claim_publication_delivery(101, 2)?.unwrap();
        assert_eq!(second.relay, base[1]);
        drop(journal); // Simulate a positive relay response lost before local retention.
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert!(journal.claim_publication_delivery(130, 2)?.is_none());
        let replay = journal.claim_publication_delivery(131, 2)?.unwrap();
        assert_eq!(replay.event, raw);
        assert_eq!(replay.relay, second.relay);
        assert!(
            journal
                .freeze_publication_routes(
                    &operation,
                    &["wss://different.example/".into()],
                    &discovery,
                    131
                )
                .is_err()
        );
        assert!(journal.claim_publication_delivery(10000, 2)?.is_none());
        journal.acknowledge_publication_relay(&operation, &replay.relay, &replay.event_id)?;
        journal.acknowledge_publication_relay(&operation, &replay.relay, &replay.event_id)?;
        let published: bool = journal.connection.query_row(
            "SELECT published FROM nostr_publications WHERE id=?",
            [&operation],
            |row| row.get(0),
        )?;
        assert!(published);
        assert!(journal.claim_publication_delivery(20000, 2)?.is_none());
        assert_eq!(journal.signed_publication(&operation)?, Some(raw));
        Ok(())
    }
    #[tokio::test]
    async fn redemption_routes_require_exact_sender_signed_read_evidence() -> Result<()> {
        use crate::publication::{
            Unsigned,
            tests::{envelope, owner},
        };
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let proofs = [crate::intake::tests::proof(
            crate::intake::tests::locked().secret.to_string(),
        )];
        let author = owner();
        let source = "ab".repeat(32);
        let Admission::New(id) = journal.admit(&ValidatedIntake {
            mint_id: "fixture",
            policy_version: "v1",
            transport: Transport::Nutzap,
            source_ref: &source,
            source_author: Some(&author),
            proofs: &proofs,
        })?
        else {
            anyhow::bail!("fixture must be fresh")
        };
        journal.prepare(id, b"synthetic swap")?;
        journal.start_acquisition(id)?;
        journal.record_acquired(id, 7, 1, b"synthetic outputs")?;
        journal.prepare_redemption(id, &author, 100)?;
        let op = format!("redemption:{id}");
        journal.freeze_history_ciphertext(&op, &envelope(9))?;
        journal.freeze_signed_publication(&op, &sign(journal.publication_unsigned(&op)?)?)?;
        let list = sign(Unsigned {
            pubkey: author.clone(),
            created_at: 100,
            kind: 10002,
            content: String::new(),
            tags: vec![
                vec!["r".into(), "wss://read.example/".into(), "read".into()],
                vec!["r".into(), "wss://write.example/".into(), "write".into()],
            ],
        })?;
        let base = vec!["wss://base.example/".into()];
        let mut discovery = Discovery {
            read_relays: vec!["wss://write.example/".into()],
            signed_list: Some(list),
        };
        assert!(
            journal
                .freeze_publication_routes(&op, &base, &discovery, 100)
                .is_err()
        );
        discovery.read_relays = vec!["wss://read.example/".into()];
        assert!(
            journal
                .freeze_publication_routes(&op, &base, &discovery, 99)
                .is_err()
        );
        journal.freeze_publication_routes(&op, &base, &discovery, 100)?;
        let first = journal.claim_publication_delivery(100, 1)?.unwrap();
        let second = journal.claim_publication_delivery(100, 1)?.unwrap();
        assert_eq!(
            [first.relay, second.relay],
            ["wss://base.example/", "wss://read.example/"]
        );
        assert!(journal.claim_publication_delivery(100, 1)?.is_none());
        assert_eq!(journal.state(id)?, "acquired");
        assert_eq!(journal.export_receipt(id)?.net_sats, 7);
        Ok(())
    }
    #[tokio::test]
    async fn routing_claims_are_durable_bounded_and_transactional() -> Result<()> {
        let dir = crate::test_directory()?;
        let epoch = Uuid::new_v4();
        let path = dir.path().join("journal.sqlite");
        let mut journal = Journal::open(&path, epoch, 8)?;
        let mut operations = Vec::new();
        for _ in 0..2 {
            let id = Uuid::new_v4();
            let op = format!("announcement:{id}");
            journal.prepare_announcement(id, &announcement())?;
            journal.freeze_signed_publication(&op, &sign(journal.publication_unsigned(&op)?)?)?;
            operations.push(op);
        }
        assert!(
            journal
                .claim_publication_routing(100, 1, false, true)?
                .is_none()
        );
        journal.connection.execute_batch("CREATE TRIGGER fail_routing_claim BEFORE INSERT ON publication_routing_schedule BEGIN SELECT RAISE(ABORT,'fixture'); END;")?;
        assert!(
            journal
                .claim_publication_routing(100, 1, true, true)
                .is_err()
        );
        journal
            .connection
            .execute_batch("DROP TRIGGER fail_routing_claim;")?;
        assert_eq!(
            journal.claim_publication_routing(100, 1, true, false)?,
            Some(operations[0].clone())
        );
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert_eq!(
            journal.claim_publication_routing(101, 1, true, false)?,
            Some(operations[1].clone())
        );
        assert!(
            journal
                .claim_publication_routing(9999, 1, true, true)?
                .is_none()
        );
        assert_eq!(
            journal.claim_publication_routing(9999, 2, true, false)?,
            Some(operations[0].clone())
        );
        assert!(
            journal
                .claim_publication_routing(i64::MAX, 2, true, false)
                .is_err()
        );
        assert_eq!(
            journal.claim_publication_routing(9999, 2, true, false)?,
            Some(operations[1].clone())
        );
        Ok(())
    }
    #[tokio::test]
    async fn failed_route_transaction_never_leaves_partial_destinations() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let id = Uuid::new_v4();
        let op = format!("announcement:{id}");
        journal.prepare_announcement(id, &announcement())?;
        journal.freeze_signed_publication(&op, &sign(journal.publication_unsigned(&op)?)?)?;
        let discovery = Discovery {
            read_relays: vec![],
            signed_list: None,
        };
        journal.connection.execute_batch("CREATE TRIGGER fail_route BEFORE INSERT ON publication_delivery BEGIN SELECT RAISE(ABORT,'fixture'); END;")?;
        assert!(
            journal
                .freeze_publication_routes(&op, &["wss://relay.example/".into()], &discovery, 100)
                .is_err()
        );
        assert_eq!(journal.next_unrouted_publication()?, Some(op.clone()));
        assert!(journal.claim_publication_delivery(100, 2)?.is_none());
        journal
            .connection
            .execute_batch("DROP TRIGGER fail_route;")?;
        assert!(
            journal
                .freeze_publication_routes(&op, &["wss://10.8.0.6/".into()], &discovery, 100)
                .is_err()
        );
        journal.freeze_publication_routes(
            &op,
            &["wss://relay.example/".into()],
            &discovery,
            100,
        )?;
        let event = VerifiedEvent::from_json(&journal.signed_publication(&op)?.unwrap())?;
        assert!(
            journal
                .acknowledge_publication_relay(&op, "wss://relay.example/", &event.event().id)
                .is_err()
        );
        assert!(journal.claim_publication_delivery(i64::MAX, 2).is_err());
        assert!(journal.claim_publication_delivery(100, 2)?.is_some());
        Ok(())
    }
}
