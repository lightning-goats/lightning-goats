"""Exercise the signing and watchdog processes with controlled reserve observations."""

import sqlite3
import threading
import time
from pathlib import Path
from tempfile import TemporaryDirectory

import bolt11
from mnemonic import Mnemonic

from mint_acceptance import (
    Fixture, PROJECT_TOKEN, encode_outputs, expect, new_outputs, start_mint,
)


def wait_for_sample(db, balance, issued):
    for _ in range(80):
        with sqlite3.connect(db) as conn:
            row = conn.execute(
                "SELECT backend_balance,keyset_balance FROM balance_log ORDER BY rowid DESC LIMIT 1"
            ).fetchone()
        if row == (balance, issued):
            return
        time.sleep(.1)
    raise AssertionError(f"Missing checkpoint for reserve {balance}, liability {issued}")


def scenario(home, case):
    cred, state = home / "creds", home / "state"
    cred.mkdir(parents=True)
    state.mkdir()
    (cred / "mint-seed").write_text(Mnemonic("english").generate(strength=128))
    (cred / "project-quote-token").write_text(PROJECT_TOKEN)
    with Fixture(home / "reserve.sock") as reserve:
        reserve.balance = 132
        threading.Thread(target=reserve.serve_forever, daemon=True).start()

        def start():
            return start_mint(state, home / "mint.sock", home / "reserve.sock", cred, watchdog=True)

        proc, viewer = start()
        try:
            db = next(state.rglob("mint.sqlite3"))
            wait_for_sample(db, 132, 0)
            keyset = expect(viewer.get("/v1/keys"))["keysets"][0]["id"]
            quote = expect(viewer.post("/v1/mint/quote/bolt11", json={"unit": "sat", "amount": 53000}))
            h = bolt11.decode(quote["request"]).payment_hash
            reserve.paid.add(h)
            reserve.settlements[h] = {
                "settled_at": int(time.time()) + (60 if case == "late-receipt" else -1),
                "amount_sat": 1 if case == "wrong-amount" else 53000,
            }
            reserve.balance += 53000
            wait_for_sample(db, 53132, 0)
            proc.terminate()
            proc.wait(timeout=8)
            viewer.close()
            proc, viewer = start()
            amounts = [1 << bit for bit in range(16) if 53000 & (1 << bit)]
            outs, _ = new_outputs(keyset, amounts)
            expect(viewer.post("/v1/mint/bolt11", json={"quote": quote["quote"], "outputs": encode_outputs(outs)}))
            if case in ("loss", "deficit"):
                wait_for_sample(db, 53132, 53000)
                expect(viewer.get("/v1/info"))
                reserve.balance = 53131 if case == "loss" else 52999
            proc.wait(timeout=8)
            assert "Shutting down the mint due to balance mismatch" in (state / "process.log").read_text()
        finally:
            if proc.poll() is None:
                proc.terminate()
                proc.wait(timeout=8)
            viewer.close()


if __name__ == "__main__":
    with TemporaryDirectory() as root:
        for case in ("loss", "deficit", "wrong-amount", "late-receipt"):
            scenario(Path(root) / case, case)
    print("Verified delayed claim, durable restart, mismatched receipt rejection and real reserve loss shutdown")
