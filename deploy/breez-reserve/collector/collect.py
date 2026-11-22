"""Claim settled project quotes using only their original, journaled outputs."""

import fcntl
import hashlib
import json
from pathlib import Path
import secrets
from urllib.parse import quote as escape
from uuid import UUID, uuid4

from project_quote import prepare_outputs
from claim_store import SplitRequired, encode
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


def payout_input_fee(client, input_keysets):
    keysets = client.get("/v1/keysets").raise_for_status().json()["keysets"]
    rates = {}
    for item in keysets:
        if item["unit"] == "sat":
            rate = item.get("input_fee_ppk", 0)
            if type(rate) is not int or rate < 0 or item["id"] in rates:
                raise ValueError("invalid mint input fee schedule")
            rates[item["id"]] = rate
    return (sum(rates[keyset] for keyset in input_keysets) + 999) // 1000


def recover_swap(client, inputs, material, pinned_keys):
    outputs = [{k: item[k] for k in ("amount", "id", "B_")} for item in material]
    restored = client.post("/v1/restore", json={"outputs": outputs}).raise_for_status().json()
    if restored["outputs"] or restored["signatures"]:
        if [{k: o[k] for k in ("amount","id","B_")} for o in restored["outputs"]] != outputs:
            raise ValueError("swap restoration differs from original outputs")
        promises = restored["signatures"]
    else:
        input_total = sum(p["amount"] for p in inputs)
        output_total = sum(o["amount"] for o in outputs)
        if not outputs or input_total - output_total != payout_input_fee(client, [p["id"] for p in inputs]):
            raise ValueError("frozen swap outputs differ from the current mint fee")
        promises = client.post("/v1/swap", json={"inputs": inputs, "outputs": outputs}).raise_for_status().json()["signatures"]
    if [p["amount"] for p in promises] != [o["amount"] for o in outputs]:
        raise ValueError("swap promise amounts changed")
    return [unblind(item, promise, pinned_keys) for item, promise in zip(material, promises, strict=True)]


def split_payout_proof(store, client, secret, portion, keyset, pinned_keys):
    """Make exact change without consuming a payout fee or changing claim ownership."""
    row = store.db.execute("SELECT request_id,proof FROM collected_output WHERE secret=?", (secret,)).fetchone()
    if row is None:
        raise ValueError("missing split input")
    claim_id, proof = row[0], json.loads(row[1])
    known = store.db.execute("SELECT 1 FROM proof_split WHERE secret=?", (secret,)).fetchone()
    if not known and payout_input_fee(client, [proof["id"], keyset]):
        raise ValueError("proof split requires zero-fee keysets")
    with store.write():
        prior = store.db.execute("SELECT material,complete FROM proof_split WHERE secret=?", (secret,)).fetchone()
        if prior:
            if prior[1]:
                return
            material = json.loads(prior[0])
        else:
            if type(portion) is not int or not 0 < portion < proof["amount"]:
                raise ValueError("invalid split portion")
            if store.db.execute("SELECT 1 FROM disposition_input WHERE secret=?", (secret,)).fetchone():
                raise ValueError("split input already reserved")
            material = prepare_outputs(portion, keyset) + prepare_outputs(proof["amount"] - portion, keyset)
            split_id = str(uuid4())
            store.db.execute("INSERT INTO proof_disposition VALUES (?,'operator_transfer',?)",
                             (split_id, json.dumps({"purpose": "make_change", "claim": claim_id})))
            store.db.execute("INSERT INTO disposition_input VALUES (?,?)", (secret, split_id))
            store.db.execute("INSERT INTO proof_split(secret,material) VALUES (?,?)", (secret, json.dumps(material)))
    proofs = recover_swap(client, [proof], material, pinned_keys)
    with store.write():
        store._index_outputs(claim_id, proofs)
        store.db.execute("UPDATE proof_split SET complete=1 WHERE secret=?", (secret,))


def recover_payout_refund(store, client, disposition_id, keyset, pinned_keys):
    """Restore mixed-claim fee change using the original private swap outputs."""
    with store.write():
        if store.db.execute("SELECT 1 FROM payout_claim_refund WHERE disposition_id=?",
                            (disposition_id,)).fetchone():
            return
        row = store.db.execute("SELECT change_proofs FROM payout_melt_result WHERE disposition_id=?",
                               (disposition_id,)).fetchone()
        inputs = json.loads(row[0])
        remaining = sum(p["amount"] for p in inputs)
        if not remaining:
            return
        prior = store.db.execute("SELECT material FROM payout_refund_swap WHERE disposition_id=?",
                                 (disposition_id,)).fetchone()
        if prior:
            plan = json.loads(prior[0])
        else:
            plan = []
            for claim, reserved in store.db.execute("""
                SELECT claim_id,reserved_sat FROM payout_claim_allocation
                WHERE disposition_id=? ORDER BY reserved_sat DESC,claim_id
            """, (disposition_id,)):
                share = min(remaining, reserved)
                plan.extend((claim, output) for output in prepare_outputs(share, keyset))
                remaining -= share
                if not remaining:
                    break
            if remaining:
                raise ValueError("refund exceeds reserved inputs")
            store.db.execute("INSERT INTO payout_refund_swap VALUES (?,?)", (disposition_id, encode(plan)))
    outputs = recover_swap(client, inputs, [output for _, output in plan], pinned_keys)
    by_claim = {}
    for (claim, _), proof in zip(plan, outputs, strict=True):
        by_claim.setdefault(claim, []).append(proof)
    with store.write():
        for claim, proofs in by_claim.items():
            store._index_outputs(claim, proofs)
            store.db.execute("INSERT INTO payout_claim_refund VALUES (?,?,?)",
                             (disposition_id, claim, sum(p["amount"] for p in proofs)))


def collect_acquisition(store, client, claim_id, evidence_digest, inputs, keyset, pinned_keys):
    """Receive wallet-owned project tokens by swapping once into collector-owned outputs."""
    if (not isinstance(evidence_digest, str) or len(evidence_digest) != 64
            or any(c not in "0123456789abcdef" for c in evidence_digest)
            or not isinstance(inputs, list) or not 0 < len(inputs) <= 1024):
        raise ValueError("invalid acquisition handoff")
    if any(type(p["amount"]) is not int or p["amount"] <= 0 or p["id"] not in pinned_keys
           for p in inputs) or len({p["secret"] for p in inputs}) != len(inputs):
        raise ValueError("invalid acquisition proofs")
    amount = sum(p["amount"] for p in inputs)
    if amount > 1_000_000:
        raise ValueError("acquisition exceeds mint limit")
    inputs = sorted(inputs, key=lambda p: p["secret"])
    terms = {"amount":amount, "attribution":{"purpose":"acquisition","credit_pool":"herd"},
             "input_digest":hashlib.sha256(encode(inputs).encode()).hexdigest(), "evidence_digest":evidence_digest}
    def prepare_handoff():
        fee = payout_input_fee(client, [p["id"] for p in inputs])
        if fee >= amount:
            raise ValueError("handoff fee would consume the receipt")
        return prepare_outputs(amount - fee, keyset)

    material = store.prepare(claim_id, terms, prepare_handoff)
    prior = store.db.execute("SELECT proofs FROM acquired_custody WHERE claim_id=?", (claim_id,)).fetchone()
    if prior:
        return sum(proof["amount"] for proof in json.loads(prior[0]))
    proofs = recover_swap(client, inputs, material, pinned_keys)
    with store.write():
        store.db.execute("INSERT INTO acquired_custody VALUES (?,?,?) ON CONFLICT(claim_id) DO NOTHING",
                         (claim_id,evidence_digest,encode(proofs)))
        if store.db.execute("SELECT proofs FROM acquired_custody WHERE claim_id=?", (claim_id,)).fetchone() != (encode(proofs),):
            raise ValueError("acquisition handoff evidence changed")
        store._index_outputs(claim_id, proofs)
    return sum(proof["amount"] for proof in proofs)


def project_allocations(parts):
    allocations = {}
    ledger_ids = set()
    for part in parts:
        anchor = part["anchor"]
        ident = anchor["claim_id"]
        if (anchor["source"] not in ("acquisition", "project_invoice") or ident in allocations
                or type(part["ledger_id"]) is not int or part["ledger_id"] <= 0
                or part["ledger_id"] in ledger_ids):
            raise ValueError("invalid project payout allocation")
        ledger_ids.add(part["ledger_id"])
        allocations[ident] = {"consumed_sats": part["consumed_sats"],
                              "evidence_digest": anchor["evidence_digest"]}
        if anchor["source"] == "acquisition":
            allocations[ident]["source"] = "acquisition"
    return allocations


def execute_payout(store, client, request, keyset, pinned_keys):
    """Execute an authenticated payout worker request using its admitted grant evidence."""
    probe = {"quote_invoice", "amount_sat"}
    with_inputs = probe | {"feed_id", "gross_sat", "retained_allocations"}
    if set(request) in (probe, with_inputs):
        invoice_text = request["quote_invoice"]
        invoice = bolt11.decode(invoice_text)
        amount = request["amount_sat"]
        if (type(amount) is not int or not 0 < amount <= 1_000_000 or invoice.currency != "bc"
                or invoice.amount_msat != amount * 1000 or invoice.expiry_time <= time.time()):
            raise ValueError("invalid payout fee discovery invoice")
        status = client.post("/v1/melt/quote/bolt11", json={
            "request": invoice_text, "unit": "sat",
        }).raise_for_status().json()
        if (status.get("request") != invoice_text or status.get("unit") != "sat"
                or status.get("state") != "UNPAID" or type(status.get("amount")) is not int
                or status["amount"] != amount or type(status.get("fee_reserve")) is not int
                or status["fee_reserve"] < 0 or type(status.get("expiry")) is not int
                or status["expiry"] <= time.time()):
            raise ValueError("invalid payout fee quote")
        result = {"payment_hash": invoice.payment_hash, "amount_sat": amount,
                  "routing_reserve_sat": status["fee_reserve"], "expires_at": status["expiry"]}
        if set(request) == with_inputs:
            try:
                selected, _ = store.payout_inputs(request["feed_id"], request["gross_sat"],
                                                  project_allocations(request["retained_allocations"]))
            except SplitRequired as plan:
                # Only a lossless swap is supported. Fee discovery never submits inputs.
                split_keys = [json.loads(store.db.execute("SELECT proof FROM collected_output WHERE secret=?",
                              (secret,)).fetchone()[0])["id"] for secret, _ in plan.splits]
                if payout_input_fee(client, split_keys + [keyset]):
                    raise ValueError("proof split requires zero-fee keysets")
                selected = plan.selected
            keysets = [json.loads(store.db.execute("SELECT proof FROM collected_output WHERE secret=?",
                                                   (secret,)).fetchone()[0])["id"] for secret in selected]
            result["input_fee_sat"] = payout_input_fee(client, keysets)
        return result
    if set(request) == {"reconcile_operator", "invoice"}:
        feed = str(UUID(request["reconcile_operator"]))
        if not store.db.execute(
            "SELECT 1 FROM proof_disposition WHERE id=? AND purpose='operator_transfer' "
            "AND json_extract(binding,'$.feed')=? AND json_type(binding,'$.operator')='object'",
            (feed, feed),
        ).fetchone():
            return {"state": "UNRESERVED"}
        return submit_payout(store, client, feed, request["invoice"], keyset, pinned_keys, reconcile_only=True)
    if set(request) == {"reconcile_disposition", "invoice"}:
        return submit_payout(store, client, request["reconcile_disposition"], request["invoice"],
                             keyset, pinned_keys, reconcile_only=True)
    payment_fields = {"disposition_id", "feed_id", "invoice", "payment_hash", "amount_sat",
                      "maximum_fee_sat", "retained_allocations"}
    if set(request) not in (payment_fields, payment_fields | {"operator"}) or ("operator" in request and request["operator"] is None):
        raise ValueError("invalid payout request")
    invoice = bolt11.decode(request["invoice"])
    if (type(request["amount_sat"]) is not int or invoice.currency != "bc"
            or invoice.amount_msat != request["amount_sat"] * 1000
            or invoice.payment_hash != request["payment_hash"]):
        raise ValueError("recipient invoice differs from payout authorization")
    allocations = project_allocations(request["retained_allocations"])
    while True:
        try:
            store.reserve_payout(request["disposition_id"], request["feed_id"], invoice.payment_hash,
                                 request["amount_sat"], request["maximum_fee_sat"], allocations,
                                 invoice_expiry=invoice.expiry_time,
                                 invoice_digest=hashlib.sha256(request["invoice"].encode()).hexdigest(),
                                 operator=request.get("operator"))
            break
        except SplitRequired as plan:
            for secret, portion in plan.splits:
                split_payout_proof(store, client, secret, portion, keyset, pinned_keys)
    result = submit_payout(store, client, request["disposition_id"], request["invoice"], keyset, pinned_keys)
    if result["state"] == "PAID":
        return result
    return {"state": result["state"], "quote": result["quote"]}


def submit_payout(store, client, disposition_id, invoice_text, keyset, pinned_keys, *, reconcile_only=False):
    """Serialize dispatch and recovery across processes sharing this custody journal."""
    database = Path(store.db.execute("PRAGMA database_list").fetchone()[2])
    with database.with_suffix(".melt.lock").open("ab") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        return _submit_payout(store, client, disposition_id, invoice_text, keyset, pinned_keys, reconcile_only)


def _submit_payout(store, client, disposition_id, invoice_text, keyset, pinned_keys, reconcile_only):
    row = store.db.execute(
        "SELECT d.binding FROM proof_disposition d JOIN payout_invoice i ON i.disposition_id=d.id "
        "WHERE d.id=? AND (d.purpose='cyberherd_payout' OR "
        "(d.purpose='operator_transfer' AND json_type(d.binding,'$.operator')='object'))",
        (disposition_id,),
    ).fetchone()
    if row is None:
        if reconcile_only:
            return {"state": "UNRESERVED"}
        raise ValueError("unknown reserved payout")
    terms = json.loads(row[0])
    invoice = bolt11.decode(invoice_text)
    if "invoice_digest" in terms and terms["invoice_digest"] != hashlib.sha256(invoice_text.encode()).hexdigest():
        raise ValueError("signed payout invoice differs from reserved terms")
    if (invoice.currency != "bc" or invoice.payment_hash != terms["invoice_hash"]
            or invoice.amount_msat != 1000 * terms["amount_sat"]):
        raise ValueError("signed payout invoice differs from reserved terms")
    def paid_receipt(quote, fee, preimage):
        if (type(fee) is not int or not 0 <= fee <= terms["maximum_fee_sat"]
                or len(preimage) != 64 or hashlib.sha256(bytes.fromhex(preimage)).hexdigest() != invoice.payment_hash):
            raise ValueError("invalid journaled payout settlement")
        return {"state": "PAID", "quote": quote, "amount_sat": terms["amount_sat"],
                "fee_sat": fee, "payment_hash": invoice.payment_hash, "payment_preimage": preimage}

    saved = store.db.execute("""
        SELECT m.quote_id,r.fee_sat,r.preimage FROM payout_melt m
        JOIN payout_melt_result r USING(disposition_id) WHERE m.disposition_id=?
    """, (disposition_id,)).fetchone()
    if saved is not None:
        return paid_receipt(*saved)

    prior = store.db.execute("SELECT quote_id FROM payout_melt WHERE disposition_id=?",
                             (disposition_id,)).fetchone()
    if prior is None:
        if reconcile_only:
            return {"state": "UNQUOTED"}
        if invoice.expiry_time <= time.time():
            raise ValueError("expired payout invoice")
        status = client.post("/v1/melt/quote/bolt11", json={
            "request": invoice_text, "unit": "sat",
        }).raise_for_status().json()
        quote_id = status["quote"]
        if not isinstance(quote_id, str) or not quote_id.strip():
            raise ValueError("missing mint melt quote")
    else:
        quote_id = prior[0]
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
            receipt = paid_receipt(quote_id, fee, preimage.hex())
            store.record_melt_result(disposition_id, fee, preimage.hex(), proofs)
            return receipt
        return result

    if status["state"] != "UNPAID":
        if prior != (quote_id,) or status["state"] not in ("PENDING", "PAID"):
            raise ValueError("unexpected payout quote state")
        if reconcile_only and status["state"] == "PENDING":
            return {"state": "PENDING", "quote": quote_id}
        return verify_result(status)
    if reconcile_only:
        return {"state": "UNPAID", "quote": quote_id}
    if invoice.expiry_time <= time.time() or status["expiry"] <= time.time():
        raise ValueError("expired payout quote or invoice")
    inputs = [json.loads(r[0]) for r in store.db.execute("""
        SELECT o.proof FROM collected_output o JOIN disposition_input i USING(secret)
        WHERE i.disposition_id=? ORDER BY o.secret
    """, (disposition_id,))]
    if sum(p["amount"] for p in inputs) != terms["amount_sat"] + terms["maximum_fee_sat"]:
        raise ValueError("payout inputs differ from reserved total")
    # NUT-02 charges per input, including proofs from inactive keysets.
    # The NUT-05 routing reserve is additional to those mint fees.
    input_fee = payout_input_fee(client, [p["id"] for p in inputs])
    if status["fee_reserve"] + input_fee > terms["maximum_fee_sat"]:
        raise ValueError("mint input fees and routing reserve exceed payout allowance")
    blanks = store.prepare_melt(disposition_id, invoice.payment_hash, quote_id, generate_change)
    outputs = [{k: item[k] for k in ("amount", "id", "B_")} for item in blanks]
    if not store.begin_melt_submission(disposition_id, quote_id):
        # Only replay the identical request if every reserved input is still available.
        ys = [b_dhke.hash_to_curve(p["secret"].encode()).format().hex() for p in inputs]
        checked = client.post("/v1/checkstate", json={"Ys": ys}).raise_for_status().json()["states"]
        if sorted(item["Y"] for item in checked) != sorted(ys):
            raise ValueError("incomplete payout input status")
        if any(item["state"] != "UNSPENT" for item in checked):
            raise ValueError("unpaid payout has unavailable inputs")
    return verify_result(client.post("/v1/melt/bolt11", json={
        "quote": quote_id, "inputs": inputs, "outputs": outputs,
    }).raise_for_status().json())
