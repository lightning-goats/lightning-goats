"""Reconcile definitively unsubmitted member allocations under the existing service UIDs."""
import argparse
import fcntl
import json
import os
import re
import sqlite3
import sys
import tomllib
from pathlib import Path
from uuid import UUID, uuid5

PAYOUT = "/var/lib/cyberherd-payout/payout.db"
CORE = "/var/lib/lightning-goats/lightning-goats.db"
MEMBERS = "/var/lib/cyberherd/cyberherd.db"
COLLECTOR = Path("/var/lib/lightning-goats-project-collector")
CONFIG = Path("/etc/lightning-goats-project-collector")
MINT = "https://mint.lightning-goats.com"


def connect(path, mode="ro"):
    return sqlite3.connect(f"file:{path}?mode={mode}", uri=True, timeout=15)


def config(path):
    with open(path, "rb") as stream:
        return tomllib.load(stream)


def notices(db):
    return [source.split(":")[1:] for source, in db.execute(
        "SELECT source_id FROM domain_events WHERE event_type='primal_invoice_mismatch' "
        "AND source_id LIKE 'primal-invoice-mismatch:%' ORDER BY source_id")]


def qualify(db, failures, policy):
    if not (policy["enabled"] and policy["policy"]["execution_enabled"]
            and policy["funding_source"] == "retained_cashu"
            and policy.get("operator_p2pk", {}).get("mint") == MINT):
        return []
    terms = policy["policy"]
    failures = set(map(tuple, failures))
    # Durable no-execute closures are eligible even without a presentation event.
    failures.update(db.execute("""
        SELECT g.request_id,c.member FROM payout_unsubmitted c JOIN payout_grants g ON g.id=c.grant_id
        WHERE json_extract(g.qualification_json,'$.execution_provider')='cashu'
    """))
    result = []
    for feed, member in sorted(failures):
        row = db.execute("""
            SELECT g.id,g.plan_hash,r.gross_sats,g.reserved_sats,m.destination FROM payout_grants g
            JOIN payout_intake_receipts t ON t.grant_id=g.id AND t.request_id=g.request_id AND t.plan_hash=g.plan_hash
            JOIN payout_operator_methods m ON m.sequence=t.sequence AND m.method='p2pk'
            JOIN payout_rows r ON r.grant_id=g.id WHERE g.request_id=? AND r.member=?
            AND json_extract(g.qualification_json,'$.execution_provider')='cashu'
            AND (NOT EXISTS(SELECT 1 FROM payout_invoice_intents i WHERE i.grant_id=g.id AND i.member=r.member)
                     OR EXISTS(SELECT 1 FROM payout_unsubmitted c WHERE c.grant_id=g.id AND c.member=r.member))
                AND NOT EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_invoices v ON v.intent_id=i.id
                               WHERE i.grant_id=g.id AND i.member=r.member)
            AND NOT EXISTS(SELECT 1 FROM payout_outcome_manifests m WHERE m.grant_id=g.id)
        """, (feed,member)).fetchone()
        if not row:
            continue
        grant, plan, gross, reserved, destination = row
        wallet, frozen_mode, frozen_limit = json.loads(destination)
        if wallet["mint"] != MINT:
            continue
        frozen_fee = gross if frozen_mode == "deduct_from_share" else frozen_limit
        current_fee = gross if terms["fee_mode"] == "deduct_from_share" else terms["max_fee_sats"]
        if not 0 < gross <= reserved <= terms["max_sats_per_feed"]:
            continue
        result.append(dict(feed=feed, member=member, grant=grant, plan=plan, gross=gross,
                           reserved=reserved, recipient=wallet["recipient"],
                           fee_limit=min(gross, frozen_fee, current_fee)))
    return result


def authorize(db, rows, settings):
    bridge = settings["core_bridge"]
    if (settings["service"]["mode"] == "shadow" or not bridge["payout_execution_enabled"]
            or bridge["reward_policy"]["funding_source"] != "retained_cashu"):
        return []
    cap = bridge["reward_policy"]["max_sats_per_feed"]
    authorized = []
    for row in rows:
        if row["reserved"] > cap:
            continue
        check = db.execute("""
            SELECT 1 FROM reward_reservations r JOIN operator_p2pk_claims o USING(request_id)
            JOIN feed_attempts f ON f.id=r.request_id
            WHERE r.id=? AND r.request_id=? AND r.plan_hash=? AND r.reserved_sats=? AND r.owner=?
            AND f.status IN ('confirmed','reconciled_fed')
            AND NOT EXISTS(SELECT 1 FROM cashu_payout_execution_claims c WHERE c.reservation_id=r.id AND c.member=?)
            AND NOT EXISTS(SELECT 1 FROM member_ecash_execution_claims e WHERE e.reservation_id=r.id AND e.member=?)
            AND NOT EXISTS(SELECT 1 FROM reward_outcome_manifests m WHERE m.reservation_id=r.id)
        """,(row["grant"],row["feed"],row["plan"],row["reserved"],bridge["reward_policy"]["owner"],row["member"],row["member"])).fetchone()
        if check:
            authorized.append(row)
    return authorized


def receipt(db, feed, plan, member):
    row = db.execute("""
        SELECT CAST(m.value AS INTEGER) FROM operator_ecash e JOIN proof_disposition d ON d.id=e.feed
        JOIN json_each(d.binding,'$.failed_members') m
        WHERE e.token IS NOT NULL AND json_extract(d.binding,'$.feed')=?
        AND json_extract(d.binding,'$.plan_hash')=? AND m.key=?
    """,(feed,plan,member)).fetchall()
    if len(row)>1:
        raise ValueError("duplicate failed member disposition")
    return row[0][0] if row else None


def reclaim(store, client, rows, keyset, keys):
    from claim_store import encode
    from operator_ecash import _transfer

    if not rows:
        return []
    feed = str(UUID(rows[0]["feed"]))
    original = store.db.execute("SELECT binding FROM proof_disposition p JOIN operator_ecash e ON e.feed=p.id "
                                "WHERE p.id=? AND p.purpose='operator_transfer' AND e.token IS NOT NULL",(feed,)).fetchone()
    if not original:
        return []
    parent = json.loads(original[0])
    limits = {}
    members = {}
    for row in rows:
        member = row["member"]
        if (row["feed"] != feed or parent["feed"] != feed or "failed_members" in parent
                or row["recipient"] != parent["recipient"] or row["plan"] != parent["plan_hash"]
                or row["reserved"] != parent["member"] or not re.fullmatch("[0-9a-f]{64}",member)
                or member in members or not 0 < row["gross"] <= parent["member"]):
            raise ValueError("failed member allocation differs from original feed")
        members[member] = row["gross"]
        limits[member] = row["fee_limit"]
    results = []
    db_path = Path(store.db.execute("PRAGMA database_list").fetchone()[2])
    with db_path.with_suffix(".member-reclaim.lock").open("ab") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        allocated = 0
        for disposition, raw, ready in store.db.execute("""
            SELECT p.id,p.binding,e.token IS NOT NULL FROM proof_disposition p JOIN operator_ecash e ON e.feed=p.id
            WHERE json_extract(p.binding,'$.feed')=? AND json_type(p.binding,'$.failed_members')='object'
        """,(feed,)).fetchall():
            saved = json.loads(raw)
            batch = saved.pop("failed_members")
            allocated += sum(batch.values())
            for member, gross in batch.items():
                if member in members and members.pop(member) != gross:
                    raise ValueError("failed member retry differs")
            if not ready:
                results.append(_transfer(store,client,saved,keyset,keys,disposition,batch))
        gross = sum(members.values())
        if allocated + gross > parent["member"]:
            raise ValueError("member reclaims exceed original reservation")
        if gross:
            disposition = str(uuid5(UUID(feed), "failed-members:"+encode(members)))
            request = {**parent, "gross":gross, "member":parent["member"]+parent["gross"]-gross,
                       "fee_limit":min(gross,sum(limits[m] for m in members))}
            results.append(_transfer(store,client,request,keyset,keys,disposition,members))
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage",choices=("notices","plan","authorize","seal","issue"))
    stage = parser.parse_args().stage
    if stage == "notices":
        print(json.dumps(notices(connect(MEMBERS))))
        return
    rows = json.load(sys.stdin)
    if stage == "authorize":
        output = authorize(connect(CORE),rows,config("/etc/lightning-goats/config.receive.toml"))
    elif stage in ("plan","seal"):
        seal = stage=="seal"
        pairs = [(r["feed"],r["member"]) for r in rows] if seal else rows
        # Recheck current policy and inputs under the payout journal lock before sealing.
        if seal:
            authorized = {(r["feed"],r["member"]):r for r in rows}
            db = connect(PAYOUT,"rw")
            policy = config("/etc/cyberherd-payout/config.toml")
            with db:
                db.execute("BEGIN IMMEDIATE")
                candidates = qualify(db,pairs,policy)
                output = [r for r in candidates if authorized.get((r["feed"],r["member"])) == r]
                for r in output:
                    db.execute("INSERT INTO payout_unsubmitted VALUES(?,?,?,unixepoch()) "
                               "ON CONFLICT(grant_id,member) DO NOTHING",
                               (r["grant"],r["member"],json.dumps("operator_cancelled")))
        else:
            output = qualify(connect(PAYOUT),pairs,config("/etc/cyberherd-payout/config.toml"))
    else:
        import httpx
        from claim_store import ClaimStore
        keys = json.loads((CONFIG/"keysets.json").read_text())
        store = ClaimStore(COLLECTOR)
        with httpx.Client(base_url=MINT, timeout=15, trust_env=False, follow_redirects=False) as client:
            output = []
            for feed in sorted({r["feed"] for r in rows}):
                try:
                    group = [row for row in rows if row["feed"] == feed]
                    output.extend(reclaim(store, client, group, os.environ["MINT_KEYSET"], keys))
                except (ValueError, httpx.HTTPError):
                    output.append({"state": "held", "reason": "collector_reconciliation"})
    print(json.dumps(output))


if __name__=="__main__":
    main()
