"""Confirmed-feed collector allocation and replay checks using synthetic proofs."""
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from uuid import uuid4

store_module = runpy.run_path(str(Path(__file__).resolve().parents[1] / "breez-reserve/collector/claim_store.py"))
ClaimStore = store_module["ClaimStore"]
encode = store_module["encode"]


class OperatorTransfers(unittest.TestCase):
    def test_reservation_survives_restart_with_and_without_member_rewards(self):
        for member in (0, 2):
            with self.subTest(member=member), tempfile.TemporaryDirectory() as directory:
                store = ClaimStore(Path(directory))
                claim, feed = str(uuid4()), str(uuid4())
                proofs = [{"secret": str(uuid4()), "amount": sats} for sats in (8, 2)]
                store.prepare(claim, {"attribution": {"credit_pool": "herd"}}, list)
                store.record_invoice(claim, "fixture", "01" * 32)
                store.record_proofs(claim, "01" * 32, proofs)
                allocations = {claim: {"consumed_sats": 10, "evidence_digest": hashlib.sha256(encode(proofs).encode()).hexdigest()}}
                operator = {"plan_hash": "02" * 32, "member_reserved_sat": member, "operator_sat": 10-member}
                terms = (feed, feed, "03" * 32, 9-member, 1, allocations)
                with self.assertRaises(ValueError):
                    store.reserve_payout(*terms, operator={**operator, "operator_sat": 11}, invoice_digest="04"*32)
                original = store.reserve_payout(*terms, operator=operator, invoice_digest="04"*32)
                self.assertEqual(sum(p["amount"] for p in original), 10-member)
                store.db.close()
                store = ClaimStore(Path(directory))
                self.assertEqual(original, store.reserve_payout(*terms, operator=operator, invoice_digest="04"*32))
                with self.assertRaises(ValueError):
                    store.reserve_payout(str(uuid4()), *terms[1:], operator=operator, invoice_digest="04"*32)
                with self.assertRaises(ValueError):
                    store.reserve_payout(*terms, operator=operator, invoice_digest="05"*32)
                if member:
                    reserved = store.reserve_payout(str(uuid4()), feed, "06"*32, member, 0, allocations)
                    self.assertEqual(sum(p["amount"] for p in reserved), member)
                self.assertEqual(store.db.execute("SELECT COUNT(*) FROM proof_disposition WHERE purpose='operator_transfer'").fetchone(), (1,))
                store.db.close()
