use std::path::Path;

use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

use crate::{Create, Invoice, Pay};

pub enum Admission<T> {
    New,
    Known(T),
    Held,
}

#[derive(Debug)]
pub enum Failure {
    Conflict,
    Store,
}

impl From<rusqlite::Error> for Failure {
    fn from(_: rusqlite::Error) -> Self {
        Self::Store
    }
}

impl From<serde_json::Error> for Failure {
    fn from(_: serde_json::Error) -> Self {
        Self::Store
    }
}

type Stored = (String, Option<String>);

pub struct Journal(Connection);

impl Journal {
    pub fn open(state: &Path, network: &str, seed_fingerprint: &str) -> Result<Self> {
        let path = state.join("reserve.sqlite3");
        ensure!(
            path.exists() || !state.join("spark").exists(),
            "missing operations journal for existing SDK storage"
        );
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS identity (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                network TEXT NOT NULL, seed_fingerprint TEXT NOT NULL, wallet TEXT
            );
            CREATE TABLE IF NOT EXISTS received (
                id TEXT PRIMARY KEY, intent TEXT NOT NULL, payment_hash TEXT UNIQUE,
                invoice TEXT, settlement TEXT, transfer_id TEXT UNIQUE
            );
            CREATE TABLE IF NOT EXISTS sent (
                id TEXT PRIMARY KEY, payment_hash TEXT UNIQUE NOT NULL,
                intent TEXT NOT NULL, settlement TEXT
            );",
        )?;
        db.execute("INSERT OR IGNORE INTO identity (singleton, network, seed_fingerprint) VALUES (1, ?1, ?2)", params![network, seed_fingerprint])?;
        let matches: bool = db.query_row(
            "SELECT network=?1 AND seed_fingerprint=?2 FROM identity",
            params![network, seed_fingerprint],
            |row| row.get(0),
        )?;
        ensure!(
            matches,
            "network or reserve credential differs from the journal"
        );
        Ok(Self(db))
    }

    pub fn bind_wallet(&self, wallet: &str) -> Result<()> {
        ensure!(
            self.0.execute(
                "UPDATE identity SET wallet=?1 WHERE singleton=1 AND (wallet IS NULL OR wallet=?1)",
                [wallet]
            )? == 1,
            "wallet identity differs from the journal"
        );
        Ok(())
    }

    pub fn admit_receive(&mut self, request: &Create) -> Result<Admission<Invoice>, Failure> {
        let intent = serde_json::to_string(request)?;
        let id = request.operation_id.to_string();
        let existing: Option<Stored> = self
            .0
            .query_row(
                "SELECT intent, invoice FROM received WHERE id=?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((original, invoice)) = existing {
            if original != intent {
                return Err(Failure::Conflict);
            }
            return Ok(match invoice {
                Some(invoice) => Admission::Known(serde_json::from_str(&invoice)?),
                None => Admission::Held,
            });
        }
        self.0.execute(
            "INSERT INTO received (id, intent) VALUES (?1, ?2)",
            params![id, intent],
        )?;
        Ok(Admission::New)
    }

    pub fn record_receive(&mut self, id: Uuid, invoice: &Invoice) -> Result<(), Failure> {
        let updated = self.0.execute(
            "UPDATE received SET payment_hash=?1, invoice=?2 WHERE id=?3 AND invoice IS NULL
            AND NOT EXISTS (SELECT 1 FROM sent WHERE payment_hash=?1)",
            params![
                invoice.payment_hash,
                serde_json::to_string(invoice)?,
                id.to_string()
            ],
        )?;
        if updated != 1 {
            return Err(Failure::Store);
        }
        Ok(())
    }

    pub fn receive(&mut self, hash: &str) -> Result<Option<(Invoice, Option<Value>)>, Failure> {
        let stored: Option<Stored> = self
            .0
            .query_row(
                "SELECT invoice, settlement FROM received WHERE payment_hash=?1",
                [hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        decode(stored)
    }

    pub fn record_receive_settlement(
        &mut self,
        hash: &str,
        settlement: &Value,
    ) -> Result<(), Failure> {
        let transfer = settlement["transfer_id"].as_str().ok_or(Failure::Store)?;
        let data = serde_json::to_string(settlement)?;
        let updated = self.0.execute("UPDATE received SET settlement=?1, transfer_id=?2 WHERE payment_hash=?3 AND (settlement IS NULL OR settlement=?1)", params![data, transfer, hash])?;
        if updated != 1 {
            return Err(Failure::Store);
        }
        Ok(())
    }

    pub fn send(
        &mut self,
        request: &Pay,
        hash: &str,
        insert: bool,
    ) -> Result<Admission<Value>, Failure> {
        let intent = serde_json::to_string(request)?;
        let id = request.operation_id.to_string();
        if let Some((original, settlement)) = self
            .0
            .query_row(
                "SELECT intent, settlement FROM sent WHERE id=?1",
                [&id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?
        {
            if original != intent {
                return Err(Failure::Conflict);
            }
            return Ok(match settlement {
                Some(result) => Admission::Known(serde_json::from_str(&result)?),
                None => Admission::Held,
            });
        }
        let hash_exists: bool = self.0.query_row("SELECT EXISTS (SELECT 1 FROM sent WHERE payment_hash=?1) OR EXISTS (SELECT 1 FROM received WHERE payment_hash=?1)", [hash], |row| row.get(0))?;
        if hash_exists {
            return Err(Failure::Conflict);
        }
        if insert {
            self.0.execute(
                "INSERT INTO sent (id, intent, payment_hash) VALUES (?1, ?2, ?3)",
                params![id, intent, hash],
            )?;
        }
        Ok(Admission::New)
    }

    pub fn lookup_send(&mut self, id: Uuid) -> Result<Option<(Pay, Option<Value>)>, Failure> {
        let stored = self
            .0
            .query_row(
                "SELECT intent, settlement FROM sent WHERE id=?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        decode(stored)
    }

    pub fn record_send(&mut self, id: Uuid, settlement: &Value) -> Result<(), Failure> {
        let data = serde_json::to_string(settlement)?;
        let updated = self.0.execute(
            "UPDATE sent SET settlement=?1 WHERE id=?2 AND (settlement IS NULL OR settlement=?1)",
            params![data, id.to_string()],
        )?;
        if updated != 1 {
            return Err(Failure::Store);
        }
        Ok(())
    }
}

fn decode<T: serde::de::DeserializeOwned>(
    stored: Option<Stored>,
) -> Result<Option<(T, Option<Value>)>, Failure> {
    stored
        .map(|(intent, settlement)| {
            Ok((
                serde_json::from_str(&intent)?,
                settlement
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()?,
            ))
        })
        .transpose()
}
