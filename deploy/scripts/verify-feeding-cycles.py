#!/usr/bin/env python3
"""Read-only accounting verification for a schema-41 feeding-cycle journal."""
import argparse
import json
from itertools import zip_longest
from pathlib import Path
import sqlite3


def require(condition, message):
    if not condition:
        raise ValueError(message)


def target(rate):
    whole, _, fraction = rate.partition(".")
    scale = 10 ** len(fraction)
    numerator = int(whole) * scale + int(fraction or "0")
    require(scale <= numerator <= 10_000_000 * scale, "rate outside pricing bounds")
    return (100_000_000 * scale + numerator - 1) // numerator


def verify(db):
    db.row_factory = sqlite3.Row
    if not db.in_transaction:
        db.execute("BEGIN")
    def scalar(query):
        return db.execute(query).fetchone()[0]

    require(not db.execute("PRAGMA foreign_key_check").fetchone(), "invalid foreign keys")
    policy = db.execute("SELECT * FROM feed_cycle_policy WHERE singleton=1").fetchone()
    require(policy is not None, "funding cycle policy has not been initialized")
    cycles = db.execute("""
        SELECT c.*, COALESCE(SUM(a.sats),0) AS allocated
        FROM feed_cycles c LEFT JOIN feed_cycle_allocations a ON a.cycle_id=c.id
        GROUP BY c.id ORDER BY c.seq
    """).fetchall()
    pending = scalar("SELECT COALESCE(SUM(sats),0) FROM feed_cycle_pending")
    held = 0
    funded = 0
    confirmations = 0
    open_goal = None
    for cycle in cycles:
        require(cycle["credited_sats"] == cycle["allocated"], "cycle allocation sum differs")
        if cycle["source"] == "OpenHAB BTC_USD_Price":
            require(cycle["target_sats"] == target(cycle["rate_usd"]), "locked target differs from rate")
            age = cycle["opened_at"] * 1000 - cycle["observed_at_ms"]
            # opened_at is recorded to seconds, while lastStateUpdate is milliseconds.
            require(-30_999 <= age <= 300_000, "cycle opened with an observation outside its age bounds")
        else:
            require(cycle["source"] == "legacy", "unexpected source")
        if cycle["confirmed_attempt"] is not None:
            evidence = db.execute("""
                SELECT a.status,a.threshold_sats,l.delta_sats,b.cycle_id
                FROM feed_attempts a JOIN ledger_entries l ON l.feed_attempt_id=a.id
                JOIN feed_cycle_attempts b ON b.attempt_id=a.id
                WHERE a.id=? AND l.entry_type='FEED_DEBIT'
            """, (cycle["confirmed_attempt"],)).fetchall()
            require(len(evidence) == 1, "cycle lacks exactly one physical debit")
            row = evidence[0]
            require(row["status"] == "confirmed" and row["cycle_id"] == cycle["id"]
                    and row["threshold_sats"] == cycle["target_sats"]
                    and row["delta_sats"] == -cycle["target_sats"], "cycle and physical debit differ")
            confirmations += 1
        else:
            held += cycle["allocated"]
            funded += int(cycle["funded_at"] is not None)
        if cycle["funded_at"] is None:
            require(open_goal is None, "more than one open cycle")
            open_goal = {k: cycle[k] for k in ("id", "target_sats", "source", "rate_usd",
                                              "observed_at_ms", "opened_at", "credited_sats")}
            open_goal["remaining_sats"] = cycle["target_sats"] - cycle["credited_sats"]
    require(held + pending == scalar("SELECT COALESCE(SUM(delta_sats),0) FROM ledger_entries"),
            "cycle funding and pending credit differ from the ledger")
    unmatched_debits = db.execute("""
        SELECT COUNT(*) FROM ledger_entries l LEFT JOIN feed_cycles c ON c.confirmed_attempt=l.feed_attempt_id
        WHERE l.id>? AND l.entry_type='FEED_DEBIT' AND c.id IS NULL
    """, (policy["legacy_through_ledger_id"],)).fetchone()[0]
    require(unmatched_debits == 0, "post-migration physical debit lacks a cycle")
    mismatch = db.execute("""
        WITH allocation AS (SELECT ledger_id,SUM(sats) AS sats,COUNT(*) AS cycles
                           FROM feed_cycle_allocations GROUP BY ledger_id),
             retired AS (SELECT credit_ledger_id AS ledger_id,SUM(retired_sats) AS sats
                         FROM feed_credit_retirements GROUP BY credit_ledger_id)
        SELECT COUNT(*) FROM ledger_entries l
        LEFT JOIN allocation a ON a.ledger_id=l.id
        LEFT JOIN feed_cycle_pending p ON p.ledger_id=l.id
        LEFT JOIN retired r ON r.ledger_id=l.id
        WHERE l.id>? AND l.delta_sats>0 AND (
          l.delta_sats<>COALESCE(a.sats,0)+COALESCE(p.sats,0)+COALESCE(r.sats,0)
          OR COALESCE(a.cycles,0)>3 OR (COALESCE(a.cycles,0)=3 AND p.ledger_id IS NOT NULL))
    """, (policy["legacy_through_ledger_id"],)).fetchone()[0]
    require(mismatch == 0, "post-migration receipt cap or allocation does not reconcile")
    unmatched = scalar("""
        SELECT COUNT(*) FROM feed_cycle_plans b JOIN feed_financial_plans p ON p.id=b.plan_id
        JOIN feed_cycles c ON c.id=b.cycle_id
        WHERE p.threshold_sats<>c.target_sats OR (p.request_id IS NOT NULL AND NOT EXISTS(
            SELECT 1 FROM feed_cycle_attempts a WHERE a.attempt_id=p.request_id AND a.cycle_id=b.cycle_id))
    """)
    require(unmatched == 0, "financial reservation is not bound to its cycle target")
    return {"current_cycle": open_goal, "funded_count": funded, "unallocated_sats": pending,
            "confirmed_cycle_debits": confirmations, "feed_credit_sats": held + pending}


def compare_paid_state(before, after):
    # Those three projections can advance while initializing the policy. The
    # ledger balance and the initialized cycle journal are verified separately.
    derived = {"_sqlx_migrations", "reward_funding_state", "reward_funding_lots"}
    baseline = before.execute("SELECT MAX(version) FROM _sqlx_migrations").fetchone()[0]
    selected = after.execute("SELECT MAX(version) FROM _sqlx_migrations").fetchone()[0]
    require((baseline, selected) == (40, 41), "expected schema 40 and 41 for paid-state comparison")
    history = "SELECT version,description,installed_on,success,checksum,execution_time FROM _sqlx_migrations"
    require(before.execute(history + " ORDER BY version").fetchall()
            == after.execute(history + " WHERE version<=40 ORDER BY version").fetchall(),
            "original migration history changed")
    tables = before.execute("SELECT name FROM sqlite_schema WHERE type='table' AND (name NOT LIKE 'sqlite_%' OR name='sqlite_sequence')")
    for (table,) in tables:
        if table in derived:
            continue
        identifier = '"' + table.replace('"', '""') + '"'
        query = f"SELECT _rowid_, * FROM {identifier} ORDER BY _rowid_"
        require(all(a == b for a, b in zip_longest(before.execute(query), after.execute(query))),
                f"retained rows changed in {table}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path, required=True)
    parser.add_argument("--before", type=Path, help="separate schema-40 backup, for initialization checks")
    args = parser.parse_args()
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        if args.before is not None:
            connection.execute("BEGIN")
            with sqlite3.connect(args.before.resolve().as_uri() + "?mode=ro", uri=True) as prior:
                prior.execute("BEGIN")
                compare_paid_state(prior, connection)
        report = verify(connection)
        if args.before is not None:
            report["compared_original_paid_state"] = True
        print(json.dumps(report, indent=2))
