"""Deliver one reconciled fee batch per UTC day through the operator wallet request."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sqlite3
import ssl
from uuid import uuid4

import httpx
from invoice_server import home_tls_name
from operator_delivery import deliver_token, destination
from project_quote import prepare_outputs


def tick(db, client, credential, request, keyset, batch, day, deliver=deliver_token):
    if type(batch) is not int or not 1 <= batch <= 1_000_000:
        raise ValueError("invalid fee batch")
    db.execute("""CREATE TABLE IF NOT EXISTS fee_batches(
        day TEXT PRIMARY KEY, id TEXT NOT NULL UNIQUE, request TEXT NOT NULL,
        amount INTEGER NOT NULL, outputs TEXT NOT NULL, delivered INTEGER NOT NULL DEFAULT 0)""")
    with db:
        db.execute("BEGIN IMMEDIATE")
        row = db.execute("SELECT id,request,amount,outputs FROM fee_batches WHERE delivered=0 ORDER BY day LIMIT 1").fetchone()
        if row is None:
            if db.execute("SELECT 1 FROM fee_batches WHERE day=?", (day,)).fetchone():
                return
            _, recipient, _, _ = destination(request)
            row = (str(uuid4()), request, batch, json.dumps(prepare_outputs(batch, keyset, recipient=recipient)))
            db.execute("INSERT INTO fee_batches(day,id,request,amount,outputs) VALUES(?,?,?,?,?)", (day,*row))
    identity, frozen_request, amount, outputs = row
    route = "/private/v1/operator-fee-conversions"
    headers = {"Authorization": "Bearer " + credential}
    response = client.get(route + "/" + identity, headers=headers)
    if response.status_code == 404:
        response = client.post(route, headers=headers, json={"request_id":identity,"outputs":json.loads(outputs)})
    if response.status_code == 409:
        return
    payout = response.raise_for_status().json()
    if payout["id"] != identity:
        raise ValueError("fee conversion identity changed")
    _, recipient, _, _ = destination(frozen_request)
    if deliver(db, "mint-fee:" + identity, payout["token"], amount, recipient, frozen_request):
        with db:
            db.execute("UPDATE fee_batches SET delivered=1 WHERE id=?", (identity,))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batch-sat", type=int, required=True)
    parser.add_argument("--keyset", required=True)
    parser.add_argument("--request", type=Path, required=True)
    args = parser.parse_args()
    credentials = Path(os.environ["CREDENTIALS_DIRECTORY"])
    tls = ssl.create_default_context(cafile=str(credentials / "home-ca.pem"))
    tls.load_cert_chain(credentials / "client.pem", credentials / "client.key")
    secret = (credentials / "operator-fee-token").read_text().strip()
    with httpx.Client(base_url="https://10.8.0.6:8793", verify=tls, timeout=15, trust_env=False,
                      headers={"Host":"mint-home.internal:8793"}, event_hooks={"request":[home_tls_name]}) as client:
        db = sqlite3.connect(Path(os.environ["STATE_DIRECTORY"]) / "fee-batches.sqlite")
        try:
            tick(db, client, secret, args.request.read_text().strip(), args.keyset, args.batch_sat,
                 datetime.now(timezone.utc).date().isoformat())
        finally:
            db.close()


if __name__ == "__main__":
    main()
