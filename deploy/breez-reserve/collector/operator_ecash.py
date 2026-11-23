"""Swap confirmed collector allocations into offline, recipient-only Cashu proofs."""
import fcntl
import hashlib
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


def member_transfer(store, client, assignment, keyset, keys):
    fields = {"feed", "grant", "member_key", "plan_hash", "destination", "lots",
              "gross", "reserved", "fee_limit", "minimum_net", "maximum_net"}
    if set(assignment) != fields:
        raise ValueError("invalid member assignment")
    member = assignment["member_key"]
    grant = str(UUID(assignment["grant"]))
    feed = str(UUID(assignment["feed"]))
    destination = assignment["destination"]
    if (not isinstance(member, str) or not re.fullmatch(r"[0-9a-f]{64}", member)
            or destination.get("method") not in ("public_nutzap", "private_ecash")
            or destination.get("mint", "").rstrip("/") != MINT
            or destination["method"] == "public_nutzap" and destination.get("member") != member):
        raise ValueError("invalid member ecash destination")
    gross, reserved, minimum, maximum = (assignment[k] for k in ("gross","reserved","minimum_net","maximum_net"))
    total = sum(a["consumed_sats"] for a in project_allocations(assignment["lots"]).values())
    if (any(type(n) is not int for n in (gross, reserved, minimum, maximum))
            or not 0 < minimum <= maximum <= gross <= reserved <= total):
        raise ValueError("member share exceeds its reservation")
    request = {"feed": feed, "mint": MINT, "recipient": destination["receiving_key"],
               "gross": gross, "member": total-gross, "plan_hash": assignment["plan_hash"],
               "lots": assignment["lots"], "fee_limit": assignment["fee_limit"]}
    # Feed/member, never a fresh retry ID. A changed grant or delivery identity
    # conflicts with the original binding even when it has the same lock key.
    disposition = f"member-ecash:{feed}:{member}"
    digest = hashlib.sha256(json.dumps(assignment, ensure_ascii=False, sort_keys=True, separators=(",",":")).encode()).hexdigest()
    terms = {"terms_hash":digest, "grant":grant, "member_key":member, "reserved":reserved,
             "minimum_net":minimum, "maximum_net":maximum, "destination":destination}
    return _transfer(store, client, request, keyset, keys, disposition, None, terms)


def _transfer(store, client, request, keyset, keys, disposition, failed_members, member_terms=None):
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
    table, purpose = "operator_ecash", "operator_transfer"
    if member_terms is not None:
        table, purpose = "member_ecash", "cyberherd_payout"
        binding = encode({**request, "assignment":member_terms})
    with path.with_suffix(".melt.lock").open("ab") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        with store.write():
            old = db.execute("SELECT binding FROM proof_disposition WHERE id=?", (disposition,)).fetchone()
            if old and old != (binding,):
                raise ValueError("operator method or terms changed")
            if old:
                material, amount, fee, ready = db.execute(
                    "SELECT material,amount_sat,fee_sat,token IS NOT NULL "
                    f"FROM {table} WHERE feed=?", (disposition,)
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
                if member_terms is not None:
                    total = gross
                    for (prior_raw,) in db.execute("""
                            SELECT p.binding FROM proof_disposition p JOIN member_ecash m ON m.feed=p.id
                            WHERE json_extract(p.binding,'$.feed')=?""", (feed,)):
                        prior = json.loads(prior_raw)
                        previous = prior["assignment"]
                        if (previous["grant"] != member_terms["grant"] or prior["plan_hash"] != request["plan_hash"]
                                or previous["reserved"] != member_terms["reserved"]):
                            raise ValueError("member reservation changed")
                        total += prior["gross"]
                    if total > member_terms["reserved"]:
                        raise ValueError("combined member shares exceed reservation")
                if db.execute("SELECT 1 FROM payout_split_cost WHERE disposition_id=?", (disposition,)).fetchone():
                    raise ValueError("operator invoice split already reserved")
                info = client.get("/v1/info").raise_for_status().json()
                if info.get("nuts", {}).get("11", {}).get("supported") is not True:
                    return {"state": "held", "reason": "nut11_unsupported"}
                inputs, spent, change = select(store, feed, gross, allocations)
                fee = payout_input_fee(client, [p["id"] for p in inputs])
                amount = gross - fee
                if member_terms is not None and amount < member_terms["minimum_net"]:
                    return {"state":"held", "reason":"below_member_minimum"}
                if amount <= 0 or fee > limit:
                    return {"state": "held", "reason": "no_payable_amount" if amount <= 0 else "fee_limit"}
                if member_terms is not None and amount > member_terms["maximum_net"]:
                    # Return unused fixed fee allowance to the original funding lots.
                    refund = amount - member_terms["maximum_net"]
                    amount -= refund
                    for claim in sorted(spent):
                        restore = min(spent[claim], refund)
                        change[claim] += restore
                        spent[claim] -= restore
                        refund -= restore
                        if refund == 0:
                            break
                outputs = [(None, o) for o in prepare_outputs(amount, keyset, recipient=recipient)]
                for claim, sats in change.items():
                    outputs.extend((claim, output) for output in prepare_outputs(sats, keyset))
                db.execute("INSERT INTO proof_disposition VALUES (?,?,?)", (disposition, purpose, binding))
                db.executemany("INSERT INTO disposition_input VALUES (?,?)", [(p["secret"], disposition) for p in inputs])
                db.executemany("INSERT INTO payout_claim_allocation VALUES (?,?,?,?,?)",
                               [(disposition, feed, claim, allocations[claim]["consumed_sats"], used)
                                for claim, used in spent.items() if used])
                db.execute(f"INSERT INTO {table}(feed,material,amount_sat,fee_sat) VALUES(?,?,?,?)",
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
            db.execute(f"UPDATE {table} SET token=? WHERE feed=? AND token IS NULL", (encoded, disposition))
        return {"state": "ready", "amount_sat": amount, "fee_sat": fee}
