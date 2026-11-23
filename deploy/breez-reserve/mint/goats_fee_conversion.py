"""Private issuer journal. Entries commit in the transaction that signs the outputs."""


async def ensure_journal(conn):
    await conn.execute("""
        CREATE TABLE IF NOT EXISTS goats_fee_conversions (
            conversion_id TEXT PRIMARY KEY NOT NULL,
            recipient_key TEXT NOT NULL,
            request_digest TEXT NOT NULL UNIQUE,
            amount_sat INTEGER NOT NULL CHECK(amount_sat > 0),
            reserve_sat INTEGER NOT NULL CHECK(reserve_sat >= amount_sat),
            synced_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL CHECK(created_at >= synced_at)
        )
    """)
    await conn.execute("""
        CREATE TABLE IF NOT EXISTS goats_fee_outputs (
            blinded TEXT PRIMARY KEY NOT NULL REFERENCES promises(b_),
            conversion_id TEXT NOT NULL REFERENCES goats_fee_conversions(conversion_id),
            output_index INTEGER NOT NULL CHECK(output_index>=0),
            secret TEXT NOT NULL UNIQUE,
            blind TEXT NOT NULL,
            UNIQUE(conversion_id,output_index)
        )
    """)
    # Only costs borne by the issuer belong here. A routing fee covered by
    # the melting wallet's inputs must not also be charged to operator income.
    await conn.execute("""
        CREATE TABLE IF NOT EXISTS goats_fee_expenses (
            source_id TEXT PRIMARY KEY NOT NULL CHECK(length(source_id)>0),
            amount_sat INTEGER NOT NULL CHECK(amount_sat>0),
            incurred_at INTEGER NOT NULL CHECK(incurred_at>0)
        )
    """)
    for table in ("goats_fee_conversions", "goats_fee_outputs", "goats_fee_expenses"):
        for operation in ("UPDATE", "DELETE"):
            await conn.execute(f"""
                CREATE TRIGGER IF NOT EXISTS {table}_no_{operation} BEFORE {operation} ON {table}
                BEGIN SELECT RAISE(ABORT, 'fee conversion history is immutable'); END
            """)


async def converted_sat(conn):
    # Only signed sat promises matched to a journal entry explain a fee conversion.
    rows = await conn.fetchall("""
        SELECT c.amount_sat, COALESCE(SUM(CASE WHEN p.c_ IS NOT NULL AND k.unit='sat'
                                               THEN p.amount ELSE 0 END),0) AS issued
        FROM goats_fee_conversions c LEFT JOIN goats_fee_outputs o USING(conversion_id)
        LEFT JOIN promises p ON p.b_=o.blinded LEFT JOIN keysets k ON k.id=p.id
        GROUP BY c.conversion_id
    """)
    if any(row["amount_sat"] != row["issued"] for row in rows):
        raise RuntimeError("fee conversion liability differs from signed outputs")
    return sum(row["amount_sat"] for row in rows)


async def issue_conversion(ledger, backend, conversion_id, material, recipient_key, operating_buffer):
    """Run on HOME with the locally pinned recipient and durable private output material."""
    import hashlib
    import json
    import re
    import time
    from uuid import UUID

    from cashu.core.base import BlindedMessage, Unit
    from cashu.core.crypto import b_dhke
    from cashu.core.db import LockOptions
    from cashu.core.settings import settings
    from coincurve import PrivateKey, PublicKey

    identity = str(UUID(conversion_id))
    if PublicKey(bytes.fromhex(recipient_key)).format().hex() != recipient_key:
        raise ValueError("invalid operator key")
    if type(operating_buffer) is not int or operating_buffer < 0 or not 1 <= len(material) <= 20:
        raise ValueError("invalid fee conversion terms")
    outputs = []
    nonces = set()
    for item in material:
        kind, lock = json.loads(item["secret"])
        if (kind != "P2PK" or set(lock) != {"data", "nonce"} or lock["data"] != recipient_key
                or not re.fullmatch("[0-9a-f]{64}", lock["nonce"]) or lock["nonce"] in nonces):
            raise ValueError("fee output must be exclusively operator-owned")
        nonces.add(lock["nonce"])
        point, _ = b_dhke.step1_alice(item["secret"], PrivateKey(bytes.fromhex(item["blind"])))
        if point.format().hex() != item["B_"]:
            raise ValueError("operator lock differs from blinded output")
        outputs.append(BlindedMessage(amount=item["amount"], id=item["id"], B_=item["B_"]))
    amount = sum(o.amount for o in outputs)
    if not 1 <= amount <= 1_000_000:
        raise ValueError("fee conversion amount outside bounds")
    digest = hashlib.sha256(json.dumps([recipient_key, [o.model_dump() for o in outputs]],
                                       sort_keys=True).encode()).hexdigest()
    async with ledger.db.connect(locks=[LockOptions(table="balance_log")]) as conn:
        await ensure_journal(conn)
        prior = await conn.fetchone("SELECT request_digest FROM goats_fee_conversions WHERE conversion_id=:id",
                                    {"id": identity})
        if prior:
            if prior["request_digest"] != digest:
                raise ValueError("fee conversion retry changed outputs or recipient")
        else:
            await ledger._verify_outputs(outputs, expected_unit=Unit.sat, conn=conn)
            funding = await backend._call("GET", "/v1/funding/balance")
            backend._verify_wallet(funding)
            now = int(time.time())
            if not 0 <= now - funding["last_synced_at"] <= 60:
                raise ValueError("fee conversion requires a fresh reserve sync")
            exposure = await conn.fetchone("""
                SELECT (SELECT COUNT(*) FROM melt_quotes WHERE unit='sat' AND
                           (state='PENDING' OR (state='PAID' AND paid_time>=:sync))) AS melts,
                       (SELECT COUNT(*) FROM mint_quotes WHERE unit='sat' AND state='PENDING') AS pending,
                       (SELECT COALESCE(SUM(amount),0) FROM mint_quotes
                         WHERE unit='sat' AND state='PAID') AS owed,
                       (SELECT COALESCE(SUM(amount_sat),0) FROM goats_fee_expenses) AS costs
            """, {"sync": funding["last_synced_at"]})
            if exposure["melts"] or exposure["pending"]:
                raise ValueError("fee conversion awaits settlement")
            liability, fees = await ledger.get_unit_balance_and_fees(Unit.sat, ledger.db, conn)
            undistributed = fees.amount - await converted_sat(conn)
            reserve = funding["balance_sat"]
            if settings.mint_max_balance and liability.amount + amount > settings.mint_max_balance:
                raise ValueError("fee conversion exceeds the mint liability cap")
            if (reserve < liability.amount + undistributed + exposure["owed"]
                    or amount > min(undistributed - exposure["costs"],
                                    reserve - liability.amount - exposure["owed"] - operating_buffer)):
                raise ValueError("insufficient reconciled operator earnings")
            await ledger._store_blinded_messages(outputs, conn=conn)
            signatures = await ledger._sign_blinded_messages(outputs, conn=conn)
            await conn.execute("""
                INSERT INTO goats_fee_conversions
                (conversion_id,recipient_key,request_digest,amount_sat,reserve_sat,synced_at,created_at)
                VALUES (:id,:key,:digest,:amount,:reserve,:sync,:now)
            """, {"id": identity, "key": recipient_key, "digest": digest, "amount": amount,
                  "reserve": reserve, "sync": funding["last_synced_at"], "now": now})
            for index, (output, item) in enumerate(zip(outputs, material, strict=True)):
                await conn.execute("""
                    INSERT INTO goats_fee_outputs(blinded,conversion_id,output_index,secret,blind)
                    VALUES (:b,:id,:idx,:secret,:blind)
                """, {"b": output.B_, "id": identity, "idx": index,
                      "secret": item["secret"], "blind": item["blind"]})
            return signatures
    restored, signatures = await ledger.restore(outputs)
    if restored != outputs:
        raise RuntimeError("could not restore complete fee conversion")
    return signatures


async def recover_material(conn, conversion_id):
    """Load the exact locked outputs after a commit, including after a lost reply."""
    rows = await conn.fetchall("""
        SELECT p.amount, p.id, o.blinded AS blinded, o.secret, o.blind
        FROM goats_fee_outputs o JOIN promises p ON p.b_=o.blinded
        WHERE o.conversion_id=:id AND p.c_ IS NOT NULL ORDER BY o.output_index
    """, {"id": conversion_id})
    return [{"amount": row["amount"], "id": row["id"], "B_": row["blinded"],
             "secret": row["secret"], "blind": row["blind"]} for row in rows]


async def recover_token(ledger, conversion_id):
    """Recover verified operator ecash from the original signed denomination outputs."""
    from cashu.core.base import BlindedMessage
    from operator_proofs import encode_token, unblind

    async with ledger.db.connect() as conn:
        material = await recover_material(conn, conversion_id)
        row = await conn.fetchone("SELECT amount_sat FROM goats_fee_conversions WHERE conversion_id=:id",
                                  {"id": conversion_id})
    if not row or not material or sum(item["amount"] for item in material) != row["amount_sat"]:
        raise RuntimeError("incomplete operator fee conversion")
    outputs = [BlindedMessage(id=item["id"], amount=item["amount"], B_=item["B_"])
               for item in material]
    restored, signatures = await ledger.restore(outputs)
    if restored != outputs or any(sig.amount != out.amount for sig, out in zip(signatures, outputs, strict=True)):
        raise RuntimeError("restored outputs differ from journal")
    keys = {key: {str(amount): point.format().hex() for amount, point in ledger.keysets[key].public_keys.items()}
            for key in {out.id for out in outputs}}
    proofs = [unblind(item, sig.model_dump(), keys) for item, sig in zip(material, signatures, strict=True)]
    return encode_token("https://mint.lightning-goats.com", proofs)
