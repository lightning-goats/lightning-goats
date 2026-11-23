"""Run release -> rollback -> release with the same funded, disposable payment journals."""
import argparse
from contextlib import closing, contextmanager
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import socketserver
import sqlite3
import subprocess
import sys
from tempfile import TemporaryDirectory
import threading
import time

import bolt11
import httpx
from mnemonic import Mnemonic

from mint_acceptance import Fixture, PROJECT_TOKEN, start_mint

RESERVE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(RESERVE / "collector"))
from claim_store import ClaimStore
from collect import collect
from invoice_server import handle


def snapshot(db):
    tables = ("settled_payments", "ledger_entries", "feed_attempts", "breez_collected_claims")
    with closing(sqlite3.connect(db.as_uri() + "?mode=ro", uri=True)) as connection:
        connection.execute("BEGIN")
        records = {table: connection.execute(f"SELECT * FROM {table} ORDER BY rowid").fetchall()
                   for table in tables}
        records["invoices"] = connection.execute(
            "SELECT id,intent,invoice,payment_hash FROM breez_project_invoices ORDER BY rowid").fetchall()
        records["payment_events"] = connection.execute(
            "SELECT * FROM event_log WHERE event_type='payment_received' ORDER BY seq").fetchall()
        return records


@contextmanager
def running(binary, config, viewer):
    with subprocess.Popen([binary, "--config", config], env={"PATH": "/usr/bin:/bin", "TOKIO_WORKER_THREADS": "2", "RUST_LOG": "error"}) as process:
        try:
            for _ in range(200):
                if process.poll() is not None:
                    raise RuntimeError("receiver exited before health check")
                try:
                    if viewer.get("/healthz").status_code == 200:
                        break
                except httpx.TransportError:
                    pass
                time.sleep(0.05)
            else:
                raise RuntimeError("fixture receiver unavailable")
            yield
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    raise
            if process.returncode:
                raise RuntimeError("fixture receiver exited unsuccessfully")


def wait_for_credit(viewer, expected):
    for _ in range(400):
        if viewer.get("/api/v1/status").raise_for_status().json()["feed_credit_sats"] == expected:
            return
        time.sleep(0.1)
    raise RuntimeError("fixture credit did not settle")


def rehearse(root, release, rollback):
    db = root / "core.db"
    collector_socket = root / "collector.sock"
    claim_dir = root / "claims"
    claim_dir.mkdir(mode=0o700)
    config = root / "receiver.toml"
    text = (RESERVE.parent / "config.canary.toml.example").read_text()
    text = text.replace('mode = "canary"', 'mode = "shadow"').replace(
        "/var/lib/lightning-goats/lightning-goats-canary.db", str(db)).replace(
        "http://10.8.0.6:8790/", "http://127.0.0.1:9/").replace(
        "/run/lightning-goats-canary-project-invoices.sock", str(collector_socket)).replace(
        "[lnurl.collector]", 'wallet_backend = "breez"\n[lnurl.breez_collector]').replace(
        "peer_uid = 1001", f"peer_uid = {os.getuid()}")
    config.write_text(text)
    (root / "mint-seed").write_text(Mnemonic("english").generate(strength=128))
    (root / "project-quote-token").write_text(PROJECT_TOKEN)
    with Fixture(root / "reserve.sock") as reserve, httpx.Client(base_url="http://127.0.0.1:8788", trust_env=False) as viewer:
        threading.Thread(target=reserve.serve_forever, daemon=True).start()
        mint, client = start_mint(root, root / "mint.sock", root / "reserve.sock", root)
        try:
            keys = client.get("/v1/keys").raise_for_status().json()["keysets"][0]
            class Handler(socketserver.BaseRequestHandler):
                def handle(self):
                    store = ClaimStore(claim_dir)
                    try:
                        handle(self.request, os.getuid(), store, client, PROJECT_TOKEN, keys["id"])
                    finally:
                        store.db.close()

            def pay(sats):
                pr = viewer.get("/lnurlp/dexter/callback", params={"amount": sats * 1000}).raise_for_status().json()["pr"]
                reserve.paid.add(bolt11.decode(pr).payment_hash)
                store = ClaimStore(claim_dir)
                try:
                    (request_id,) = store.db.execute("SELECT id FROM project_claim WHERE invoice=?", (pr,)).fetchone()
                    for _ in range(100):
                        if collect(store, client, request_id, {keys["id"]: keys["keys"]}):
                            return
                        time.sleep(0.1)
                    raise RuntimeError("mint failed to issue fixture proofs")
                finally:
                    store.db.close()

            with socketserver.UnixStreamServer(str(collector_socket), Handler) as server:
                threading.Thread(target=server.serve_forever, daemon=True).start()
                with running(release, config, viewer):
                    pay(7)
                    wait_for_credit(viewer, 7)
                original = snapshot(db)
                assert len(original["settled_payments"]) == len(original["payment_events"]) == 1
                with running(rollback, config, viewer):
                    wait_for_credit(viewer, 7)
                    assert snapshot(db) == original
                    pay(11)
                    wait_for_credit(viewer, 18)
                rolled_back = snapshot(db)
                for table, rows in original.items():
                    assert rolled_back[table][:len(rows)] == rows
                with running(release, config, viewer):
                    wait_for_credit(viewer, 18)
                    assert snapshot(db) == rolled_back
                assert len(rolled_back["payment_events"]) == len(rolled_back["breez_collected_claims"]) == 2
                assert not rolled_back["feed_attempts"]
                store = ClaimStore(claim_dir)
                try:
                    assert store.db.execute("SELECT sum(amount_sat) FROM collected_output").fetchone() == (18,)
                finally:
                    store.db.close()
                assert len(reserve.receives) == 2
                server.shutdown()
        finally:
            client.close()
            mint.terminate()
            mint.wait(timeout=15)
    print(json.dumps({"release_sha256": hashlib.sha256(release.read_bytes()).hexdigest(),
                      "rollback_sha256": hashlib.sha256(rollback.read_bytes()).hexdigest(),
                      "collector_sats": 18, "feeder_credit_sats": 18, "payment_events": 2, "feed_attempts": 0}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("release", type=Path)
    parser.add_argument("rollback", type=Path)
    args = parser.parse_args()
    if os.getuid() == 0 or {name for _, name in socket.if_nameindex()} != {"lo"}:
        parser.error("use a non-root user in a fresh network namespace with only loopback up")
    with TemporaryDirectory(prefix="goats-roll-") as directory:
        rehearse(Path(directory), args.release.resolve(), args.rollback.resolve())
