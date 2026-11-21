"""Private VPS journal for NUT-20 claims and exact-output recovery."""

import json
import sqlite3
from contextlib import contextmanager
from pathlib import Path
from uuid import UUID


def encode(value):
    return json.dumps(value, separators=(",", ":"), sort_keys=True)


class ClaimStore:
    """The caller owns a private state directory and a connection per worker."""

    def __init__(self, directory):
        directory = Path(directory)
        if directory.stat().st_mode & 0o077:
            raise PermissionError("claim directory must be private")
        self.db = sqlite3.connect(directory / "project-claims.sqlite", isolation_level=None)
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.execute("""
            CREATE TABLE IF NOT EXISTS project_claim (
                id TEXT PRIMARY KEY,
                terms TEXT NOT NULL,
                material TEXT NOT NULL,
                invoice TEXT UNIQUE,
                payment_hash TEXT UNIQUE,
                proofs TEXT,
                CHECK ((invoice IS NULL) = (payment_hash IS NULL)),
                CHECK (proofs IS NULL OR invoice IS NOT NULL)
            )
        """)

        self.db.execute("""
            CREATE TABLE IF NOT EXISTS project_claim_poll (
                id TEXT PRIMARY KEY REFERENCES project_claim(id),
                next_attempt INTEGER NOT NULL CHECK(next_attempt >= 0)
            )
        """)

    def reserve_poll(self, now, lease_seconds):
        """Claim one due invoice. An interrupted attempt becomes eligible after its lease."""
        if now < 0 or lease_seconds <= 0:
            raise ValueError("invalid claim poll lease")
        with self.write():
            row = self.db.execute("""
                SELECT c.id FROM project_claim c LEFT JOIN project_claim_poll p ON p.id=c.id
                WHERE c.invoice IS NOT NULL AND c.proofs IS NULL
                  AND COALESCE(p.next_attempt,0)<=?
                ORDER BY COALESCE(p.next_attempt,0), c.rowid LIMIT 1
            """, (now,)).fetchone()
            if row is None:
                return None
            self.db.execute("""
                INSERT INTO project_claim_poll(id,next_attempt) VALUES (?,?)
                ON CONFLICT(id) DO UPDATE SET next_attempt=excluded.next_attempt
            """, (row[0], now + lease_seconds))
            return row[0]

    def schedule_unpaid(self, request_id, now, expiry):
        """Keep checking expired invoices for a late authoritative settlement."""
        if now < expiry:
            return
        interval = 300 if now - expiry < 86400 else 3600
        self.db.execute("""
            INSERT INTO project_claim_poll(id,next_attempt) VALUES (?,?)
            ON CONFLICT(id) DO UPDATE SET next_attempt=MAX(next_attempt,excluded.next_attempt)
        """, (request_id, now + interval))

    @contextmanager
    def write(self):
        self.db.execute("BEGIN IMMEDIATE")
        try:
            yield
            self.db.commit()
        except BaseException:
            self.db.rollback()
            raise

    def prepare(self, request_id, terms, generate_material):
        """Freeze ownership key, secrets and blinds before the first HTTPS request."""
        request_id = str(UUID(request_id))
        if not UUID(request_id).int:
            raise ValueError("nil request identity")
        binding = encode(terms)
        with self.write():
            row = self.db.execute(
                "SELECT terms, material FROM project_claim WHERE id=?", (request_id,)
            ).fetchone()
            if row:
                if row[0] != binding:
                    raise ValueError("project invoice intent changed")
                return json.loads(row[1])
            material = generate_material()
            self.db.execute(
                "INSERT INTO project_claim(id,terms,material) VALUES (?,?,?)",
                (request_id, binding, encode(material)),
            )
            return material

    def record_invoice(self, request_id, invoice, payment_hash):
        """Record a caller-verified BOLT11 before exposing it to the payer."""
        with self.write():
            changed = self.db.execute("""
                UPDATE project_claim SET invoice=?,payment_hash=? WHERE id=?
                AND (invoice IS NULL OR (invoice=? AND payment_hash=?))
            """, (invoice, payment_hash, request_id, invoice, payment_hash)).rowcount
            if changed != 1:
                raise ValueError("unknown or conflicting project invoice")

    def record_proofs(self, request_id, payment_hash, proofs):
        """Persist caller-verified, unblinded outputs before delivery to accounting."""
        if not proofs:
            raise ValueError("empty project claim")
        payload = encode(proofs)
        with self.write():
            changed = self.db.execute("""
                UPDATE project_claim SET proofs=? WHERE id=? AND payment_hash=?
                AND (proofs IS NULL OR proofs=?)
            """, (payload, request_id, payment_hash, payload)).rowcount
            if changed != 1:
                raise ValueError("unknown or conflicting project claim")
