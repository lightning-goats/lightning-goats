"""Swap confirmed collector allocations into offline, recipient-only Cashu proofs."""
import fcntl
import json
import re
from pathlib import Path
from uuid import UUID

from coincurve import PublicKey
from claim_store import SplitRequired, encode
from collect import payout_input_fee, project_allocations, recover_swap
from project_quote import prepare_outputs
from operator_proofs import encode_token

MINT = "https://mint.lightning-goats.com"


def select(store, feed, gross, allocations):
    try:
        selected, _ = store.payout_inputs(feed, gross, allocations)
        parts = [(s, None) for s in selected]
    except SplitRequired as plan:
        if any(portion is None for _, portion in plan.splits):
            raise ValueError("collector is recovering an earlier split")
        parts = [(s, None) for s in plan.selected] + plan.splits
    inputs = []
    spent = {}
    change = {}
    for secret, portion in parts:
        claim, raw = store.db.execute("SELECT request_id,proof FROM collected_output WHERE secret=?", (secret,)).fetchone()
        if store.db.execute("SELECT 1 FROM disposition_input WHERE secret=?", (secret,)).fetchone():
            raise ValueError("input already reserved")
        proof = json.loads(raw)
        used = proof["amount"] if portion is None else portion
        spent[claim] = spent.get(claim, 0) + used
        change[claim] = change.get(claim, 0) + proof["amount"] - used
        inputs.append(proof)
    if sum(spent.values()) != gross:
        raise ValueError("operator input allocation mismatch")
    return inputs, spent, change


def transfer(store, client, request, keyset, keys):
    return _transfer(store, client, request, keyset, keys, request["feed"], None)


def _transfer(store, client, request, keyset, keys, disposition, failed_members):
    if set(request) != {"feed", "mint", "recipient", "gross", "member", "plan_hash", "lots", "fee_limit"}:
        raise ValueError("invalid direct payout request")
    feed = str(UUID(request["feed"]))
    recipient = request["recipient"]
    if (request["mint"] != MINT
            or not isinstance(recipient, str)
            or not re.fullmatch(r"0[23][0-9a-f]{64}", recipient)
            or PublicKey(bytes.fromhex(recipient)).format().hex() != recipient
            or not re.fullmatch(r"[0-9a-f]{64}", request["plan_hash"])):
        raise ValueError("invalid direct payout destination")
    allocations = project_allocations(request["lots"])
    gross, member, limit = (request[k] for k in ("gross", "member", "fee_limit"))
    if (any(type(n) is not int for n in (gross, member, limit)) or not 0 <= limit <= gross
            or gross <= 0 or member < 0 or sum(a["consumed_sats"] for a in allocations.values()) != gross + member):
        raise ValueError("invalid direct payout budget")
    db = store.db
    path = Path(db.execute("PRAGMA database_list").fetchone()[2])
    binding = encode(request if failed_members is None else {**request, "failed_members": failed_members})
    with path.with_suffix(".melt.lock").open("ab") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        with store.write():
            old = db.execute("SELECT binding FROM proof_disposition WHERE id=?", (disposition,)).fetchone()
            if old and old != (binding,):
                raise ValueError("operator method or terms changed")
            if old:
                material, amount, fee, ready = db.execute(
                    "SELECT material,amount_sat,fee_sat,token IS NOT NULL "
                    "FROM operator_ecash WHERE feed=?", (disposition,)
                ).fetchone()
                if ready:
                    return {"state": "ready", "amount_sat": amount, "fee_sat": fee}
                outputs = json.loads(material)
                rows = db.execute(
                    "SELECT o.proof FROM collected_output o JOIN disposition_input i USING(secret) "
                    "WHERE i.disposition_id=? ORDER BY secret", (disposition,)
                )
                inputs = [json.loads(proof) for proof, in rows]
            else:
                if db.execute("SELECT 1 FROM payout_split_cost WHERE disposition_id=?", (disposition,)).fetchone():
                    raise ValueError("operator invoice split already reserved")
                info = client.get("/v1/info").raise_for_status().json()
                if info.get("nuts", {}).get("11", {}).get("supported") is not True:
                    return {"state": "held", "reason": "nut11_unsupported"}
                inputs, spent, change = select(store, feed, gross, allocations)
                fee = payout_input_fee(client, [p["id"] for p in inputs])
                amount = gross - fee
                if amount <= 0 or fee > limit:
                    return {"state": "held", "reason": "no_payable_amount" if amount <= 0 else "fee_limit"}
                outputs = [(None, o) for o in prepare_outputs(amount, keyset, recipient=recipient)]
                for claim, sats in change.items():
                    outputs.extend((claim, output) for output in prepare_outputs(sats, keyset))
                db.execute("INSERT INTO proof_disposition VALUES (?,'operator_transfer',?)", (disposition, binding))
                db.executemany("INSERT INTO disposition_input VALUES (?,?)", [(p["secret"], disposition) for p in inputs])
                db.executemany("INSERT INTO payout_claim_allocation VALUES (?,?,?,?,?)",
                               [(disposition, feed, claim, allocations[claim]["consumed_sats"], used)
                                for claim, used in spent.items()])
                db.execute("INSERT INTO operator_ecash(feed,material,amount_sat,fee_sat) VALUES(?,?,?,?)",
                           (disposition, encode(outputs), amount, fee))
        proofs = recover_swap(
            client, sorted(inputs, key=lambda p: p["secret"]),
            [output for _, output in outputs], keys, require_unspent=True,
        )
        operator = []
        with store.write():
            for (claim, _), proof in zip(outputs, proofs, strict=True):
                if claim is None:
                    operator.append(proof)
                else:
                    store._index_outputs(claim, [proof])
            if sum(p["amount"] for p in operator) != amount:
                raise ValueError("mint returned inconsistent operator amount")
            encoded = encode_token(MINT, operator)
            db.execute("UPDATE operator_ecash SET token=? WHERE feed=? AND token IS NULL", (encoded, disposition))
        return {"state": "ready", "amount_sat": amount, "fee_sat": fee}
