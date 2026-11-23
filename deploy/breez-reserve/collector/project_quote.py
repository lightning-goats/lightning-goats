"""VPS project invoice issuance with the pinned Nutshell wallet primitives."""

import json
import re
import secrets
import time
from uuid import UUID

import bolt11
from cashu.core.crypto import b_dhke
from coincurve import PrivateKey


def prepare_outputs(amount, keyset, *, recipient=None):
    outputs = []
    for bit in range(amount.bit_length()):
        denomination = 1 << bit
        if amount & denomination:
            secret = secrets.token_hex(32)
            if recipient is not None:
                secret = json.dumps(["P2PK", {"nonce": secret, "data": recipient}], separators=(",", ":"))
            blinded, factor = b_dhke.step1_alice(secret)
            outputs.append({
                "amount": denomination, "id": keyset, "B_": blinded.format().hex(),
                "secret": secret, "blind": factor.secret.hex(),
            })
    return outputs


def prepare_claim(amount, keyset):
    return {"owner": PrivateKey().secret.hex(), "outputs": prepare_outputs(amount, keyset)}


def issue(store, private_client, token, request_id, amount, digest, keyset, attribution):
    """private_client is scoped to the HOME mTLS origin with redirects disabled."""
    if type(amount) is not int or not 1 <= amount <= 1_000_000:
        raise ValueError("project invoice amount outside HOME bounds")
    request_id = str(UUID(request_id))
    if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise ValueError("invalid project description hash")
    terms = {"amount": amount, "description_hash": digest, "attribution": attribution}
    material = store.prepare(request_id, terms, lambda: prepare_claim(amount, keyset))
    owner = PrivateKey(bytes.fromhex(material["owner"])).public_key.format().hex()
    response = private_client.post("/private/v1/project-quotes", headers={
        "Authorization": "Bearer " + token,
    }, json={
        "request_id": request_id, "amount_sat": amount,
        "description_hash": digest, "claim_pubkey": owner,
    }, follow_redirects=False)
    response.raise_for_status()
    return record_quote(store, response.json(), request_id, owner, amount, digest, require_current=True)


def record_quote(store, quote, request_id, owner, amount, digest, *, require_current):
    if quote["quote"] != "goats-" + request_id or quote["pubkey"] != owner:
        raise ValueError("project quote owner or identity differs")
    invoice = bolt11.decode(quote["request"])
    if (invoice.currency != "bc" or invoice.amount_msat != amount * 1000
            or invoice.description_hash != digest or (require_current and invoice.expiry_time <= time.time())
            or not 0 < invoice.expiry <= 3600):
        raise ValueError("signed project invoice differs from requested terms")
    store.record_invoice(request_id, quote["request"], invoice.payment_hash)
    return quote["request"]


def recover_invoice(store, client, request_id):
    """Look up the original quote after an interrupted creation. Never create one here."""
    terms_text, material_text = store.db.execute(
        "SELECT terms,material FROM project_claim WHERE id=?", (request_id,)
    ).fetchone()
    terms = json.loads(terms_text)
    owner = PrivateKey(bytes.fromhex(json.loads(material_text)["owner"])).public_key.format().hex()
    quote = client.get("/v1/mint/quote/bolt11/goats-" + str(UUID(request_id))).raise_for_status().json()
    record_quote(store, quote, request_id, owner, terms["amount"], terms["description_hash"],
                 require_current=False)
