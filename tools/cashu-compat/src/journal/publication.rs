use super::*;
use crate::publication::Intent;

impl Journal {
    /// An explicit operator ID freezes one announcement. Retrying preserves its
    /// original timestamp while still requiring the current advertised policy to match.
    pub(crate) fn prepare_configured_announcement(
        &mut self,
        id: Uuid,
        config: &crate::service_config::Config,
        now: u64,
    ) -> Result<Intent> {
        ensure!(
            !id.is_nil(),
            "announcement requires a nonempty operation ID"
        );
        let operation = format!("announcement:{id}");
        let bytes: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT intent FROM nostr_publications WHERE id=?",
                [&operation],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = match bytes {
            Some(bytes) => {
                ensure!(bytes.len() <= 65_536, "stored announcement exceeds bound");
                let previous: Intent = serde_json::from_slice(&bytes)?;
                previous.validate()?;
                let Intent::Announcement { created_at, .. } = previous else {
                    anyhow::bail!("announcement purpose changed")
                };
                created_at
            }
            None => {
                let latest:Option<u64>=self.connection.query_row("SELECT MAX(json_extract(intent,'$.created_at')) FROM nostr_publications WHERE id LIKE 'announcement:%'",[],|row|row.get(0))?;
                ensure!(
                    latest.is_none_or(|previous| now > previous),
                    "new announcement requires a later timestamp; retry the original operation or correct the clock"
                );
                now
            }
        };
        let intent = Intent::announcement(config, created_at)?;
        self.prepare_announcement(id, &intent)?;
        Ok(intent)
    }
    /// Stage one acquired nutzap without blocking acquisition on presentation backlog.
    pub fn stage_next_redemption(&mut self, owner: &str, now: i64) -> Result<Option<Uuid>> {
        ensure!(now >= 0, "invalid publication time");
        let pending: u64 = self.connection.query_row(
            "SELECT COUNT(*) FROM nostr_publications WHERE published=0",
            [],
            |row| row.get(0),
        )?;
        if pending >= 1024 {
            return Ok(None);
        }
        let id: Option<String> = self.connection.query_row(
            "SELECT a.id FROM acquisitions a JOIN acquisition_context c ON c.acquisition_id=a.id WHERE a.transport='nutzap' AND a.state IN ('acquired','credit_export_pending','credited') AND a.acquired_evidence IS NOT NULL AND a.net>0 AND c.source_author IS NOT NULL AND NOT EXISTS(SELECT 1 FROM nostr_publications p WHERE p.id='redemption:'||a.id) ORDER BY a.rowid LIMIT 1",
            [], |row| row.get(0),
        ).optional()?;
        let Some(id) = id else {
            return Ok(None);
        };
        let id = Uuid::parse_str(&id)?;
        self.prepare_redemption(id, owner, u64::try_from(now)?)?;
        Ok(Some(id))
    }
    /// Claim before IPC. The 180-second minimum exceeds the broker client's deadline.
    /// Failed or cancelled work keeps its attempt/backoff, allowing later rows to proceed.
    pub fn claim_publication_signing(
        &mut self,
        now: i64,
        maximum_attempts: u32,
        announcements: bool,
        redemptions: bool,
    ) -> Result<Option<String>> {
        ensure!(
            now >= 0 && maximum_attempts <= 10_000,
            "invalid publication retry limits"
        );
        if maximum_attempts == 0 || (!announcements && !redemptions) {
            return Ok(None);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String,u32)> = tx.query_row(
            "SELECT p.id,COALESCE(s.attempts,0) FROM nostr_publications p LEFT JOIN publication_signing_schedule s ON s.publication_id=p.id WHERE p.published=0 AND p.signed_event IS NULL AND ((? AND p.id LIKE 'announcement:%') OR (? AND p.id LIKE 'redemption:%')) AND COALESCE(s.attempts,0)<? AND COALESCE(s.next_attempt,0)<=? ORDER BY COALESCE(s.next_attempt,0),p.rowid LIMIT 1",
            params![announcements,redemptions,maximum_attempts,now], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        let Some((id, attempts)) = row else {
            return Ok(None);
        };
        let delay = (180_i64 << attempts.min(5)).min(3600);
        let next = now
            .checked_add(delay)
            .ok_or_else(|| anyhow::anyhow!("publication retry clock overflow"))?;
        tx.execute("INSERT INTO publication_signing_schedule(publication_id,attempts,next_attempt) VALUES(?,?,?) ON CONFLICT(publication_id) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt",params![id,attempts+1,next])?;
        tx.commit()?;
        Ok(Some(id))
    }
    /// Freeze an operator-prepared announcement under an explicit stable operation ID.
    pub fn prepare_announcement(&mut self, id: Uuid, intent: &Intent) -> Result<()> {
        ensure!(
            !id.is_nil() && matches!(intent, Intent::Announcement { .. }),
            "invalid announcement operation"
        );
        intent.unsigned(None)?;
        self.freeze_publication(&format!("announcement:{id}"), intent)
    }
    /// Authoritative wallet acquisition, not public receipt claims, creates history.
    /// Preparing history does not export a receipt or modify credit/delivery state.
    pub fn prepare_redemption(&mut self, id: Uuid, owner: &str, created_at: u64) -> Result<Intent> {
        let (source,sender,net): (String,String,u64) = self.connection.query_row(
            "SELECT a.source_ref,c.source_author,a.net FROM acquisitions a JOIN acquisition_context c ON c.acquisition_id=a.id WHERE a.id=? AND a.transport='nutzap' AND a.state IN ('acquired','credit_export_pending','credited') AND a.acquired_evidence IS NOT NULL AND a.net>0",
            [id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        )?;
        let key = format!("redemption:{id}");
        let existing: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT intent FROM nostr_publications WHERE id=?",
                [&key],
                |r| r.get(0),
            )
            .optional()?;
        let intent = if let Some(bytes) = existing {
            ensure!(bytes.len() <= 65_536, "stored publication exceeds bound");
            let intent: Intent = serde_json::from_slice(&bytes)?;
            // Ignore a new wall-clock time on retry; all source and owner bindings remain exact.
            ensure!(
                matches!(&intent,Intent::Redemption {owner:previous,acquisition_id,source_event,sender:previous_sender,net_sats,..} if previous==owner && *acquisition_id==id && source_event==&source && previous_sender==&sender && *net_sats==net),
                "redemption binding changed"
            );
            intent
        } else {
            Intent::Redemption {
                owner: owner.into(),
                created_at,
                acquisition_id: id,
                source_event: source,
                sender,
                net_sats: net,
            }
        };
        intent.validate()?;
        self.freeze_publication(&key, &intent)?;
        Ok(intent)
    }
    fn freeze_publication(&mut self, id: &str, intent: &Intent) -> Result<()> {
        intent.validate()?;
        let encoded = serde_json::to_vec(intent)?;
        ensure!(encoded.len() <= 65_536, "publication intent exceeds bound");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<Vec<u8>> = tx
            .query_row(
                "SELECT intent FROM nostr_publications WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            ensure!(existing == encoded, "publication operation changed");
        } else {
            let pending: i64 = tx.query_row(
                "SELECT COUNT(*) FROM nostr_publications WHERE published=0",
                [],
                |r| r.get(0),
            )?;
            ensure!(pending < 1024, "publication backlog capacity exhausted");
            tx.execute(
                "INSERT INTO nostr_publications(id,intent) VALUES(?,?)",
                params![id, encoded],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn publication_intent(&self, id: &str) -> Result<Intent> {
        ensure!(id.len() <= 64, "invalid publication operation");
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT intent FROM nostr_publications WHERE id=?",
            [id],
            |r| r.get(0),
        )?;
        ensure!(bytes.len() <= 65_536, "stored publication exceeds bound");
        let intent: Intent = serde_json::from_slice(&bytes)?;
        intent.validate()?;
        Ok(intent)
    }
    /// Store the broker's original encryption before asking it to sign. Never
    /// replace ciphertext after loss of a signing or publishing acknowledgment.
    pub fn freeze_history_ciphertext(&mut self, id: &str, ciphertext: &str) -> Result<()> {
        let intent = self.publication_intent(id)?;
        ensure!(
            matches!(intent, Intent::Redemption { .. }),
            "history encryption requires redemption intent"
        );
        intent.unsigned(Some(ciphertext))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<String> = tx.query_row(
            "SELECT ciphertext FROM nostr_publications WHERE id=?",
            [id],
            |r| r.get(0),
        )?;
        ensure!(
            previous.as_deref().is_none_or(|value| value == ciphertext),
            "history ciphertext changed"
        );
        tx.execute(
            "UPDATE nostr_publications SET ciphertext=? WHERE id=? AND ciphertext IS NULL",
            params![ciphertext, id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn publication_unsigned(&self, id: &str) -> Result<crate::publication::Unsigned> {
        let intent = self.publication_intent(id)?;
        let ciphertext: Option<String> = self.connection.query_row(
            "SELECT ciphertext FROM nostr_publications WHERE id=?",
            [id],
            |r| r.get(0),
        )?;
        intent.unsigned(ciphertext.as_deref())
    }
    pub fn freeze_signed_publication(&mut self, id: &str, raw: &str) -> Result<()> {
        let intent = self.publication_intent(id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (ciphertext, previous): (Option<String>, Option<String>) = tx.query_row(
            "SELECT ciphertext,signed_event FROM nostr_publications WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        intent.verify_signed(ciphertext.as_deref(), raw)?;
        // Keep the exact first response bytes, including harmless JSON whitespace.
        if let Some(previous) = previous {
            ensure!(previous == raw, "signed publication bytes changed");
        } else {
            tx.execute(
                "UPDATE nostr_publications SET signed_event=? WHERE id=?",
                params![raw, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Registration and the immutable journal intent supply the entire broker request.
    pub fn publication_signing_request(
        &self,
        id: &str,
    ) -> Result<crate::publication_broker::Request> {
        let intent = self.publication_intent(id)?;
        let (prefix, operation) = id
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid publication operation"))?;
        ensure!(
            matches!(
                (&intent, prefix),
                (Intent::Announcement { .. }, "announcement")
                    | (Intent::Redemption { .. }, "redemption")
            ),
            "publication purpose mismatch"
        );
        let instance: String = self.connection.query_row(
            "SELECT instance FROM runtime_registration WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        let epoch: String = self.connection.query_row(
            "SELECT epoch FROM wallet_epoch WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        let request = crate::publication_broker::Request {
            version: 1,
            wallet_instance: Uuid::parse_str(&instance)?,
            wallet_epoch: Uuid::parse_str(&epoch)?,
            operation_id: Uuid::parse_str(operation)?,
            intent,
        };
        crate::publication_broker::Request::decode(&serde_json::to_vec(&request)?)
    }
    /// Call only with a kernel-authenticated broker response. Encryption and exact
    /// signed bytes are retained atomically, independently of financial state.
    pub fn accept_signed_publication(
        &mut self,
        id: &str,
        result: &crate::publication_broker::Signed,
    ) -> Result<()> {
        let request = self.publication_signing_request(id)?;
        result.validate_for(&request)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (ciphertext, raw): (Option<String>, Option<String>) = tx.query_row(
            "SELECT ciphertext,signed_event FROM nostr_publications WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            ciphertext
                .as_ref()
                .is_none_or(|v| Some(v) == result.ciphertext.as_ref())
                && raw.as_ref().is_none_or(|v| v == &result.event_json),
            "publication response changed retained bytes"
        );
        tx.execute(
            "UPDATE nostr_publications SET ciphertext=?,signed_event=? WHERE id=?",
            params![result.ciphertext, result.event_json, id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn signed_publication(&self, id: &str) -> Result<Option<String>> {
        let intent = self.publication_intent(id)?;
        let (ciphertext, raw): (Option<String>, Option<String>) = self.connection.query_row(
            "SELECT ciphertext,signed_event FROM nostr_publications WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if let Some(raw) = &raw {
            intent.verify_signed(ciphertext.as_deref(), raw)?;
        }
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::tests::{announcement, envelope, owner, sign};
    fn admit(journal: &mut Journal, transport: Transport, nonce: &str) -> Result<Uuid> {
        let proof = crate::intake::tests::proof(
            crate::intake::tests::locked()
                .secret
                .to_string()
                .replace("fixture", nonce),
        );
        let proofs = [proof];
        let author = owner();
        let source = hex::encode(Sha256::digest(nonce.as_bytes()));
        let intake = ValidatedIntake {
            mint_id: "fixture",
            policy_version: "v1",
            transport,
            source_ref: &source,
            source_author: matches!(transport, Transport::Nutzap).then_some(author.as_str()),
            proofs: &proofs,
        };
        Ok(match journal.admit(&intake)? {
            Admission::New(id) => id,
            _ => panic!("expected new fixture"),
        })
    }
    fn acquired(journal: &mut Journal, id: Uuid) -> Result<()> {
        journal.prepare(id, b"original-swap")?;
        journal.start_acquisition(id)?;
        journal.record_acquired(id, 7, 1, b"verified-original-outputs")
    }
    #[tokio::test]
    async fn staging_requires_acquisition_and_does_not_repeat_or_touch_credit() -> Result<()> {
        let dir = crate::test_directory()?;
        let epoch = Uuid::new_v4();
        let path = dir.path().join("journal.sqlite");
        let mut journal = Journal::open(&path, epoch, 8)?;
        let ordinary = admit(&mut journal, Transport::Cashu, "ordinary-stage")?;
        acquired(&mut journal, ordinary)?;
        let nutzap = admit(&mut journal, Transport::Nutzap, "nutzap-stage")?;
        assert!(journal.stage_next_redemption(&owner(), 100)?.is_none());
        acquired(&mut journal, nutzap)?;
        assert_eq!(journal.stage_next_redemption(&owner(), 101)?, Some(nutzap));
        let intent = journal.publication_intent(&format!("redemption:{nutzap}"))?;
        assert_eq!(journal.state(nutzap)?, "acquired");
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert!(journal.stage_next_redemption(&owner(), 999)?.is_none());
        assert!(journal.publication_intent(&format!("redemption:{nutzap}"))? == intent);
        assert_eq!(journal.export_receipt(nutzap)?.net_sats, 7);
        assert_eq!(journal.state(ordinary)?, "acquired");
        Ok(())
    }
    #[tokio::test]
    async fn signing_attempts_survive_restart_exhaustion_and_transaction_failure() -> Result<()> {
        let dir = crate::test_directory()?;
        let epoch = Uuid::new_v4();
        let path = dir.path().join("journal.sqlite");
        let mut journal = Journal::open(&path, epoch, 8)?;
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        for id in [first, second] {
            journal.prepare_announcement(id, &announcement())?;
        }
        assert!(
            journal
                .claim_publication_signing(100, 1, false, true)?
                .is_none()
        );
        journal.connection.execute_batch("CREATE TRIGGER fail_signing_claim BEFORE INSERT ON publication_signing_schedule BEGIN SELECT RAISE(ABORT,'fixture'); END;")?;
        assert!(
            journal
                .claim_publication_signing(100, 1, true, true)
                .is_err()
        );
        journal
            .connection
            .execute_batch("DROP TRIGGER fail_signing_claim;")?;
        assert_eq!(
            journal.claim_publication_signing(100, 1, true, false)?,
            Some(format!("announcement:{first}"))
        );
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert_eq!(
            journal.claim_publication_signing(101, 1, true, false)?,
            Some(format!("announcement:{second}"))
        );
        assert!(
            journal
                .claim_publication_signing(9999, 1, true, true)?
                .is_none()
        );
        // Explicitly raising the limit resumes the same operation, not a new event.
        assert_eq!(
            journal.claim_publication_signing(9999, 2, true, false)?,
            Some(format!("announcement:{first}"))
        );
        let attempts: u32 = journal.connection.query_row(
            "SELECT attempts FROM publication_signing_schedule WHERE publication_id=?",
            [format!("announcement:{first}")],
            |r| r.get(0),
        )?;
        assert_eq!(attempts, 2);
        assert!(
            journal
                .claim_publication_signing(i64::MAX, 2, true, false)
                .is_err()
        );
        assert!(
            journal
                .claim_publication_signing(-1, 2, true, false)
                .is_err()
        );
        Ok(())
    }
    #[tokio::test]
    async fn redemption_requires_acquisition_and_freezes_encryption_and_exact_signed_bytes()
    -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("journal.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let id = admit(&mut journal, Transport::Nutzap, "history")?;
        assert!(journal.prepare_redemption(id, &owner(), 100).is_err());
        acquired(&mut journal, id)?;
        let first = journal.prepare_redemption(id, &owner(), 100)?;
        assert_eq!(journal.state(id)?, "acquired");
        assert!(first == journal.prepare_redemption(id, &owner(), 999)?);
        let operation = format!("redemption:{id}");
        assert!(journal.publication_unsigned(&operation).is_err());
        let ciphertext = envelope(9);
        journal.freeze_history_ciphertext(&operation, &ciphertext)?;
        let signed = sign(journal.publication_unsigned(&operation)?)?;
        journal.freeze_signed_publication(&operation, &signed)?;
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert_eq!(
            journal.signed_publication(&operation)?,
            Some(signed.clone())
        );
        journal.freeze_history_ciphertext(&operation, &ciphertext)?;
        journal.freeze_signed_publication(&operation, &signed)?;
        assert!(
            journal
                .freeze_history_ciphertext(&operation, &envelope(10))
                .is_err()
        );
        let mut changed = journal.publication_unsigned(&operation)?;
        changed.created_at += 1;
        assert!(
            journal
                .freeze_signed_publication(&operation, &sign(changed)?)
                .is_err()
        );
        assert_eq!(journal.signed_publication(&operation)?, Some(signed));
        let ordinary = admit(&mut journal, Transport::Cashu, "ordinary")?;
        acquired(&mut journal, ordinary)?;
        assert!(journal.prepare_redemption(ordinary, &owner(), 100).is_err());
        // Credit export is independent of best-effort history publication.
        let receipt = journal.export_receipt(id)?;
        assert_eq!(receipt.net_sats, 7);
        assert!(first == journal.prepare_redemption(id, &owner(), 1000)?);
        assert_eq!(journal.state(id)?, "credit_export_pending");
        Ok(())
    }
    #[tokio::test]
    async fn publication_backpressure_and_conflicts_do_not_mutate_acquisitions() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let first = Uuid::new_v4();
        let intent = announcement();
        journal.prepare_announcement(first, &intent)?;
        for _ in 1..1024 {
            journal.prepare_announcement(Uuid::new_v4(), &intent)?;
        }
        assert!(
            journal
                .prepare_announcement(Uuid::new_v4(), &intent)
                .is_err()
        );
        journal.prepare_announcement(first, &intent)?;
        let mut changed = intent.clone();
        if let Intent::Announcement { created_at, .. } = &mut changed {
            *created_at += 1;
        }
        assert!(journal.prepare_announcement(first, &changed).is_err());
        let id = admit(&mut journal, Transport::Nutzap, "backlog")?;
        acquired(&mut journal, id)?;
        assert!(journal.prepare_redemption(id, &owner(), 100).is_err());
        assert!(journal.stage_next_redemption(&owner(), 100)?.is_none());
        assert_eq!(journal.state(id)?, "acquired");
        assert_eq!(journal.export_receipt(id)?.net_sats, 7);
        Ok(())
    }
}

#[cfg(test)]
mod broker_response_tests {
    use super::*;
    use crate::publication::tests::{announcement, sign};
    #[tokio::test]
    async fn response_identity_and_transaction_failure_preserve_original_outbox() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let id = Uuid::new_v4();
        let operation = format!("announcement:{id}");
        journal.prepare_announcement(id, &announcement())?;
        assert!(journal.publication_signing_request(&operation).is_err());
        journal.runtime_registration(Uuid::new_v4(), &[7; 32], true)?;
        let request = journal.publication_signing_request(&operation)?;
        let mut result = crate::publication_broker::Signed {
            operation_id: Uuid::new_v4(),
            ciphertext: None,
            event_json: sign(request.intent.unsigned(None)?)?,
        };
        assert!(
            journal
                .accept_signed_publication(&operation, &result)
                .is_err()
        );
        result.operation_id = id;
        journal.connection.execute_batch("CREATE TRIGGER fail_publication BEFORE UPDATE ON nostr_publications BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")?;
        assert!(
            journal
                .accept_signed_publication(&operation, &result)
                .is_err()
        );
        assert!(journal.signed_publication(&operation)?.is_none());
        journal
            .connection
            .execute_batch("DROP TRIGGER fail_publication;")?;
        journal.accept_signed_publication(&operation, &result)?;
        journal.accept_signed_publication(&operation, &result)?;
        assert_eq!(
            journal.signed_publication(&operation)?,
            Some(result.event_json)
        );
        Ok(())
    }
}
