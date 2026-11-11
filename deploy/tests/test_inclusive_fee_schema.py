"""Exercise actual payout migrations with SQLite; no provider or service calls.

These tests establish schema constraints, not typed invoice validation or runtime
activation. Native Rust tests separately exercise funding and retained grants.
"""
import sqlite3
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class InclusiveFeeSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.addCleanup(self.db.close)
        self.db.execute("PRAGMA foreign_keys=ON")
        for migration in sorted((ROOT / "payout-migrations").glob("*.sql")):
            self.db.executescript(migration.read_text())
        self.db.execute(
            "INSERT INTO payout_grants VALUES(?,?,?,?,?,?,?)",
            ("grant", "feed", "hash", "{}", "{}", 100, 100),
        )
        self.db.execute(
            "INSERT INTO payout_rows VALUES(?,?,?,?,?,?,?)",
            ("grant", "member", "member@example.com", "{}", 100, 0, 100),
        )

    def probe(self, round_, net, previous=None, created=100, member="member"):
        self.db.execute(
            "INSERT INTO payout_fee_probe_intents VALUES(?,?,?,?,?,?,?,?)",
            (f"probe-{round_}", "grant", member, round_, previous, net, "{}", created),
        )

    def observe(self, round_, net, fee, observed=100):
        self.db.execute(
            "INSERT INTO payout_fee_observations VALUES(?,?,?,?,?,?,?,?,?,?)",
            ("grant", "member", round_, f"quote-{round_}", 100, net, fee,
             net + fee, observed, "{}"),
        )

    def test_adjustments_stop_after_fit_and_preserve_original_budget(self):
        self.probe(0, 100)
        self.observe(0, 100, 2)
        self.probe(1, 98, "quote-0")
        # A changed route fee requires another strictly smaller invoice.
        self.observe(1, 98, 3)
        self.probe(2, 97, "quote-1")
        self.observe(2, 97, 2)
        with self.assertRaises(sqlite3.IntegrityError):
            self.probe(3, 98, "quote-2")
        self.assertEqual(
            self.db.execute("SELECT net_sats,fee_limit_sats,gross_sats FROM payout_rows").fetchone(),
            (100, 0, 100),
        )
        self.assertEqual(self.db.execute("SELECT count(*) FROM payout_execute_intents").fetchone()[0], 0)

    def test_refuses_skipped_round_changed_amount_member_and_earlier_time(self):
        for round_, net, previous, created, member in [
            (0, 99, None, 100, "member"),
            (0, 100, None, 99, "member"),
            (0, 100, None, 100, "other"),
            (1, 98, None, 100, "member"),
        ]:
            with self.assertRaises(sqlite3.IntegrityError):
                self.probe(round_, net, previous, created, member)
        self.probe(0, 100)
        self.observe(0, 100, 2, observed=101)
        for round_, net, previous, created in [
            (1, 98, "quote-0", 100),
            (1, 99, "quote-0", 101),
            (2, 98, "quote-0", 101),
            (1, 98, "missing", 101),
        ]:
            with self.assertRaises(sqlite3.IntegrityError):
                self.probe(round_, net, previous, created)
        self.probe(1, 98, "quote-0", 101)

    def test_fee_consuming_entire_share_never_allows_another_probe(self):
        self.probe(0, 100)
        self.observe(0, 100, 100)
        for net in (0, 1, 100):
            with self.assertRaises(sqlite3.IntegrityError):
                self.probe(1, net, "quote-0")

    def test_probe_cannot_start_after_legacy_invoice_identity_exists(self):
        self.db.execute("INSERT INTO payout_invoice_intents VALUES('legacy','grant','member','{}')")
        with self.assertRaises(sqlite3.IntegrityError):
            self.probe(0, 100)

    def test_probe_chain_is_append_only_and_quote_cannot_execute(self):
        self.probe(0, 100)
        self.db.execute("INSERT INTO payout_fee_probe_callbacks VALUES('probe-0',NULL,100)")
        self.db.execute("INSERT INTO payout_fee_probe_invoices VALUES('probe-0','hash','{}',100)")
        self.db.execute("INSERT INTO payout_fee_probe_quotes VALUES('probe-quote','probe-0','{}')")
        self.observe(0, 100, 2)
        for table in ("payout_fee_probe_intents", "payout_fee_probe_callbacks",
                      "payout_fee_probe_invoices", "payout_fee_probe_quotes",
                      "payout_fee_observations"):
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(f"UPDATE {table} SET document_json='changed'" if table != "payout_fee_probe_callbacks"
                                else f"UPDATE {table} SET raw_zap='changed'")
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(f"DELETE FROM {table}")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO payout_execute_intents VALUES('probe-quote','{}',100)")

    def test_backup_preserves_discovery_identity_and_constraints(self):
        self.probe(0, 100)
        self.observe(0, 100, 2)
        self.db.commit()
        restored = sqlite3.connect(":memory:")
        self.addCleanup(restored.close)
        self.db.backup(restored)
        self.assertEqual(restored.execute("PRAGMA integrity_check").fetchone()[0], "ok")
        self.assertEqual(restored.execute("SELECT id,net_sats FROM payout_fee_probe_intents").fetchall(), [("probe-0", 100)])
        with self.assertRaises(sqlite3.IntegrityError):
            restored.execute("DELETE FROM payout_fee_observations")


if __name__ == "__main__":
    unittest.main()
