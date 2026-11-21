"""Claim settled project quotes using only their original, journaled outputs."""

import hashlib
import json
import secrets
from urllib.parse import quote as escape
import time

import bolt11

from cashu.core.base import BlindedMessage
from cashu.core.crypto import b_dhke
from cashu.core.crypto.secp import PrivateKey, PublicKey
from cashu.core.nuts import nut20


def unblind(item, promise, pinned_keys):
    amount, keyset = promise["amount"], promise["id"]
    if type(amount) is not int or amount <= 0 or keyset != item["id"]:
        raise ValueError("invalid mint promise denomination or keyset")
    mint_key = PublicKey(bytes.fromhex(pinned_keys[keyset][str(amount)]))
    blind_signature = PublicKey(bytes.fromhex(promise["C_"]))
    dleq = promise["dleq"]
    if not b_dhke.alice_verify_dleq(
        PublicKey(bytes.fromhex(item["B_"])), blind_signature,
        PrivateKey(bytes.fromhex(dleq["e"])), PrivateKey(bytes.fromhex(dleq["s"])), mint_key,
    ):
        raise ValueError("invalid project mint signature")
    signature = b_dhke.step3_alice(blind_signature, PrivateKey(bytes.fromhex(item["blind"])), mint_key)
    return {"id": keyset, "amount": amount, "secret": item["secret"],
                   "C": signature.format().hex(),
                   "dleq": {"e": dleq["e"], "s": dleq["s"], "r": item["blind"]}}


def collect(store, client, request_id, pinned_keys):
    row = store.db.execute("""
        SELECT terms, material, invoice, payment_hash, proofs FROM project_claim
        WHERE id=? AND invoice IS NOT NULL
    """, (request_id,)).fetchone()
    if row is None:
        raise ValueError("unknown project invoice")
    terms, material, invoice, payment_hash, saved = row
    if saved is not None:
        return True
    quote_id = "goats-" + request_id
    quote = client.get(f"/v1/mint/quote/bolt11/{quote_id}").raise_for_status().json()
    if quote["quote"] != quote_id or quote["request"] != invoice:
        raise ValueError("project quote binding differs")
    if quote["state"] == "UNPAID":
        store.schedule_unpaid(request_id, int(time.time()), bolt11.decode(invoice).expiry_time)
        return False
    material = json.loads(material)
    secrets = material["outputs"]
    outputs = [{k: item[k] for k in ("id", "amount", "B_")} for item in secrets]
    if quote["state"] == "PAID":
        signature = nut20.sign_mint_quote(
            quote_id, [BlindedMessage(**item) for item in outputs], material["owner"]
        )
        reply = client.post("/v1/mint/bolt11", json={
            "quote": quote_id, "outputs": outputs, "signature": signature,
        }).raise_for_status().json()
    elif quote["state"] == "ISSUED":
        reply = client.post("/v1/restore", json={"outputs": outputs}).raise_for_status().json()
        if [o["B_"] for o in reply["outputs"]] != [o["B_"] for o in outputs]:
            raise ValueError("restore returned different project outputs")
    else:
        raise ValueError("unknown project quote state")

    proofs = []
    for item, promise in zip(secrets, reply["signatures"], strict=True):
        amount, keyset = item["amount"], item["id"]
        if amount != promise["amount"] or keyset != promise["id"]:
            raise ValueError("project promise terms differ")
        proofs.append(unblind(item, promise, pinned_keys))
    if sum(p["amount"] for p in proofs) != json.loads(terms)["amount"]:
        raise ValueError("project claim amount differs")
    store.record_proofs(request_id, payment_hash, proofs)
    return True


def submit_payout(store, client, disposition_id, invoice_text, quote_id, keyset, pinned_keys):
    """Submit reserved inputs once, or read the bound quote after an interruption."""
    row = store.db.execute(
        "SELECT binding FROM proof_disposition WHERE id=? AND purpose='cyberherd_payout'",
        (disposition_id,),
    ).fetchone()
    if row is None:
        raise ValueError("unknown reserved payout")
    terms = json.loads(row[0])
    invoice = bolt11.decode(invoice_text)
    if (invoice.currency != "bc" or invoice.payment_hash != terms["invoice_hash"]
            or invoice.amount_msat != 1000 * terms["amount_sat"]):
        raise ValueError("signed payout invoice differs from reserved terms")
    status = client.get("/v1/melt/quote/bolt11/" + escape(quote_id, safe="")).raise_for_status().json()
    if (status["quote"] != quote_id or status["request"] != invoice_text
            or status["unit"] != "sat" or status["amount"] != terms["amount_sat"]
            or type(status["fee_reserve"]) is not int
            or not 0 <= status["fee_reserve"] <= terms["maximum_fee_sat"]):
        raise ValueError("mint melt quote differs from reserved payout")

    def generate_change():
        blanks = []
        maximum = terms["maximum_fee_sat"]
        count = max(1, (maximum - 1).bit_length()) if maximum else 0
        for _ in range(count):
            secret = secrets.token_hex(32)
            blinded, factor = b_dhke.step1_alice(secret)
            blanks.append({"amount": 0, "id": keyset, "B_": blinded.format().hex(),
                           "secret": secret, "blind": factor.secret.hex()})
        return blanks

    prior = store.db.execute("SELECT quote_id FROM payout_melt WHERE disposition_id=?",
                             (disposition_id,)).fetchone()
    def verify_result(result):
        if result["quote"] != quote_id or result["state"] not in ("PAID", "PENDING"):
            raise ValueError("unexpected payout result")
        if result["state"] == "PAID":
            preimage = bytes.fromhex(result["payment_preimage"])
            if len(preimage) != 32 or hashlib.sha256(preimage).hexdigest() != invoice.payment_hash:
                raise ValueError("invalid payout preimage")
            saved = store.db.execute("""
                SELECT change_material FROM payout_melt JOIN payout_melt_submission USING(disposition_id)
                WHERE disposition_id=? AND quote_id=?
            """, (disposition_id, quote_id)).fetchone()
            if saved is None:
                raise ValueError("missing payout submission")
            blanks = json.loads(saved[0])
            promises = result.get("change") or []
            if len(promises) > len(blanks):
                raise ValueError("too many change promises")
            proofs = [unblind(item, promise, pinned_keys) for item, promise in zip(blanks, promises)]
            fee = terms["maximum_fee_sat"] - sum(p["amount"] for p in proofs)
            if fee < 0:
                raise ValueError("excess payout change")
            store.record_melt_result(disposition_id, fee, preimage.hex(), proofs)
        return result

    if status["state"] != "UNPAID":
        if prior != (quote_id,) or status["state"] not in ("PENDING", "PAID"):
            raise ValueError("unexpected payout quote state")
        return verify_result(status)
    if invoice.expiry_time <= time.time() or status["expiry"] <= time.time():
        raise ValueError("expired payout quote or invoice")
    blanks = store.prepare_melt(disposition_id, invoice.payment_hash, quote_id, generate_change)
    inputs = [json.loads(r[0]) for r in store.db.execute("""
        SELECT o.proof FROM collected_output o JOIN disposition_input i USING(secret)
        WHERE i.disposition_id=? ORDER BY o.secret
    """, (disposition_id,))]
    if sum(p["amount"] for p in inputs) != terms["amount_sat"] + terms["maximum_fee_sat"]:
        raise ValueError("payout inputs differ from reserved total")
    outputs = [{k: item[k] for k in ("amount", "id", "B_")} for item in blanks]
    if not store.begin_melt_submission(disposition_id, quote_id):
        return status
    return verify_result(client.post("/v1/melt/bolt11", json={
        "quote": quote_id, "inputs": inputs, "outputs": outputs,
    }).raise_for_status().json())
