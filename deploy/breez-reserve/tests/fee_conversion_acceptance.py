"""Exercise atomic conversion with real pinned issuer storage and blinded signatures."""
import asyncio
import hashlib
import os
import tempfile
import sys
import time
from pathlib import Path
from uuid import uuid4

from coincurve import PrivateKey
from mnemonic import Mnemonic


class Reserve:
    def __init__(self):
        self.synced = int(time.time())

    async def _call(self, method, path):
        assert (method, path) == ("GET", "/v1/funding/balance")
        return {"wallet": "fixture", "balance_sat": 100, "last_synced_at": self.synced}

    def _verify_wallet(self, reply):
        assert reply["wallet"] == "fixture"


async def scenario(directory):
    os.environ.update(MINT_DATABASE=str(directory / "db"), MINT_AUTH_DATABASE=str(directory / "auth"), CASHU_DIR=str(directory),
                      MINT_MNEMONIC=Mnemonic("english").generate(), MINT_BACKEND_BOLT11_SAT="FakeWallet",
                      MINT_WATCHDOG_ENABLED="false", MINT_RPC_SERVER_ENABLE="false")
    from cashu.core.migrations import migrate_databases
    from cashu.mint.startup import ledger, mint_migrations
    from goats_fee_conversion import issue_conversion, recover_material, recover_token
    from project_quote import prepare_outputs

    await migrate_databases(ledger.db, mint_migrations)
    await ledger._startup_keysets()
    keyset = ledger.keyset
    async with ledger.db.connect() as conn:
        await ledger.crud.bump_keyset_fees_paid(keyset=keyset, amount=9, db=ledger.db, conn=conn)
    key = PrivateKey().public_key.format().hex()
    reserve = Reserve()
    from cashu.core.base import Unit
    from cashu.lightning.goats_watchdog import check_sample

    await check_sample(ledger, reserve, Unit.sat)
    identifier = str(uuid4())
    outputs = prepare_outputs(7, keyset.id, recipient=key)

    async def issue():
        return await issue_conversion(ledger, reserve, identifier, outputs, key, 5)

    reserve.synced -= 61
    try:
        await issue()
    except ValueError as exc:
        assert "fresh reserve" in str(exc)
    else:
        raise AssertionError("stale reserve accepted")
    reserve.synced = int(time.time())
    from cashu.core.settings import settings
    settings.mint_max_balance = 6
    try:
        await issue()
    except ValueError as exc:
        assert "liability cap" in str(exc)
    else:
        raise AssertionError("mint liability cap exceeded")
    settings.mint_max_balance = 7
    first, retry = await asyncio.gather(issue(), issue())
    assert [p.C_ for p in first] == [p.C_ for p in retry]
    assert sum(p.amount for p in retry) == 7
    async with ledger.db.connect() as conn:
        row = await conn.fetchone("SELECT COUNT(*) AS n FROM goats_fee_conversions")
        assert row["n"] == 1
        assert await recover_material(conn, identifier) == outputs
        row = await conn.fetchone("SELECT balance,fees_paid FROM keysets WHERE id=:id", {"id":keyset.id})
        assert (row["balance"],row["fees_paid"]) == (7,9)
    await check_sample(ledger, reserve, Unit.sat)
    async with ledger.db.connect() as conn:
        rows = await conn.fetchall("""
            SELECT backend_balance,keyset_balance,keyset_fees_paid FROM balance_log
            WHERE unit='sat' ORDER BY rowid
        """)
        assert [(r["backend_balance"], r["keyset_balance"], r["keyset_fees_paid"])
                for r in rows] == [(100, 0, 9), (100, 7, 2)]
    token = await recover_token(ledger, identifier)
    assert token.startswith("cashuA") and token == await recover_token(ledger, identifier)
    # A separate interpreter has only the original issuer credentials and disk journal.
    expected = hashlib.sha256(token.encode()).hexdigest()
    child = await asyncio.create_subprocess_exec(sys.executable, __file__, "recover", identifier, expected)
    if await child.wait() != 0:
        raise AssertionError("persisted token recovery failed")
    reserve.synced = int(time.time())
    settings.mint_max_balance = 0
    # Seven of nine fee sats were converted. The remaining two cannot fund three.
    try:
        await issue_conversion(ledger, reserve, str(uuid4()), prepare_outputs(3, keyset.id, recipient=key),
                               key, 5)
    except ValueError as exc:
        assert "insufficient reconciled" in str(exc)
    else:
        raise AssertionError("converted fees spent twice")
    print("Fee conversion: stale sample held, concurrent exact retry, seven sats recovered after restart")


async def recover(identifier, expected):
    from cashu.mint.startup import ledger
    from goats_fee_conversion import recover_token

    await ledger._startup_keysets()
    token = await recover_token(ledger, identifier)
    assert hashlib.sha256(token.encode()).hexdigest() == expected


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "recover":
        asyncio.run(recover(*sys.argv[2:]))
    else:
        with tempfile.TemporaryDirectory() as tmp:
            asyncio.run(scenario(Path(tmp)))
