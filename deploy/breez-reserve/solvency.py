#!/usr/bin/env python3
"""Read local mint liability and reserve counters without exposing payment records."""

import argparse
import json
import sqlite3
from datetime import datetime, timezone
from pathlib import Path

import httpx


def totals(file):
    with sqlite3.connect(file.resolve().as_uri() + "?mode=ro", uri=True) as conn:
        conn.execute("BEGIN")
        outstanding, input_fees = conn.execute("SELECT COALESCE(SUM(balance), 0), COALESCE(SUM(fees_paid), 0) FROM keysets WHERE unit='sat'").fetchone()
        mint = dict(conn.execute("SELECT state, SUM(amount) FROM mint_quotes WHERE unit='sat' GROUP BY state"))
        pending_melts, pending_fee_reserve, lightning_fees = conn.execute("""
            SELECT COALESCE(SUM(CASE WHEN state='PENDING' THEN amount END), 0),
                COALESCE(SUM(CASE WHEN state='PENDING' THEN fee_reserve END), 0),
                COALESCE(SUM(CASE WHEN state='PAID' THEN fee_paid END), 0)
            FROM melt_quotes WHERE unit='sat'
        """).fetchone()
    return {"issued_liability_sat": outstanding, "mint_input_fees_sat": input_fees,
            "mint_unpaid_face_value_sat": mint.get("UNPAID", 0),
            "mint_paid_unissued_sat": mint.get("PAID", 0),
            "mint_pending_face_value_sat": mint.get("PENDING", 0),
            "pending_melt_principal_sat": pending_melts,
            "pending_melt_fee_reserve_sat": pending_fee_reserve,
            "settled_lightning_melt_fees_sat": lightning_fees}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mint_sqlite", type=Path)
    parser.add_argument("funding_socket", type=Path)
    parser.add_argument("--expected-wallet", required=True)
    args = parser.parse_args()
    with httpx.Client(transport=httpx.HTTPTransport(uds=str(args.funding_socket)), timeout=8) as client:
        balance = client.get("http://reserve/v1/funding/balance").raise_for_status().json()
    if balance["wallet"] != args.expected_wallet:
        raise SystemExit("reserve identity mismatch")
    print(json.dumps({"collected_at": datetime.now(timezone.utc).isoformat(),
        "reserve_balance_sat": balance["balance_sat"], "reserve_synced_at": balance["last_synced_at"],
        **totals(args.mint_sqlite)}, indent=2))


if __name__ == "__main__":
    main()
