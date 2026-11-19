//! Discovery checkpoints only. A completed relay range is never payment evidence.
use super::*;

#[derive(Clone, Copy)]
pub enum Lane {
    /// Rolling samples keep recent traffic independent of blocked historical ranges.
    Recent,
    /// Repeated historical passes discover delayed events without trusting timestamps.
    History,
}
impl Lane {
    fn name(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::History => "history",
        }
    }
}

pub struct Registration {
    /// Digest of the receiving key, recipient and complete admission policy inventory.
    /// A policy change must use a new scope; old acquisitions remain independently durable.
    pub scope: [u8; 32],
    pub relay: String,
    pub floor: i64,
    pub span: i64,
    pub overlap: i64,
}
impl Registration {
    fn validate(&self) -> Result<()> {
        crate::relay::validate_relay(&self.relay)?;
        ensure!(
            self.scope != [0; 32]
                && self.floor >= 0
                && (1..=86_400).contains(&self.span)
                && self.overlap >= 0
                && self.overlap < self.span,
            "invalid relay scan registration"
        );
        Ok(())
    }
}

pub struct Claim {
    scope: [u8; 32],
    relay: String,
    lane: Lane,
    id: Uuid,
    pub since: i64,
    pub until: i64,
}
#[derive(Clone, Copy)]
pub enum Outcome {
    /// Use only after EOSE, below all response caps, and after durable disposition
    /// of every returned event. Transient admission/storage failure is incomplete.
    AdmittedComplete,
    Incomplete,
    Saturated,
}

impl Journal {
    /// Persist a lease before network I/O. Recent and historical work have separate
    /// leases. No registration or retry operation releases admitted proofs.
    pub fn claim_relay_scan(
        &mut self,
        registration: &Registration,
        lane: Lane,
        now: i64,
    ) -> Result<Option<Claim>> {
        registration.validate()?;
        ensure!(now >= registration.floor, "relay clock precedes scan floor");
        // Reader deadline is 20 seconds; a crashed claim remains fenced for 30.
        let next = now
            .checked_add(30)
            .ok_or_else(|| anyhow::anyhow!("relay clock overflow"))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key = params![
            registration.scope.as_slice(),
            registration.relay,
            lane.name()
        ];
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM relay_scans WHERE scope=? AND relay=? AND lane=?)",
            key,
            |row| row.get(0),
        )?;
        if !existing {
            let count: i64 =
                tx.query_row("SELECT COUNT(*) FROM relay_scans", [], |row| row.get(0))?;
            ensure!(count < 128, "relay scan registration capacity exhausted");
            tx.execute("INSERT INTO relay_scans(scope,relay,lane,floor,span,overlap,cursor,horizon,width) VALUES(?,?,?,?,?,?,?,?,?)",
                params![registration.scope.as_slice(),registration.relay,lane.name(),registration.floor,registration.span,registration.overlap,registration.floor,now,registration.span])?;
        }
        let (floor,span,overlap,cursor,horizon,width,due): (i64,i64,i64,i64,i64,i64,i64) = tx.query_row(
            "SELECT floor,span,overlap,cursor,horizon,width,next_attempt FROM relay_scans WHERE scope=? AND relay=? AND lane=?",
            key, |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)),
        )?;
        ensure!(
            (floor, span, overlap) == (registration.floor, registration.span, registration.overlap),
            "relay scan registration changed without new scope"
        );
        if due > now {
            return Ok(None);
        }
        let (since, until) = match lane {
            Lane::Recent => (floor.max(now.saturating_sub(span - 1)), now),
            Lane::History => {
                // A clock rollback must not manufacture future coverage.
                if now < horizon {
                    return Ok(None);
                }
                (cursor, cursor.saturating_add(width - 1).min(horizon))
            }
        };
        let id = Uuid::new_v4();
        tx.execute("UPDATE relay_scans SET claim=?,scan_since=?,scan_until=?,next_attempt=?,outcome='pending' WHERE scope=? AND relay=? AND lane=?",
            params![id.to_string(),since,until,next,registration.scope.as_slice(),registration.relay,lane.name()])?;
        tx.commit()?;
        Ok(Some(Claim {
            scope: registration.scope,
            relay: registration.relay.clone(),
            lane,
            id,
            since,
            until,
        }))
    }

    /// Caller commits all admissions before this checkpoint. A crash between those
    /// commits and this one replays the same proofs through the shared Y index.
    pub fn finish_relay_scan(&mut self, claim: &Claim, outcome: Outcome, now: i64) -> Result<()> {
        self.finish_relay_scan_with_counts(claim, outcome, now, 0, 0)
    }

    pub fn finish_relay_scan_with_counts(
        &mut self,
        claim: &Claim,
        outcome: Outcome,
        now: i64,
        admitted: u16,
        rejected: u16,
    ) -> Result<()> {
        ensure!(
            u32::from(admitted) + u32::from(rejected) <= 256,
            "relay disposition count exceeds batch bound"
        );
        ensure!(now >= claim.until, "relay completion clock precedes scan");
        let next = now
            .checked_add(5)
            .ok_or_else(|| anyhow::anyhow!("relay clock overflow"))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(i64,i64,i64,i64,i64,i64)> = tx.query_row(
            "SELECT floor,span,overlap,horizon,width,next_attempt FROM relay_scans WHERE scope=? AND relay=? AND lane=? AND claim=? AND scan_since=? AND scan_until=?",
            params![claim.scope.as_slice(),claim.relay,claim.lane.name(),claim.id.to_string(),claim.since,claim.until],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
        ).optional()?;
        let (floor, span, overlap, mut horizon, mut width, leased_until) =
            row.ok_or_else(|| anyhow::anyhow!("stale relay scan claim"))?;
        ensure!(
            now >= leased_until - 30,
            "relay completion clock moved backward"
        );
        let mut cursor = claim.since;
        if matches!(claim.lane, Lane::Recent) {
            horizon = claim.until;
        }
        let result = match outcome {
            Outcome::AdmittedComplete => {
                if matches!(claim.lane, Lane::History) {
                    if claim.until == horizon {
                        // Revisit the bounded historical inventory on a new pass.
                        cursor = floor;
                        horizon = now;
                    } else {
                        let covered_width = claim.until - claim.since + 1;
                        cursor = claim.until + 1 - overlap.min(covered_width - 1);
                    }
                    width = span;
                }
                "complete"
            }
            Outcome::Incomplete => "incomplete",
            Outcome::Saturated => {
                // At one second there is no safe NIP-01 pagination. Retain that
                // unresolved range; the recent lane still runs independently.
                if matches!(claim.lane, Lane::History) {
                    width = ((claim.until - claim.since + 1) / 2).max(1);
                }
                "saturated"
            }
        };
        tx.execute("UPDATE relay_scans SET cursor=?,horizon=?,width=?,claim=NULL,next_attempt=?,outcome=? WHERE scope=? AND relay=? AND lane=?",
            params![cursor,horizon,width,next,result,claim.scope.as_slice(),claim.relay,claim.lane.name()])?;
        tx.execute("INSERT INTO relay_scan_results(scope,relay,lane,admitted,rejected) VALUES(?,?,?,?,?) ON CONFLICT(scope,relay,lane) DO UPDATE SET admitted=excluded.admitted,rejected=excluded.rejected",
            params![claim.scope.as_slice(),claim.relay,claim.lane.name(),admitted,rejected])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registration() -> Registration {
        Registration {
            scope: [1; 32],
            relay: "wss://relay.example.com".into(),
            floor: 100,
            span: 10,
            overlap: 2,
        }
    }
    #[tokio::test]
    async fn incomplete_restart_stale_claim_and_overlap() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("journal.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let r = registration();
        let first = journal.claim_relay_scan(&r, Lane::History, 200)?.unwrap();
        assert_eq!((first.since, first.until), (100, 109));
        assert!(
            journal
                .finish_relay_scan(&first, Outcome::AdmittedComplete, 150)
                .is_err()
        );
        assert!(
            journal
                .finish_relay_scan(&first, Outcome::AdmittedComplete, i64::MAX)
                .is_err()
        );
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert!(journal.claim_relay_scan(&r, Lane::History, 229)?.is_none());
        let retry = journal.claim_relay_scan(&r, Lane::History, 230)?.unwrap();
        assert_eq!((retry.since, retry.until), (100, 109));
        assert!(
            journal
                .finish_relay_scan(&first, Outcome::AdmittedComplete, 230)
                .is_err()
        );
        journal.finish_relay_scan(&retry, Outcome::Incomplete, 230)?;
        let retry = journal.claim_relay_scan(&r, Lane::History, 235)?.unwrap();
        assert_eq!((retry.since, retry.until), (100, 109));
        journal.finish_relay_scan(&retry, Outcome::AdmittedComplete, 235)?;
        assert!(
            journal
                .finish_relay_scan(&retry, Outcome::AdmittedComplete, 235)
                .is_err()
        );
        let next = journal.claim_relay_scan(&r, Lane::History, 240)?.unwrap();
        assert_eq!((next.since, next.until), (108, 117));
        Ok(())
    }
    #[tokio::test]
    async fn saturation_retains_history_but_recent_lane_and_other_relay_run() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let r = registration();
        for (now, end) in [(200, 109), (205, 104), (210, 101), (215, 100), (220, 100)] {
            let claim = journal.claim_relay_scan(&r, Lane::History, now)?.unwrap();
            assert_eq!((claim.since, claim.until), (100, end));
            journal.finish_relay_scan(&claim, Outcome::Saturated, now)?;
        }
        let recent = journal.claim_relay_scan(&r, Lane::Recent, 220)?.unwrap();
        assert_eq!((recent.since, recent.until), (211, 220));
        journal.finish_relay_scan(&recent, Outcome::Saturated, 220)?;
        let recent = journal.claim_relay_scan(&r, Lane::Recent, 225)?.unwrap();
        assert_eq!((recent.since, recent.until), (216, 225));
        journal.finish_relay_scan(&recent, Outcome::Incomplete, 225)?;
        let window: (i64, i64) = journal.connection.query_row(
            "SELECT cursor,horizon FROM relay_scans WHERE lane='recent'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(window, (216, 225));
        let mut other = registration();
        other.relay = "wss://other.example.com".into();
        assert!(
            journal
                .claim_relay_scan(&other, Lane::History, 220)?
                .is_some()
        );
        let remaining: i64 = journal.connection.query_row(
            "SELECT cursor FROM relay_scans WHERE lane='history' AND relay=?",
            [r.relay],
            |r| r.get(0),
        )?;
        assert_eq!(remaining, 100);
        Ok(())
    }
    #[tokio::test]
    async fn passes_revisit_old_events_and_registration_is_immutable_bounded() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let mut r = registration();
        let claim = journal.claim_relay_scan(&r, Lane::History, 103)?.unwrap();
        assert_eq!((claim.since, claim.until), (100, 103));
        journal.finish_relay_scan(&claim, Outcome::AdmittedComplete, 105)?;
        let next = journal.claim_relay_scan(&r, Lane::History, 110)?.unwrap();
        assert_eq!((next.since, next.until), (100, 105));
        r.overlap = 3;
        assert!(journal.claim_relay_scan(&r, Lane::History, 200).is_err());
        r.scope = [2; 32];
        assert!(journal.claim_relay_scan(&r, Lane::History, 200)?.is_some());
        assert!(
            journal
                .claim_relay_scan(&r, Lane::Recent, i64::MAX)
                .is_err()
        );
        for i in 3..=128 {
            r.scope = [i; 32];
            assert!(journal.claim_relay_scan(&r, Lane::History, 200)?.is_some());
        }
        r.scope = [129; 32];
        assert!(journal.claim_relay_scan(&r, Lane::History, 200).is_err());
        Ok(())
    }
}
