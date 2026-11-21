"""Claim settled project quotes using only their original, journaled outputs."""

import json
import time

import bolt11

from cashu.core.base import BlindedMessage
from cashu.core.crypto import b_dhke
from cashu.core.crypto.secp import PrivateKey, PublicKey
from cashu.core.nuts import nut20


def collect(store, client, request_id, pinned_keys):
    row = store.db.execute("""
        SELECT terms, material, invoice, payment_hash, proofs FROM project_claim
        WHERE id=? AND invoice IS NOT NULL
    """, (request_id,)).fetchone()
    if row is None:
        raise ValueError("unknown project invoice")
    terms, material, invoice, payment_hash, saved = row
    if saved is not None:
        return True
    quote_id = "goats-" + request_id
    quote = client.get(f"/v1/mint/quote/bolt11/{quote_id}").raise_for_status().json()
    if quote["quote"] != quote_id or quote["request"] != invoice:
        raise ValueError("project quote binding differs")
    if quote["state"] == "UNPAID":
        store.schedule_unpaid(request_id, int(time.time()), bolt11.decode(invoice).expiry_time)
        return False
    material = json.loads(material)
    secrets = material["outputs"]
    outputs = [{k: item[k] for k in ("id", "amount", "B_")} for item in secrets]
    if quote["state"] == "PAID":
        signature = nut20.sign_mint_quote(
            quote_id, [BlindedMessage(**item) for item in outputs], material["owner"]
        )
        reply = client.post("/v1/mint/bolt11", json={
            "quote": quote_id, "outputs": outputs, "signature": signature,
        }).raise_for_status().json()
    elif quote["state"] == "ISSUED":
        reply = client.post("/v1/restore", json={"outputs": outputs}).raise_for_status().json()
        if [o["B_"] for o in reply["outputs"]] != [o["B_"] for o in outputs]:
            raise ValueError("restore returned different project outputs")
    else:
        raise ValueError("unknown project quote state")

    proofs = []
    for item, promise in zip(secrets, reply["signatures"], strict=True):
        amount, keyset = item["amount"], item["id"]
        if amount != promise["amount"] or keyset != promise["id"]:
            raise ValueError("project promise terms differ")
        mint_key = PublicKey(bytes.fromhex(pinned_keys[keyset][str(amount)]))
        blind_signature = PublicKey(bytes.fromhex(promise["C_"]))
        dleq = promise["dleq"]
        if not b_dhke.alice_verify_dleq(
            PublicKey(bytes.fromhex(item["B_"])), blind_signature,
            PrivateKey(bytes.fromhex(dleq["e"])), PrivateKey(bytes.fromhex(dleq["s"])), mint_key,
        ):
            raise ValueError("invalid project mint signature")
        signature = b_dhke.step3_alice(blind_signature, PrivateKey(bytes.fromhex(item["blind"])), mint_key)
        proofs.append({"id": keyset, "amount": amount, "secret": item["secret"],
                       "C": signature.format().hex(),
                       "dleq": {"e": dleq["e"], "s": dleq["s"], "r": item["blind"]}})
    if sum(p["amount"] for p in proofs) != json.loads(terms)["amount"]:
        raise ValueError("project claim amount differs")
    store.record_proofs(request_id, payment_hash, proofs)
    return True
