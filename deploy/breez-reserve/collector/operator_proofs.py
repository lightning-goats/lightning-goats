"""Verify and unblind Cashu signatures against pinned mint keys."""
import base64
import json

from cashu.core.crypto import b_dhke
from cashu.core.crypto.secp import PrivateKey, PublicKey


def unblind(item, promise, pinned_keys):
    amount, keyset = promise["amount"], promise["id"]
    if type(amount) is not int or amount <= 0 or keyset != item["id"]:
        raise ValueError("invalid mint promise denomination or keyset")
    mint_key = PublicKey(bytes.fromhex(pinned_keys[keyset][str(amount)]))
    blind_signature = PublicKey(bytes.fromhex(promise["C_"]))
    dleq = promise["dleq"]
    if not b_dhke.alice_verify_dleq(
        PublicKey(bytes.fromhex(item["B_"])), blind_signature,
        PrivateKey(bytes.fromhex(dleq["e"])), PrivateKey(bytes.fromhex(dleq["s"])), mint_key,
    ):
        raise ValueError("invalid project mint signature")
    signature = b_dhke.step3_alice(blind_signature, PrivateKey(bytes.fromhex(item["blind"])), mint_key)
    return {"id": keyset, "amount": amount, "secret": item["secret"],
                   "C": signature.format().hex(),
                   "dleq": {"e": dleq["e"], "s": dleq["s"], "r": item["blind"]}}


def encode_token(mint, proofs):
    payload = {"unit": "sat", "token": [{"mint": mint, "proofs": proofs}]}
    raw = json.dumps(payload, separators=(",", ":"), sort_keys=True).encode()
    return "cashuA" + base64.urlsafe_b64encode(raw).decode().rstrip("=")
