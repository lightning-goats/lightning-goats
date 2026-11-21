"""Private VPS journal for NUT-20 claims and exact-output recovery."""

import hashlib
import json
import sqlite3
import time
from contextlib import contextmanager
from pathlib import Path
from uuid import UUID


def encode(value):
    return json.dumps(value, separators=(",", ":"), sort_keys=True)


class SplitRequired(ValueError):
    def __init__(self, selected, splits):
        super().__init__("confirmed payout requires exact change")
        self.selected = selected
        self.splits = splits


class ClaimStore:
    """The caller owns a private state directory and a connection per worker."""

    def __init__(self, directory):
        directory = Path(directory)
        if directory.stat().st_mode & 0o077:
            raise PermissionError("claim directory must be private")
        self.db = sqlite3.connect(directory / "project-claims.sqlite", isolation_level=None)
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.execute("PRAGMA foreign_keys=ON")
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

        # Tokens remain in this private journal throughout their lifetime.  A
        # separately indexed output can fund at most one pending disposition.
        with self.write():
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS collected_output (
                    secret TEXT PRIMARY KEY NOT NULL,
                    request_id TEXT NOT NULL REFERENCES project_claim(id),
                    amount_sat INTEGER NOT NULL CHECK(amount_sat > 0),
                    proof TEXT NOT NULL
                )
            """)
            self.db.execute("""
                CREATE INDEX IF NOT EXISTS collected_output_request ON collected_output(request_id)
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS proof_disposition (
                    id TEXT PRIMARY KEY NOT NULL,
                    purpose TEXT NOT NULL CHECK(purpose IN ('cyberherd_payout','operator_transfer')),
                    binding TEXT NOT NULL
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS disposition_input (
                    secret TEXT PRIMARY KEY REFERENCES collected_output(secret),
                    disposition_id TEXT NOT NULL REFERENCES proof_disposition(id)
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS payout_invoice (
                    payment_hash TEXT PRIMARY KEY NOT NULL,
                    disposition_id TEXT UNIQUE NOT NULL REFERENCES proof_disposition(id)
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS payout_claim_allocation (
                    disposition_id TEXT NOT NULL REFERENCES proof_disposition(id),
                    feed_id TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES project_claim(id),
                    budget_sat INTEGER NOT NULL CHECK(budget_sat>0),
                    reserved_sat INTEGER NOT NULL CHECK(reserved_sat>0 AND reserved_sat<=budget_sat),
                    PRIMARY KEY(disposition_id,claim_id)
                )
            """)
            self.db.execute("""
                CREATE INDEX IF NOT EXISTS payout_claim_feed ON payout_claim_allocation(feed_id,claim_id)
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS payout_melt (
                    disposition_id TEXT PRIMARY KEY REFERENCES payout_invoice(disposition_id),
                    quote_id TEXT UNIQUE NOT NULL,
                    change_material TEXT NOT NULL
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS payout_melt_submission (
                    disposition_id TEXT PRIMARY KEY NOT NULL REFERENCES payout_melt(disposition_id)
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS payout_melt_result (
                    disposition_id TEXT PRIMARY KEY NOT NULL REFERENCES payout_melt_submission(disposition_id),
                    fee_sat INTEGER NOT NULL CHECK(fee_sat>=0),
                    preimage TEXT NOT NULL, change_proofs TEXT NOT NULL
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS payout_change_credit (
                    disposition_id TEXT PRIMARY KEY NOT NULL REFERENCES payout_melt_result(disposition_id),
                    returned_sat INTEGER NOT NULL CHECK(returned_sat>0)
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS proof_split (
                    secret TEXT PRIMARY KEY REFERENCES disposition_input(secret),
                    material TEXT NOT NULL,
                    complete INTEGER NOT NULL DEFAULT 0 CHECK(complete IN (0,1))
                )
            """)
            self.db.execute("""
                CREATE TABLE IF NOT EXISTS acquired_custody (
                    claim_id TEXT PRIMARY KEY REFERENCES project_claim(id),
                    evidence_digest TEXT UNIQUE NOT NULL,
                    proofs TEXT NOT NULL
                )
            """)
            for ident, proofs in self.db.execute("""
                SELECT id,proofs FROM project_claim c WHERE proofs IS NOT NULL AND NOT EXISTS
                    (SELECT 1 FROM collected_output o WHERE o.request_id=c.id)
            """):
                self._index_outputs(ident, json.loads(proofs))

    def _index_outputs(self, request_id, proofs):
        if len({p["secret"] for p in proofs}) != len(proofs):
            raise ValueError("duplicate project proof")
        for proof in proofs:
            secret, amount = proof["secret"], proof["amount"]
            if type(amount) is not int or amount <= 0:
                raise ValueError("invalid project proof amount")
            self.db.execute("""
                INSERT INTO collected_output(secret, request_id, amount_sat, proof) VALUES (?,?,?,?)
                ON CONFLICT(secret) DO NOTHING
            """, (secret, request_id, amount, encode(proof)))
            row = self.db.execute("""
                SELECT request_id,proof FROM collected_output WHERE secret=?
            """, (secret,)).fetchone()
            if row != (request_id, encode(proof)):
                raise ValueError("project proof already belongs to another claim")

    def reserve_inputs(self, disposition_id, purpose, intent, secrets):
        """Reserve inputs chosen by the authorized payout or transfer planner."""
        disposition_id = str(UUID(disposition_id))
        if purpose != "operator_transfer":
            raise ValueError("payout inputs require a confirmed feed allocation")
        if not secrets or len(secrets) != len(set(secrets)):
            raise ValueError("missing or duplicate disposition inputs")
        binding = encode(intent)
        with self.write():
            previous = self.db.execute(
                "SELECT purpose,binding FROM proof_disposition WHERE id=?", (disposition_id,)
            ).fetchone()
            if previous is None:
                self.db.execute("INSERT INTO proof_disposition VALUES (?,?,?)",
                                (disposition_id, purpose, binding))
                for secret in secrets:
                    changed = self.db.execute("""
                        INSERT INTO disposition_input(secret,disposition_id)
                        SELECT secret,? FROM collected_output WHERE secret=?
                    """, (disposition_id, secret)).rowcount
                    if changed != 1:
                        raise ValueError("unknown disposition input")
            elif previous != (purpose, binding):
                raise ValueError("disposition intent changed")
            rows = self.db.execute("""
                SELECT o.secret,o.proof FROM disposition_input i JOIN collected_output o USING(secret)
                WHERE disposition_id=? ORDER BY o.secret
            """, (disposition_id,)).fetchall()
            if [r[0] for r in rows] != sorted(secrets):
                raise ValueError("disposition inputs changed")
            return [json.loads(r[1]) for r in rows]

    def payout_inputs(self, feed_id, gross, allocations):
        """Select exact inputs using the same allocation bounds for quotes and reservations."""
        if type(gross) is not int or gross <= 0:
            raise ValueError("invalid payout input total")
        left = gross
        selected = []
        remaining = {}
        candidates = []
        for claim_id, terms in sorted(allocations.items()):
            digest, budget = terms["evidence_digest"], terms["consumed_sats"]
            if terms.get("source", "project_invoice") == "acquisition":
                row = self.db.execute("SELECT a.proofs,q.terms,a.evidence_digest FROM acquired_custody a "
                    "JOIN project_claim q ON q.id=a.claim_id WHERE a.claim_id=?", (claim_id,)).fetchone()
                verified_digest = row[2] if row else None
            else:
                row = self.db.execute("SELECT proofs,terms FROM project_claim WHERE id=?",
                                      (claim_id,)).fetchone()
                verified_digest = hashlib.sha256(row[0].encode()).hexdigest() if row and row[0] else None
            attribution = json.loads(row[1])["attribution"] if row else {}
            if (row is None or row[0] is None or verified_digest != digest
                    or (attribution.get("credit_pool") != "herd" and attribution.get("purpose") != "treasury_conversion")
                    or type(budget) is not int or budget <= 0
                    or budget > sum(p["amount"] for p in json.loads(row[0]))):
                raise ValueError("payout allocation conflicts with local custody")
            used, original = self.db.execute("""
                SELECT COALESCE(SUM(reserved_sat-COALESCE(returned_sat,0)),0),MIN(budget_sat)
                FROM payout_claim_allocation LEFT JOIN payout_change_credit USING(disposition_id)
                WHERE feed_id=? AND claim_id=?
            """, (feed_id, claim_id)).fetchone()
            if original is not None and original != budget:
                raise ValueError("confirmed feed allocation changed")
            remaining[claim_id] = budget - used
            candidates.extend((sats, claim_id, secret) for secret,sats in self.db.execute("""
                SELECT secret,amount_sat FROM collected_output o
                WHERE request_id=? AND (NOT EXISTS
                (SELECT 1 FROM disposition_input i WHERE i.secret=o.secret)
                OR EXISTS(SELECT 1 FROM proof_split s WHERE s.secret=o.secret AND s.complete=0))
                ORDER BY amount_sat DESC,secret
            """, (claim_id,)))
        taken = {}
        for sats, claim_id, secret in sorted(candidates, key=lambda x: (-x[0], x[1], x[2])):
            if sats <= min(remaining[claim_id], left):
                selected.append(secret)
                remaining[claim_id] -= sats
                taken[claim_id] = taken.get(claim_id, 0) + sats
                left -= sats
                if not left:
                    break
        pending = {secret for (secret,) in self.db.execute("SELECT secret FROM proof_split WHERE complete=0")}
        splits = [(secret, None) for secret in selected if secret in pending]
        if left:
            used_secrets = set(selected)
            for sats, claim_id, secret in sorted(candidates):
                if secret in used_secrets:
                    continue
                portion = min(left, remaining[claim_id], sats)
                if portion:
                    splits.append((secret, portion))
                    remaining[claim_id] -= portion
                    left -= portion
                if not left:
                    break
            if left:
                raise ValueError("confirmed allocation lacks available payout inputs")
        if splits:
            raise SplitRequired(selected, splits)
        return selected, taken

    def reserve_payout(self, disposition_id, feed_id, invoice_hash, amount, fee_limit, allocations, *, invoice_expiry=None, invoice_digest=None):
        """Reserve exact project inputs against an authenticated confirmed-feed allocation."""
        disposition_id = str(UUID(disposition_id))
        feed_id = str(UUID(feed_id))
        if (not isinstance(invoice_hash, str) or len(invoice_hash) != 64
                or any(c not in "0123456789abcdef" for c in invoice_hash)
                or type(amount) is not int or not 0 < amount <= 1_000_000
                or type(fee_limit) is not int or not 0 <= fee_limit <= 1_000_000
                or not allocations):
            raise ValueError("invalid payout terms")
        terms = {"feed": feed_id, "invoice_hash": invoice_hash, "amount_sat": amount,
                 "maximum_fee_sat": fee_limit, "claims": allocations}
        legacy_binding = encode(terms)
        if invoice_digest is not None:
            terms["invoice_digest"] = invoice_digest
        binding = encode(terms)
        with self.write():
            previous = self.db.execute("SELECT purpose,binding FROM proof_disposition WHERE id=?",
                                       (disposition_id,)).fetchone()
            if previous and previous not in (("cyberherd_payout", binding), ("cyberherd_payout", legacy_binding)):
                raise ValueError("payout replay differs")
            if previous is None:
                if invoice_expiry is not None and invoice_expiry <= time.time():
                    raise ValueError("expired payout invoice")
                self.db.execute("INSERT INTO proof_disposition VALUES (?,'cyberherd_payout',?)",
                                (disposition_id, binding))
                self.db.execute("INSERT INTO payout_invoice VALUES (?,?)", (invoice_hash, disposition_id))
                selected, taken = self.payout_inputs(feed_id, amount + fee_limit, allocations)
                self.db.executemany("INSERT INTO disposition_input VALUES (?,?)",
                                    ((secret, disposition_id) for secret in selected))
                for claim_id, sats in taken.items():
                    self.db.execute("INSERT INTO payout_claim_allocation VALUES (?,?,?,?,?)",
                                    (disposition_id, feed_id, claim_id, allocations[claim_id]["consumed_sats"], sats))
            return [json.loads(r[0]) for r in self.db.execute("""
                SELECT o.proof FROM collected_output o JOIN disposition_input i USING(secret)
                WHERE i.disposition_id=? ORDER BY o.secret
            """, (disposition_id,))]

    def prepare_melt(self, disposition_id, payment_hash, quote_id, generate_change):
        """Bind a verified mint quote and private change blinds before submitting inputs."""
        if not isinstance(quote_id, str) or not quote_id.strip():
            raise ValueError("missing melt quote")
        with self.write():
            invoice = self.db.execute(
                "SELECT payment_hash FROM payout_invoice WHERE disposition_id=?", (disposition_id,)
            ).fetchone()
            if invoice != (payment_hash,):
                raise ValueError("melt invoice differs from reserved payout")
            previous = self.db.execute(
                "SELECT quote_id,change_material FROM payout_melt WHERE disposition_id=?",
                (disposition_id,),
            ).fetchone()
            if previous:
                if previous[0] != quote_id:
                    raise ValueError("reserved payout already bound to a different melt quote")
                return json.loads(previous[1])
            material = generate_change()
            self.db.execute("INSERT INTO payout_melt VALUES (?,?,?)",
                            (disposition_id, quote_id, encode(material)))
            return material

    def begin_melt_submission(self, disposition_id, quote_id):
        """Only the first caller may submit. Others must reconcile the saved quote."""
        with self.write():
            bound = self.db.execute("SELECT quote_id FROM payout_melt WHERE disposition_id=?",
                                    (disposition_id,)).fetchone()
            if bound != (quote_id,):
                raise ValueError("unknown payout melt quote")
            return self.db.execute("""
                INSERT INTO payout_melt_submission(disposition_id) VALUES (?)
                ON CONFLICT(disposition_id) DO NOTHING
            """, (disposition_id,)).rowcount == 1

    def record_melt_result(self, disposition_id, fee, preimage, proofs):
        values = (fee, preimage, encode(proofs))
        with self.write():
            self.db.execute("""
                INSERT INTO payout_melt_result VALUES (?,?,?,?)
                ON CONFLICT(disposition_id) DO NOTHING
            """, (disposition_id, *values))
            saved = self.db.execute("""
                SELECT fee_sat,preimage,change_proofs FROM payout_melt_result WHERE disposition_id=?
            """, (disposition_id,)).fetchone()
            if saved != values:
                raise ValueError("settled payout result changed")
            # A single-source melt returns the fee change to its original claim.
            # Mixed-source change stays in custody until ownership is apportioned.
            sources = self.db.execute("""
                SELECT claim_id,reserved_sat FROM payout_claim_allocation WHERE disposition_id=?
            """, (disposition_id,)).fetchall()
            if proofs and len(sources) == 1:
                claim_id, reserved = sources[0]
                returned = sum(proof["amount"] for proof in proofs)
                if returned >= reserved:
                    raise ValueError("payout change exceeds available allocation")
                self._index_outputs(claim_id, proofs)
                self.db.execute("""
                    INSERT INTO payout_change_credit VALUES (?,?)
                    ON CONFLICT(disposition_id) DO NOTHING
                """, (disposition_id, returned))

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
            self._index_outputs(request_id, proofs)
