"""Attribute a drop in the reserve surplus to receipts observed before issuance."""

import asyncio
import time

from loguru import logger
from cashu.core.db import LockOptions
from cashu.core.settings import settings


async def check_sample(ledger, backend, unit):
    db = ledger.watcher_db
    async with db.connect(locks=[LockOptions(table="balance_log")]) as conn:
        # The issuer commits signatures, liability and ISSUED in one transaction.
        # Take the reserve sample while that transaction cannot cross our checkpoint.
        sampled_at = int(time.time())
        status = await backend.status()
        if status.error_message:
            raise RuntimeError("reserve balance unavailable")
        reserve = status.balance
        issued, fees = await ledger.get_unit_balance_and_fees(unit, db, conn)
        gap = reserve.amount - issued.amount - fees.amount
        if gap < 0:
            raise RuntimeError("reserve is below issued liability and fees")

        await conn.execute("""
            CREATE TABLE IF NOT EXISTS goats_watchdog_open_quotes (
                quote TEXT PRIMARY KEY, checking_id TEXT NOT NULL UNIQUE,
                amount INTEGER NOT NULL, sampled_at INTEGER NOT NULL
            )
        """)
        # rowid breaks ties when two samples share the same Unix second.
        previous = await conn.fetchone("""
            SELECT backend_balance, keyset_balance, keyset_fees_paid
            FROM balance_log WHERE unit = :unit ORDER BY time DESC, rowid DESC LIMIT 1
        """, {"unit": unit.name})
        if previous:
            old_gap = (previous["backend_balance"] - previous["keyset_balance"]
                       - previous["keyset_fees_paid"])
            if gap < old_gap:
                if reserve.amount < previous["backend_balance"]:
                    raise RuntimeError("reserve balance and surplus both decreased")
                claims = await conn.fetchall("""
                    SELECT p.checking_id, p.amount, p.sampled_at
                    FROM goats_watchdog_open_quotes p JOIN mint_quotes q ON q.quote = p.quote
                    WHERE q.state = 'ISSUED' AND q.unit = :unit AND q.checking_id = p.checking_id
                      AND q.amount_issued = p.amount AND q.amount = p.amount
                """, {"unit": unit.name})
                explained = 0
                for claim in claims:
                    receipt = await backend._call("GET", "/v1/invoices/" + claim["checking_id"])
                    if (receipt.get("state") == "settled"
                            and receipt.get("payment_hash") == claim["checking_id"]
                            and receipt.get("amount_sat") == claim["amount"]
                            and 0 < receipt["settled_at"] <= claim["sampled_at"]):
                        explained += claim["amount"]
                if explained < old_gap - gap:
                    raise RuntimeError("decrease in reserve surplus lacks settled quote evidence")

        await ledger.crud.store_balance_log(reserve, issued, fees, db=db, conn=conn)
        await conn.execute("DELETE FROM goats_watchdog_open_quotes")
        if gap:
            # Open quotes grant no credit. Only a later ISSUED transition plus the
            # original verified Spark receipt can explain a decrease at the next check.
            await conn.execute("""
                INSERT INTO goats_watchdog_open_quotes (quote, checking_id, amount, sampled_at)
                SELECT quote, checking_id, amount, :at FROM mint_quotes
                WHERE unit = :unit AND state IN ('UNPAID', 'PAID', 'PENDING')
            """, {"at": sampled_at, "unit": unit.name})


async def monitor(ledger, backend, unit):
    while True:
        try:
            await check_sample(ledger, backend, unit)
        except Exception as error:
            logger.error("Reserve watchdog could not accept checkpoint ({})", type(error).__name__)
            if isinstance(error, RuntimeError):
                logger.error("Reserve watchdog: {}", error)
            await ledger.abort_queue.put(True)
        await asyncio.sleep(settings.mint_watchdog_balance_check_interval_seconds)
