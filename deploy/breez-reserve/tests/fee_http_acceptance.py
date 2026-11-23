"""Exercise the private fee endpoint and its original-token recovery after key rotation."""
import base64
import json
import os
import shutil
from functools import partial

import bech32
import cbor2
import sqlite3
import threading
from pathlib import Path
from tempfile import TemporaryDirectory
from uuid import uuid4

from coincurve import PrivateKey
from mnemonic import Mnemonic
from mint_acceptance import Fixture, PROJECT_TOKEN, expect, start_mint
from project_quote import prepare_outputs
from fee_worker import tick
from operator_delivery import deliver_token


def scenario(home):
    cred, state = home / "creds", home / "state"
    cred.mkdir()
    state.mkdir()
    (cred / "mint-seed").write_text(Mnemonic("english").generate())
    (cred / "project-quote-token").write_text(PROJECT_TOKEN)
    (cred / "operator-fee-token").write_text("disposable-fee-" + "a" * 32)
    headers = {"Authorization": "Bearer " + (cred / "operator-fee-token").read_text()}
    key = PrivateKey().public_key.format().hex()
    os.environ.update(LG_OPERATOR_FEE_RECIPIENT=key, LG_OPERATOR_FEE_MAX_SAT="7", LG_OPERATOR_FEE_BUFFER_SAT="5")
    route = "/private/v1/operator-fee-conversions"
    identity = str(uuid4())
    with Fixture(home / "reserve.sock") as reserve:
        threading.Thread(target=reserve.serve_forever, daemon=True).start()
        def start():
            return start_mint(state, home / "mint.sock", home / "reserve.sock", cred, startup_timeout=240)
        proc, client = start()
        try:
            keyset = expect(client.get("/v1/keys"))["keysets"][0]["id"]
            with sqlite3.connect(next(state.rglob("mint.sqlite3"))) as conn:
                conn.execute("UPDATE keysets SET fees_paid=7 WHERE id=?", (keyset,))
            body = {"request_id": identity, "outputs": prepare_outputs(7, keyset, recipient=key)}
            assert client.post(route, json=body).status_code == 401
            relay = b"wss://relay.example"
            profile = bytes([0,32]) + bytes.fromhex(key[2:]) + bytes([1,len(relay)]) + relay
            address = bech32.bech32_encode("nprofile", bech32.convertbits(profile,8,5,True))
            request = "creqA" + base64.urlsafe_b64encode(cbor2.dumps({
                "i":"fixture","u":"sat","nut10":{"k":"P2PK","d":key},
                "t":[{"t":"nostr","a":address,"g":[["n","17"]]}]})).decode()
            class LostReply:
                get = client.get
                def post(self, *args, **kwargs):
                    expect(client.post(*args, **kwargs))
                    raise TimeoutError("synthetic lost response after commit")
            token_secret = (cred / "operator-fee-token").read_text()
            def run_worker(transport, send):
                with sqlite3.connect(home / "vps-worker.sqlite") as db:
                    tick(db, transport, token_secret, request, keyset, 7, "2026-11-22",
                         partial(deliver_token, wrapper=home/"operator_wrap.cjs", send=send))
            try:
                run_worker(LostReply(), None)
            except TimeoutError:
                pass
            else:
                raise AssertionError("lost reply was not exercised")
            with sqlite3.connect(home/"vps-worker.sqlite") as db:
                identity, = db.execute("SELECT id FROM fee_batches").fetchone()
            result = expect(client.get(route+"/"+identity,headers=headers))
            assert result["id"] == identity and result["token"].startswith("cashuA")
            proc.terminate()
            proc.wait(timeout=15)
            client.close()
            os.environ["LG_OPERATOR_FEE_RECIPIENT"] = PrivateKey().public_key.format().hex()
            proc, client = start()
            assert expect(client.get(route + "/" + identity, headers=headers)) == result
            assert client.get(route + "/" + identity).status_code == 401
            root = Path(__file__).resolve().parents[3]
            shutil.copy(root/"deploy/breez-reserve/collector/operator_wrap.cjs",home)
            shutil.copy(root/"web/vendor/nostr.bundle.js",home)
            observed = []
            def send(url,event):
                observed.append(event)
                return len(observed) == 2
            for _ in range(3):
                run_worker(client,send)
            assert len(observed) == 2 and observed[0] == observed[1]
            with sqlite3.connect(home/"vps-worker.sqlite") as db:
                assert db.execute("SELECT count(*),sum(delivered) FROM fee_batches").fetchone() == (1,1)
            assert reserve.pay_attempts == 0
        finally:
            proc.terminate()
            proc.wait(timeout=15)
            client.close()


if __name__ == "__main__":
    with TemporaryDirectory() as tmp:
        scenario(Path(tmp))
    print("Private fee endpoint: authorized issuance, offline recovery after key rotation, zero Lightning sends")
