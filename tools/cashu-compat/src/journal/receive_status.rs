//! Durable public-view capabilities; no proof acquisition or core credit mutation.
use super::Journal;
use anyhow::{Result, bail, ensure};
use lightning_goats_cashu_receipt::receive_status::{Capability, Status};
use rusqlite::OptionalExtension;
use uuid::Uuid;

impl Journal {
    pub(crate) fn existing_http_delivery(&self, source_ref: &str) -> Result<Option<Uuid>> {
        self.connection
            .query_row(
                "SELECT acquisition_id FROM intake_sources WHERE transport='cashu' AND source_ref=?",
                [source_ref],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|id| Uuid::parse_str(&id).map_err(Into::into))
            .transpose()
    }

    /// Called only after validated ordinary admission. A lost reply/retry returns
    /// the original capability even after acquisition and credit consumption.
    pub fn receipt_capability(&mut self, id: Uuid) -> Result<Capability> {
        ensure!(!id.is_nil(), "invalid receipt identity");
        let tx = self.connection.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT capability FROM receive_status_capabilities WHERE acquisition_id=?",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(value) = existing {
            let cap = Capability::parse(value)?;
            tx.commit()?;
            return Ok(cap);
        }
        let cap = Capability::fresh();
        let written=tx.execute("INSERT INTO receive_status_capabilities(acquisition_id,capability) SELECT id,? FROM acquisitions WHERE id=?",[cap.as_str(),&id.to_string()])?;
        ensure!(written == 1, "receipt is not admitted");
        tx.commit()?;
        Ok(cap)
    }
    /// Read-only and independent of whether new receiving is paused. The private
    /// listener must authenticate registration/epoch before calling this method.
    pub fn receipt_status(&self, capability: &Capability) -> Result<Option<Status>> {
        capability.validate()?;
        let value:Option<(String,Option<i64>,bool,bool)>=self.connection.query_row(
            "SELECT a.state,a.net,a.acquired_evidence IS NOT NULL,a.core_ack IS NOT NULL FROM receive_status_capabilities c JOIN acquisitions a ON a.id=c.acquisition_id WHERE c.capability=?",
            [capability.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?;
        let Some((state, net, evidence, ack)) = value else {
            return Ok(None);
        };
        let status = match state.as_str() {
            "validated" | "acquisition_prepared" | "acquiring" => Status::Pending,
            "held_for_reconciliation" => Status::Held,
            "acquired" | "credit_export_pending" => {
                ensure!(
                    net.is_some_and(|sats| sats > 0) && evidence,
                    "receipt ownership evidence missing"
                );
                Status::Acquired
            }
            "credited" => {
                ensure!(
                    net.is_some_and(|sats| sats > 0) && evidence && ack,
                    "receipt credit evidence missing"
                );
                Status::Credited
            }
            _ => bail!("invalid receipt status state"),
        };
        Ok(Some(status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_capabilities_reopen_and_status_requires_ownership_and_ack() -> Result<()> {
        let directory = crate::test_directory()?;
        let path = directory.path().join("journal.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        for acquisition in [id, other] {
            journal.connection.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross) VALUES(?,'fixture','v1','cashu',?,1,50)",[acquisition.to_string(),format!("synthetic:{acquisition}")])?;
        }
        assert!(journal.receipt_capability(Uuid::new_v4()).is_err());
        let cap = journal.receipt_capability(id)?;
        assert!(journal.receipt_capability(id)? == cap);
        assert!(journal.receipt_capability(other)? != cap);
        assert_eq!(journal.receipt_status(&Capability::fresh())?, None);
        assert!(
            journal
                .connection
                .execute(
                    "UPDATE receive_status_capabilities SET capability=? WHERE acquisition_id=?",
                    [Capability::fresh().as_str(), &id.to_string()]
                )
                .is_err()
        );
        assert!(
            journal
                .connection
                .execute(
                    "DELETE FROM receive_status_capabilities WHERE acquisition_id=?",
                    [id.to_string()]
                )
                .is_err()
        );
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert!(journal.receipt_capability(id)? == cap);
        assert_eq!(journal.receipt_status(&cap)?, Some(Status::Pending));
        journal.connection.execute(
            "UPDATE acquisitions SET state='acquired' WHERE id=?",
            [id.to_string()],
        )?;
        assert!(journal.receipt_status(&cap).is_err());
        journal.connection.execute(
            "UPDATE acquisitions SET net=50,receive_fee=0,acquired_evidence=X'01' WHERE id=?",
            [id.to_string()],
        )?;
        assert_eq!(journal.receipt_status(&cap)?, Some(Status::Acquired));
        journal.connection.execute(
            "UPDATE acquisitions SET state='credited' WHERE id=?",
            [id.to_string()],
        )?;
        assert!(journal.receipt_status(&cap).is_err());
        journal.connection.execute(
            "UPDATE acquisitions SET core_ack=X'02' WHERE id=?",
            [id.to_string()],
        )?;
        assert_eq!(journal.receipt_status(&cap)?, Some(Status::Credited));
        journal.connection.execute(
            "UPDATE acquisitions SET state='held_for_reconciliation' WHERE id=?",
            [id.to_string()],
        )?;
        assert_eq!(journal.receipt_status(&cap)?, Some(Status::Held));
        let count: i64 = journal.connection.query_row(
            "SELECT COUNT(*) FROM receive_status_capabilities",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    }
}
