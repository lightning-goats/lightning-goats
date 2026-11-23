"""Compare batched claim receipts to single-quote recovery from the same journal."""
import hashlib
import sys
import threading
import time
from pathlib import Path
from tempfile import TemporaryDirectory
from uuid import uuid4

import bolt11
import httpx
from mnemonic import Mnemonic

from mint_acceptance import Fixture, PROJECT_TOKEN, start_mint

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "collector"))
from claim_store import ClaimStore
from collect import collect
from project_quote import issue
from worker import QuotePoller


def run(directory, client, reserve):
    keyset = client.get("/v1/keys").raise_for_status().json()["keysets"][0]
    keys = {keyset["id"]: keyset["keys"]}
    wallet = directory / "project"
    replay = directory / "replay"
    wallet.mkdir(mode=0o700)
    replay.mkdir(mode=0o700)
    store = ClaimStore(wallet)
    ids = [str(uuid4()) for _ in range(3)]
    for i, request in enumerate(ids, start=1):
        invoice = issue(store, client, PROJECT_TOKEN, request, i,
                        hashlib.sha256(request.encode()).hexdigest(), keyset["id"],
                        {"user": "dexter", "credit_pool": "herd"})
        if i > 1:
            reserve.paid.add(bolt11.decode(invoice).payment_hash)

    class LostReply:
        get = client.get

        def post(self, path, **kwargs):
            assert path == "/v1/mint/bolt11"
            client.post(path, **kwargs).raise_for_status()
            raise httpx.ReadTimeout("issued but reply lost")

    for _ in range(30):
        try:
            collect(store, LostReply(), ids[-1], keys)
        except httpx.ReadTimeout:
            break
        time.sleep(.1)
    else:
        raise AssertionError("failed to reach lost issuance response")
    store.db.close()
    store = ClaimStore(wallet)
    comparison = ClaimStore(replay)
    store.db.backup(comparison.db)
    paths = []
    client.event_hooks["request"].append(lambda request: paths.append(request.url.path))
    assert QuotePoller().poll(store, client, keys, int(time.time()))
    assert paths.count("/v1/mint/quote/bolt11/check") == 1, paths
    assert not any(p.startswith("/v1/mint/quote/bolt11/goats-") for p in paths)
    assert paths.count("/v1/restore") == 1
    assert paths.count("/v1/mint/bolt11") == 1
    paths.clear()
    for request in ids:
        collect(comparison, client, request, keys)
    assert sum(p.startswith("/v1/mint/quote/bolt11/goats-") for p in paths) == 3
    query = "SELECT id, payment_hash, proofs FROM project_claim ORDER BY id"
    assert store.db.execute(query).fetchall() == comparison.db.execute(query).fetchall()
    assert store.db.execute("SELECT sum(amount_sat) FROM collected_output").fetchone() == (5,)
    print("Mixed batch: 1 status call, individual: 3, identical receipts, 5 acquired fixture sats")


if __name__ == "__main__":
    with TemporaryDirectory(prefix="lg-batch-") as name:
        directory = Path(name)
        (directory / "mint-seed").write_text(Mnemonic("english").generate(strength=128))
        (directory / "project-quote-token").write_text(PROJECT_TOKEN)
        with Fixture(directory / "reserve.sock") as reserve:
            threading.Thread(target=reserve.serve_forever, daemon=True).start()
            process, client = start_mint(directory, directory / "mint.sock",
                                         directory / "reserve.sock", directory)
            try:
                run(directory, client, reserve)
            finally:
                client.close()
                process.terminate()
                process.wait(timeout=15)
