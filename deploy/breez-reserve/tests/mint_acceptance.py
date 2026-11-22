"""Exercise the pinned Nutshell ASGI service against a disposable reserve fixture."""

import hashlib
import json
import os
import secrets
import shutil
import subprocess
import sys
import sqlite3
import threading
import time
import traceback
import uuid
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from socketserver import UnixStreamServer
from tempfile import TemporaryDirectory

import bolt11
import httpx
from coincurve import PrivateKey
from mnemonic import Mnemonic

from cashu.core.base import BlindedMessage
from cashu.core.crypto import b_dhke
from cashu.core.nuts import nut20
from cashu.core.crypto.secp import PublicKey

WALLET = "disposable-reserve-fixture"
PROJECT_TOKEN = "fixture-token-" + "a" * 36
TEST_SIGNER = "11" * 32


def invoice(sats, *, memo=None, digest=None, key=None, created_at=None):
    preimage = key or secrets.token_hex(32)
    payment_hash = hashlib.sha256(bytes.fromhex(preimage)).hexdigest()
    tags = bolt11.Tags([
        bolt11.Tag(bolt11.TagChar.payment_hash, payment_hash),
        bolt11.Tag(bolt11.TagChar.payment_secret, "22" * 32),
        bolt11.Tag(bolt11.TagChar.expire_time, 900),
        bolt11.Tag(bolt11.TagChar.description_hash if digest else bolt11.TagChar.description, digest or memo),
    ])
    return bolt11.encode(bolt11.Bolt11("bc", int(time.time()) if created_at is None else created_at, tags, bolt11.MilliSatoshi(sats * 1000)), private_key=TEST_SIGNER), preimage


class Fixture(UnixStreamServer):
    def __init__(self, path):
        super().__init__(str(path), Handler)
        self.receives = {}
        self.balance = 100
        self.synced_at = int(time.time())
        self.settlements = {}
        self.paid = set()
        self.sends = {}
        self.pay_attempts = 0
        self.complete_sends = False
        self.destination = invoice(12, memo="independent test destination")


class Handler(BaseHTTPRequestHandler):
    def address_string(self):
        return "fixture"

    def log_message(self, *_):
        pass

    def response(self, data, status=200):
        encoded = json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def do_GET(self):
        identity = {"wallet": WALLET, "network": "mainnet"}
        if self.path == "/health":
            self.response(identity)
        elif self.path == "/v1/funding/balance":
            self.response({**identity, "balance_sat": self.server.balance, "last_synced_at": self.server.synced_at})
        elif self.path.startswith("/v1/invoices/"):
            payment_hash = self.path.rsplit("/", 1)[1]
            self.response({"payment_hash": payment_hash, "state": "settled" if payment_hash in self.server.paid else "pending",
                           **self.server.settlements.get(payment_hash, {})})
        elif self.path.startswith("/v1/funding/payments/"):
            operation_id = self.path.rsplit("/", 1)[1]
            self.response(self.server.sends[operation_id] if self.server.complete_sends else
                          {"operation_id": operation_id, "state": "pending"})
        else:
            self.response({}, 404)

    def do_POST(self):
        data = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path == "/v1/invoices":
            ident = data["operation_id"]
            if ident in self.server.receives:
                original, response = self.server.receives[ident]
                self.response(response if original == data else {}, 200 if original == data else 409)
                return
            description = data["description"]
            pr, _ = invoice(data["amount_sat"], memo=description.get("memo"), digest=description.get("description_hash"))
            signed = bolt11.decode(pr)
            response = {"operation_id": ident, "invoice": pr, "wallet": WALLET, "network": "mainnet",
                        "payment_hash": signed.payment_hash, "amount_sat": data["amount_sat"], "expires_at": signed.expiry_time}
            self.server.receives[ident] = (data, response)
            self.response(response)
        elif self.path == "/v1/funding/fee":
            signed = bolt11.decode(data["invoice"])
            self.response({"payment_hash": signed.payment_hash, "amount_sat": int(signed.amount_msat) // 1000, "fee_sat": 4})
        elif self.path == "/v1/funding/payments":
            self.server.pay_attempts += 1
            assert (data["invoice"], data["max_fee_sat"]) == (self.server.destination[0], 4)
            operation = data["operation_id"]
            signed = bolt11.decode(data["invoice"])
            self.server.sends[operation] = {"state": "settled", "operation_id": operation,
                "amount_sat": 12, "fee_sat": 1, "preimage": self.server.destination[1], "payment_hash": signed.payment_hash}
            self.response({"operation_id": operation, "state": "pending"})
        else:
            self.response({}, 404)


def signed_zap_bytes():
    key = PrivateKey(bytes.fromhex("31" * 32))
    pubkey = key.public_key_xonly.format().hex()
    event = {"pubkey": pubkey, "created_at": 1700000000, "kind": 9734,
             "tags": [["p", "22" * 32], ["amount", "8000"]], "content": " goat  feed "}
    serialization = [0, event["pubkey"], event["created_at"], event["kind"], event["tags"], event["content"]]
    event_id = hashlib.sha256(json.dumps(serialization, ensure_ascii=False, separators=(",", ":")).encode()).digest()
    signature = key.sign_schnorr(event_id, aux_randomness=bytes(32))
    assert key.public_key_xonly.verify(signature, event_id)
    event.update({"id": event_id.hex(), "sig": signature.hex()})
    return json.dumps(event, indent=2).encode()


def new_outputs(keyset, amounts):
    outputs, material = [], []
    for amount in amounts:
        secret = secrets.token_hex(32)
        blinded, factor = b_dhke.step1_alice(secret)
        outputs.append(BlindedMessage(id=keyset, amount=amount, B_=blinded.format().hex()))
        material.append((secret, factor))
    return outputs, material


def encode_outputs(outputs):
    return [output.model_dump(mode="json") for output in outputs]


def proof(signature, material, keys):
    secret, factor = material
    assert signature["dleq"]
    mint_key = PublicKey(bytes.fromhex(keys[str(signature["amount"])]))
    c = b_dhke.step3_alice(PublicKey(bytes.fromhex(signature["C_"])), factor, mint_key)
    return {"id": signature["id"], "amount": signature["amount"], "secret": secret, "C": c.format().hex()}


def expect(response, status=200):
    assert response.status_code == status, (response.request.url.path, response.status_code, response.text)
    return response.json()


def wallet_state(client, item):
    y = b_dhke.hash_to_curve(item["secret"].encode()).format().hex()
    return expect(client.post("/v1/checkstate", json={"Ys": [y]}))["states"][0]["state"]


def start_mint(directory, socket, reserve, credentials, *, watchdog=False, input_fee_ppk=0):
    log = (directory / "process.log").open("ab")
    env = {**os.environ, "STATE_DIRECTORY": str(directory), "CREDENTIALS_DIRECTORY": str(credentials),
           "LG_RESERVE_FUND_SOCKET": str(reserve), "LG_MINT_SOCKET": str(socket),
           "LG_RESERVE_WALLET": WALLET, "LG_RESERVE_NETWORK": "mainnet",
           "MINT_BACKEND_BOLT11_SAT": "GoatsReserve", "MINT_DATABASE": str(directory / "db"),
           "CASHU_DIR": str(directory), "MINT_RPC_SERVER_ENABLE": "false",
           "MINT_WATCHDOG_ENABLED": str(watchdog).lower(), "MINT_WATCHDOG_BALANCE_CHECK_INTERVAL_SECONDS": "0.25", "MINT_RATE_LIMIT": "false", "DEBUG": "false",
           "MINT_MAX_MINT_BOLT11_SAT": "0", "MINT_MAX_MELT_BOLT11_SAT": "1000001",
           "MINT_INPUT_FEE_PPK": str(input_fee_ppk), "MINT_QUOTE_BACKEND_CHECK_RATE_LIMIT": "0", "LOG_LEVEL": "INFO"}
    process = subprocess.Popen([sys.executable, "-m", "lg_mint"], cwd=directory, env=env, stdout=log, stderr=log)
    client = httpx.Client(transport=httpx.HTTPTransport(uds=str(socket)), base_url="http://mint", timeout=6)
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        if process.poll() is not None:
            break
        try:
            if client.get("/v1/info").status_code == 200:
                log.close()
                return process, client
        except httpx.TransportError:
            pass
        time.sleep(.1)
    process.terminate() if process.poll() is None else None
    process.wait(timeout=10)
    log.close()
    raise RuntimeError((directory / "process.log").read_text()[-12000:])


def main():
    with TemporaryDirectory(prefix="lg-mint-") as raw:
        home = Path(raw)
        reserve_path, mint_path = home / "reserve.sock", home / "mint.sock"
        cred, original, restored = (home / name for name in ("cred", "original", "restored"))
        for directory in (cred, original):
            directory.mkdir(mode=0o700)
        (cred / "mint-seed").write_text(Mnemonic("english").generate(strength=128))
        (cred / "project-quote-token").write_text(PROJECT_TOKEN)
        with Fixture(reserve_path) as reserve:
            threading.Thread(target=reserve.serve_forever, daemon=True).start()
            process, viewer = start_mint(original, mint_path, reserve_path, cred)
            try:
                info = expect(viewer.get("/v1/info"))
                assert "20" in info["nuts"]
                for nut in ("4", "5"):
                    assert info["nuts"][nut]["methods"][0]["max_amount"] == 1_000_000
                maximum = expect(viewer.post("/v1/mint/quote/bolt11", json={"amount": 1_000_000, "unit": "sat"}))
                assert bolt11.decode(maximum["request"]).amount_msat == 1_000_000_000
                expect(viewer.post("/v1/mint/quote/bolt11", json={"amount": 1_000_001, "unit": "sat"}), 400)
                for amount, code in [(1_000_000, 200), (1_000_001, 400)]:
                    outbound, _ = invoice(amount, memo="boundary fixture")
                    expect(viewer.post("/v1/melt/quote/bolt11", json={"request": outbound, "unit": "sat"}), code)
                current = expect(viewer.get("/v1/keys"))["keysets"][0]
                keyset, keys = current["id"], current["keys"]
                viewer_quote = expect(viewer.post("/v1/mint/quote/bolt11", json={"amount": 16, "unit": "sat"}))
                assert bolt11.decode(viewer_quote["request"]).description == "Cashu mint quote"
                reserve.paid.add(bolt11.decode(viewer_quote["request"]).payment_hash)
                out, material = new_outputs(keyset, [16])
                viewer_proof = proof(expect(viewer.post("/v1/mint/bolt11", json={"quote": viewer_quote["quote"],
                    "outputs": encode_outputs(out)}))["signatures"][0], material[0], keys)
                swap_out, swap_material = new_outputs(keyset, [16])
                swapped = proof(expect(viewer.post("/v1/swap", json={"inputs": [viewer_proof],
                    "outputs": encode_outputs(swap_out)}))["signatures"][0], swap_material[0], keys)
                assert wallet_state(viewer, viewer_proof) == "SPENT"

                # Persist the collector's owner key and blinded outputs before publishing the invoice.
                owner = PrivateKey()
                project_outputs, project_material = new_outputs(keyset, [8])
                request = {"request_id": str(uuid.uuid4()), "amount_sat": 8,
                           "description_hash": hashlib.sha256(signed_zap_bytes()).hexdigest(),
                           "claim_pubkey": owner.public_key.format().hex()}
                owner_file = home / "collector.json"
                owner_file.write_text(json.dumps({"key": owner.secret.hex(), "request": request,
                    "outputs": encode_outputs(project_outputs), "secret": project_material[0][0],
                    "blind": project_material[0][1].secret.hex()}))
                auth = {"Authorization": f"Bearer {PROJECT_TOKEN}"}
                path = "/private/v1/project-quotes"
                expect(viewer.post(path, json=request), 401)
                with httpx.Client(transport=httpx.HTTPTransport(uds=str(mint_path)), base_url="http://mint") as collector:
                    boundary = {**request, "request_id": str(uuid.uuid4()), "amount_sat": 1_000_000}
                    max_project = expect(collector.post(path, json=boundary, headers=auth))
                    assert bolt11.decode(max_project["request"]).amount_msat == 1_000_000_000
                    expect(collector.post(path, json={**boundary, "request_id": str(uuid.uuid4()),
                                                      "amount_sat": 1_000_001}, headers=auth), 422)
                    project = expect(collector.post(path, json=request, headers=auth))
                    assert bolt11.decode(project["request"]).description_hash == request["description_hash"]
                    assert expect(collector.post(path, json=request, headers=auth))["request"] == project["request"]
                    expect(collector.post(path, json={**request, "amount_sat": 9}, headers=auth), 400)
                expect(viewer.post("/v1/melt/quote/bolt11", json={"request": project["request"], "unit": "sat"}), 400)
                reserve.paid.add(bolt11.decode(project["request"]).payment_hash)
                for _ in range(15):
                    check = expect(viewer.get(f"/v1/mint/quote/bolt11/{project['quote']}"))
                    if check["state"].upper() == "PAID":
                        break
                    time.sleep(.1)
                else:
                    raise AssertionError("fixture project quote never became paid")
                claim = {"quote": project["quote"], "outputs": encode_outputs(project_outputs)}
                expect(viewer.post("/v1/mint/bolt11", json=claim), 400)
                signature = nut20.sign_mint_quote(project["quote"], project_outputs, owner.secret.hex())
                discard = viewer.post("/v1/mint/bolt11", json={**claim, "signature": signature})
                assert discard.status_code == 200, discard.text  # Discard the response promises.
                replay_claim = viewer.post("/v1/mint/bolt11", json={**claim, "signature": signature})
                if replay_claim.status_code == 200:
                    assert replay_claim.json() == discard.json()  # NUT-19 exact response replay.
                else:
                    expect(replay_claim, 400)
                other_outputs, _ = new_outputs(keyset, [8])
                expect(viewer.post("/v1/mint/bolt11", json={"quote": project["quote"], "outputs": encode_outputs(other_outputs),
                    "signature": nut20.sign_mint_quote(project["quote"], other_outputs, owner.secret.hex())}), 400)
                promise = expect(viewer.post("/v1/restore", json={"outputs": claim["outputs"]}))["signatures"]
                assert len(promise) == 1
                project_proof = proof(promise[0], project_material[0], keys)
                assert wallet_state(viewer, project_proof) == "UNSPENT"

                # Run the VPS collector against this mint and its simulated reserve.
                sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "collector"))
                from claim_store import ClaimStore
                from project_quote import issue
                from collect import collect, collect_acquisition, execute_payout, recover_payout_refund, submit_payout
                from invoice_server import create, handle, handle_request
                import socket
                import struct

                claim_dir = home / "vps-claims"
                claim_dir.mkdir(mode=0o700)
                claims = ClaimStore(claim_dir)
                claim_id = str(uuid.uuid4())
                with httpx.Client(transport=httpx.HTTPTransport(uds=str(mint_path)), base_url="http://mint") as private:
                    left, right = socket.socketpair()
                    with left, right, left.makefile("rb") as replies:
                        body = json.dumps({"request_id": claim_id, "amount_sat": 10, "user": "dexter",
                                           "credit_pool": "herd", "description": "collector fixture",
                                           "zap": False}).encode()
                        left.sendall(struct.pack("!I", len(body)) + body)
                        handle(right, os.getuid(), claims, private, PROJECT_TOKEN, keyset)
                        size, = struct.unpack("!I", replies.read(4))
                        invoice_text = json.loads(replies.read(size))["invoice"]
                    terms = claims.db.execute("SELECT terms FROM project_claim WHERE id=?", (claim_id,)).fetchone()[0]
                    assert json.loads(terms)["attribution"] == {"user": "dexter", "credit_pool": "herd"}
                assert claims.reserve_poll(100, 15) == claim_id
                assert claims.reserve_poll(101, 15) is None
                assert claims.reserve_poll(115, 15) == claim_id
                assert not collect(claims, viewer, claim_id, {keyset: keys})
                expired_at = bolt11.decode(invoice_text).expiry_time
                claims.schedule_unpaid(claim_id, expired_at + 1, expired_at)
                assert claims.reserve_poll(expired_at + 300, 15) is None
                assert claims.reserve_poll(expired_at + 301, 15) == claim_id
                reserve.paid.add(bolt11.decode(invoice_text).payment_hash)
                class LostClaimResponse:
                    get = viewer.get

                    def post(self, path, **kwargs):
                        assert path == "/v1/mint/bolt11"
                        viewer.post(path, **kwargs).raise_for_status()
                        raise httpx.ReadTimeout("fixture discarded claim response")

                for _ in range(15):
                    try:
                        collect(claims, LostClaimResponse(), claim_id, {keyset: keys})
                    except httpx.ReadTimeout:
                        break
                    time.sleep(.1)
                else:
                    raise AssertionError("fixture did not reach lost project claim response")
                claims.db.close()
                claims = ClaimStore(claim_dir)
                assert collect(claims, viewer, claim_id, {keyset: keys})
                status = create({"claim_status": claim_id}, claims, None, None, None)
                assert status == {"request_id": claim_id, "payment_hash": bolt11.decode(invoice_text).payment_hash,
                                  "amount_sat": 10, "proof_digest": hashlib.sha256(claims.db.execute(
                                      "SELECT proofs FROM project_claim WHERE id=?", (claim_id,)
                                  ).fetchone()[0].encode()).hexdigest()}
                assert claims.reserve_poll(130, 15) is None
                saved = claims.db.execute("SELECT proofs FROM project_claim WHERE id=?", (claim_id,)).fetchone()[0]
                assert sum(p["amount"] for p in json.loads(saved)) == 10
                proofs = json.loads(saved)
                payout_input = next(p for p in proofs if p["amount"] == 8)
                change_input = next(p for p in proofs if p["amount"] == 2)
                payout_id = str(uuid.uuid4())
                feed_id = str(uuid.uuid4())
                allocation = {claim_id: {"evidence_digest": status["proof_digest"], "consumed_sats": 8}}
                assert claims.reserve_payout(payout_id, feed_id, "33"*32, 7, 1, allocation) == [payout_input]
                change = [{"secret": uuid.uuid4().hex, "blind": uuid.uuid4().hex}]
                assert claims.prepare_melt(payout_id, "33"*32, "quote-a", lambda: change) == change
                assert claims.begin_melt_submission(payout_id, "quote-a")
                claims.db.close()
                claims = ClaimStore(claim_dir)
                assert claims.reserve_payout(payout_id, feed_id, "33"*32, 7, 1, allocation) == [payout_input]
                assert not claims.begin_melt_submission(payout_id, "quote-a")

                def unexpected_generation():
                    raise AssertionError("regenerated change on restart")

                assert claims.prepare_melt(payout_id, "33"*32, "quote-a", unexpected_generation) == change
                for digest, quote in [("44"*32, "quote-a"), ("33"*32, "quote-b")]:
                    try:
                        claims.prepare_melt(payout_id, digest, quote, unexpected_generation)
                    except ValueError:
                        pass
                    else:
                        raise AssertionError("melt replay changed its invoice or quote")
                try:
                    claims.reserve_payout(str(uuid.uuid4()), str(uuid.uuid4()), "33"*32, 2, 0,
                                          {claim_id: {**allocation[claim_id], "consumed_sats": 2}})
                except sqlite3.IntegrityError:
                    pass
                else:
                    raise AssertionError("second disposition accepted the same payment hash")
                for invalid in [allocation, {claim_id: {**allocation[claim_id], "consumed_sats": 10}}]:
                    try:
                        claims.reserve_payout(str(uuid.uuid4()), feed_id, "44"*32, 2, 0, invalid)
                    except ValueError:
                        pass
                    else:
                        raise AssertionError("payout exceeded confirmed portion of a donation")
                transfer_id = str(uuid.uuid4())
                transfer_intent = {"request": "fixture", "amount_sats": 10}
                try:
                    claims.reserve_inputs(transfer_id, "operator_transfer", transfer_intent,
                                          [change_input["secret"], payout_input["secret"]])
                except sqlite3.IntegrityError:
                    pass
                else:
                    raise AssertionError("overlapping claim reservations succeeded")
                assert claims.reserve_inputs(transfer_id, "operator_transfer", {"amount_sats": 2},
                                             [change_input["secret"]]) == [change_input]
                claims.db.close()

                pending_request = {**request, "request_id": str(uuid.uuid4()),
                    "description_hash": hashlib.sha256(b'[["text/plain","Goats"]]').hexdigest()}
                pending_quote = expect(viewer.post(path, json=pending_request, headers=auth))
                claims = ClaimStore(claim_dir)
                contact_id = str(uuid.uuid4())
                contact_hash = hashlib.sha256(b"private contact fixture").hexdigest()
                contact_request = {"contact_id": contact_id, "description_hash": contact_hash}
                contact = create(contact_request, claims, viewer, PROJECT_TOKEN, keyset)
                signed_contact = bolt11.decode(contact["invoice"])
                assert signed_contact.amount_msat == 100_000 and signed_contact.description_hash == contact_hash
                assert create(contact_request, claims, viewer, PROJECT_TOKEN, keyset) == contact
                reserve.paid.add(signed_contact.payment_hash)
                for _ in range(15):
                    if collect(claims, viewer, contact_id, {keyset: keys}):
                        break
                    time.sleep(.1)
                contact_digest = create({"contact_claim_status": contact_id}, claims, None, None, None)["proof_digest"]
                assert contact_digest
                try:
                    claims.reserve_payout(str(uuid.uuid4()), str(uuid.uuid4()), "66"*32, 4, 0,
                                          {contact_id: {"consumed_sats": 100, "evidence_digest": contact_digest}})
                except ValueError:
                    pass
                else:
                    raise AssertionError("private contact fee funded a feeding payout")
                for wrong in [{"claim_status": contact_id}, {"contact_claim_status": claim_id}]:
                    try:
                        create(wrong, claims, None, None, None)
                    except ValueError:
                        pass
                    else:
                        raise AssertionError("contact and feeder custody evidence confused")
                funded_id = str(uuid.uuid4())
                funding = issue(claims, viewer, PROJECT_TOKEN, funded_id, 16, hashlib.sha256(b"payout fixture").hexdigest(),
                                keyset, {"user": "dexter", "credit_pool": "herd"})
                reserve.paid.add(bolt11.decode(funding).payment_hash)
                for _ in range(15):
                    if collect(claims, viewer, funded_id, {keyset: keys}):
                        break
                    time.sleep(.1)
                funded = create({"claim_status": funded_id}, claims, None, None, None)
                payment_id = str(uuid.uuid4())
                digest = bolt11.decode(reserve.destination[0]).payment_hash
                payout_feed = str(uuid.uuid4())
                authorization = {"disposition_id": payment_id, "feed_id": payout_feed,
                    "invoice": reserve.destination[0], "payment_hash": digest, "amount_sat": 12,
                    "maximum_fee_sat": 4, "retained_allocations": [{"ledger_id": 1, "consumed_sats": 16,
                    "anchor": {"source": "project_invoice", "claim_id": funded_id,
                               "evidence_digest": funded["proof_digest"]}}]}
                try:
                    execute_payout(claims, None, {**authorization, "amount_sat": 13}, keyset, {keyset: keys})
                except ValueError:
                    pass
                else:
                    raise AssertionError("mismatched payer authorization consumed funds")
                assert not claims.db.execute("SELECT 1 FROM proof_disposition WHERE id=?", (payment_id,)).fetchone()
                expired_invoice, _ = invoice(12, memo="expired fixture", created_at=int(time.time()) - 1000)
                try:
                    execute_payout(claims, None, {**authorization, "invoice": expired_invoice,
                                   "payment_hash": bolt11.decode(expired_invoice).payment_hash}, keyset, {keyset: keys})
                except ValueError as exc:
                    assert str(exc) == "expired payout invoice"
                else:
                    raise AssertionError("expired invoice reserved payout inputs")
                assert not claims.db.execute("SELECT 1 FROM proof_disposition WHERE id=?", (payment_id,)).fetchone()
                quote = execute_payout(None, viewer, {"quote_invoice": reserve.destination[0], "amount_sat": 12}, keyset, {})
                assert quote["payment_hash"] == digest and quote["amount_sat"] == 12
                assert quote["routing_reserve_sat"] == 4 and reserve.pay_attempts == 0
                costs = execute_payout(claims, viewer, {"quote_invoice": reserve.destination[0],
                    "amount_sat": 12, "feed_id": payout_feed, "gross_sat": 16,
                    "retained_allocations": authorization["retained_allocations"]}, keyset, {})
                assert costs["routing_reserve_sat"] == 4 and costs["input_fee_sat"] == 0
                assert not claims.db.execute("SELECT 1 FROM disposition_input WHERE disposition_id=?", (payment_id,)).fetchone()
                absent = {"reconcile_disposition": payment_id, "invoice": reserve.destination[0]}
                assert execute_payout(claims, None, absent, keyset, {keyset: keys}) == {"state": "UNRESERVED"}
                [payout_proof] = claims.reserve_payout(payment_id, payout_feed, digest, 12, 4,
                    {funded_id: {"consumed_sats": 16, "evidence_digest": funded["proof_digest"]}},
                    invoice_digest=hashlib.sha256(reserve.destination[0].encode()).hexdigest())
                lookup = {"reconcile_disposition": payment_id, "invoice": reserve.destination[0]}
                assert execute_payout(claims, None, lookup, keyset, {keyset: keys}) == {"state": "UNQUOTED"}
                replacement, _ = invoice(12, memo="different signed terms", key=reserve.destination[1])
                assert bolt11.decode(replacement).payment_hash == digest
                try:
                    execute_payout(claims, None, {**lookup, "invoice": replacement}, keyset, {keyset: keys})
                except ValueError as exc:
                    assert "differs from reserved terms" in str(exc)
                else:
                    raise AssertionError("reconciliation accepted a different invoice with the same hash")
                core_lookup = {"payout_receipt": payment_id, "feed_id": payout_feed, "payment_hash": digest,
                               "invoice_digest": hashlib.sha256(reserve.destination[0].encode()).hexdigest()}
                assert create(core_lookup, claims, None, None, None) is None
                class FeeBearingKeyset:
                    def post(self, path, **kwargs):
                        assert path == "/v1/melt/quote/bolt11"
                        return viewer.post(path, **kwargs)

                    def get(self, path):
                        assert path == "/v1/keysets"
                        return httpx.Response(200, request=httpx.Request("GET", "http://fixture/v1/keysets"),
                            json={"keysets": [{"id": keyset, "unit": "sat", "input_fee_ppk": 100}]})

                try:
                    submit_payout(claims, FeeBearingKeyset(), payment_id, reserve.destination[0], keyset, {keyset: keys})
                except ValueError as error:
                    assert "exceed payout allowance" in str(error)
                else:
                    raise AssertionError("input fee exceeded the authorized payout budget")
                assert reserve.pay_attempts == 0
                assert not claims.db.execute("SELECT 1 FROM payout_melt WHERE disposition_id=?", (payment_id,)).fetchone()
                class CrashBeforeDispatch:
                    get = viewer.get

                    def post(self, path, **kwargs):
                        if path == "/v1/melt/quote/bolt11":
                            return viewer.post(path, **kwargs)
                        assert path == "/v1/melt/bolt11"
                        raise httpx.ConnectError("fixture interrupted before sending request")

                try:
                    submit_payout(claims, CrashBeforeDispatch(), payment_id, reserve.destination[0],
                                  keyset, {keyset: keys})
                except httpx.ConnectError:
                    pass
                else:
                    raise AssertionError("missing simulated payout interruption")
                assert reserve.pay_attempts == 0
                claims.db.close()
                claims = ClaimStore(claim_dir)
                body = json.dumps(authorization).encode()
                class LookupOnly:
                    get = viewer.get

                assert execute_payout(claims, LookupOnly(), lookup, keyset, {keyset: keys})["state"] == "UNPAID"
                assert reserve.pay_attempts == 0
                def dispatch_payout(request):
                    return execute_payout(claims, viewer, request, keyset, {keyset: keys})

                for expected_uid in (os.getuid() + 1, os.getuid()):
                    left, right = socket.socketpair()
                    with left, right, left.makefile("rb") as replies:
                        left.sendall(struct.pack("!I", len(body)) + body)
                        handle_request(right, expected_uid, dispatch_payout)
                        if expected_uid != os.getuid():
                            assert reserve.pay_attempts == 0
                            continue
                        size, = struct.unpack("!I", replies.read(4))
                        paid = json.loads(replies.read(size))
                assert paid["state"].upper() == "PENDING"
                assert wallet_state(viewer, payout_proof) == "PENDING"
                claims.db.close()
            finally:
                viewer.close()
                process.terminate()
                process.wait(timeout=15)
            shutil.copytree(original, restored)
            if mint_path.exists():
                mint_path.unlink()
            process, viewer = start_mint(restored, mint_path, reserve_path, cred)
            try:
                assert expect(viewer.get("/v1/keys"))["keysets"][0]["id"] == keyset
                assert wallet_state(viewer, viewer_proof) == "SPENT"
                assert wallet_state(viewer, project_proof) == "UNSPENT"
                spend_out, _ = new_outputs(keyset, [8])
                expect(viewer.post("/v1/swap", json={"inputs": [project_proof], "outputs": encode_outputs(spend_out)}))
                assert wallet_state(viewer, project_proof) == "SPENT"
                replay = expect(viewer.post(path, json=pending_request, headers=auth))
                assert replay["request"] == pending_quote["request"]
                reserve.paid.add(bolt11.decode(replay["request"]).payment_hash)
                for _ in range(15):
                    checked = expect(viewer.get(f"/v1/mint/quote/bolt11/{replay['quote']}"))
                    if checked["state"].upper() == "PAID":
                        break
                    time.sleep(.1)
                else:
                    raise AssertionError("restored project quote never became paid")
                reserve.complete_sends = True
                claims = ClaimStore(claim_dir)
                LookupOnly.get = viewer.get
                final_melt = execute_payout(claims, LookupOnly(), lookup, keyset, {keyset: keys})
                claims.db.close()
                claims = ClaimStore(claim_dir)
                assert create(core_lookup, claims, None, None, None) == final_melt
                try:
                    create({**core_lookup, "feed_id": str(uuid.uuid4())}, claims, None, None, None)
                except ValueError:
                    pass
                else:
                    raise AssertionError("core accepted receipt for a different feed")
                assert submit_payout(claims, None, payment_id, reserve.destination[0], keyset, {keyset: keys}) == final_melt
                fee, saved_change = claims.db.execute("""
                    SELECT fee_sat,change_proofs FROM payout_melt_result WHERE disposition_id=?
                """, (payment_id,)).fetchone()
                selected_change = claims.reserve_payout(str(uuid.uuid4()), payout_feed, "55"*32, 3, 0,
                    {funded_id: {"consumed_sats": 16, "evidence_digest": funded["proof_digest"]}})
                assert {p["secret"] for p in selected_change} == {p["secret"] for p in json.loads(saved_change)}
                claims.db.close()
                assert fee == 1
                spend_change, spend_material = new_outputs(keyset, [1, 2])
                acquired_signatures = expect(viewer.post("/v1/swap", json={"inputs": selected_change,
                                                     "outputs": encode_outputs(spend_change)}))
                assert final_melt["state"].upper() == "PAID", final_melt
                assert final_melt["fee_sat"] == 1 and final_melt["amount_sat"] == 12
                assert sum(change["amount"] for change in selected_change) == 3
                assert wallet_state(viewer, payout_proof) == "SPENT"
                assert reserve.pay_attempts == 1
                claims = ClaimStore(claim_dir)
                conversion_id = str(uuid.uuid4())
                conversion = {"treasury_id": conversion_id, "amount_sat": 32, "description_hash": "ab"*32}
                conversion_invoice = create(conversion, claims, viewer, PROJECT_TOKEN, keyset)["invoice"]
                assert create(conversion, claims, viewer, PROJECT_TOKEN, keyset)["invoice"] == conversion_invoice
                reserve.paid.add(bolt11.decode(conversion_invoice).payment_hash)
                for _ in range(20):
                    if collect(claims, viewer, conversion_id, {keyset: keys}):
                        break
                    time.sleep(.1)
                status = create({"treasury_claim_status": conversion_id}, claims, None, None, None)
                assert status["amount_sat"] == 32 and status["proof_digest"]
                reserve.destination = invoice(12, memo="split fixture")
                split_request = {**authorization, "invoice":reserve.destination[0],
                    "payment_hash":bolt11.decode(reserve.destination[0]).payment_hash,
                    "disposition_id":str(uuid.uuid4()), "feed_id":str(uuid.uuid4()),
                    "retained_allocations":[{"ledger_id":2,"consumed_sats":16,"anchor":{
                        "source":"project_invoice","claim_id":conversion_id,"evidence_digest":status["proof_digest"]}}]}
                probe = execute_payout(claims, viewer, {"quote_invoice":reserve.destination[0],
                    "amount_sat":12,"gross_sat":16,"feed_id":split_request["feed_id"],
                    "retained_allocations":split_request["retained_allocations"]},keyset,{keyset:keys})
                assert probe["input_fee_sat"] == 0 and not claims.db.execute("SELECT 1 FROM proof_split").fetchone()
                class LostSplitResponse:
                    get = viewer.get
                    def post(self, path, **kwargs):
                        response = viewer.post(path, **kwargs)
                        if path == "/v1/swap":
                            response.raise_for_status()
                            raise httpx.ReadTimeout("discarded split response")
                        return response
                try:
                    execute_payout(claims, LostSplitResponse(), split_request, keyset, {keyset:keys})
                except httpx.ReadTimeout:
                    pass
                else:
                    raise AssertionError("failed to exercise lost swap response")
                claims.db.close()
                claims = ClaimStore(claim_dir)
                result = execute_payout(claims, viewer, split_request, keyset, {keyset:keys})
                for _ in range(30):
                    if result["state"] == "PAID":
                        break
                    time.sleep(.1)
                    result = execute_payout(claims, viewer, {"reconcile_disposition":split_request["disposition_id"],
                        "invoice":reserve.destination[0]}, keyset, {keyset:keys})
                assert result["state"] == "PAID" and result["fee_sat"] == 1
                assert reserve.pay_attempts == 2
                remaining = claims.db.execute("SELECT SUM(amount_sat) FROM collected_output o WHERE request_id=? "
                    "AND NOT EXISTS(SELECT 1 FROM disposition_input i WHERE i.secret=o.secret)",(conversion_id,)).fetchone()[0]
                assert remaining == 19
                acquired = [proof(p,m,keys) for p,m in zip(acquired_signatures["signatures"],spend_material,strict=True)]
                acquisition_id = str(uuid.uuid4())
                evidence = "ba"*32
                try:
                    collect_acquisition(claims, LostSplitResponse(), acquisition_id, evidence, acquired, keyset, {keyset:keys})
                except httpx.ReadTimeout:
                    pass
                else:
                    raise AssertionError("fixture did not discard acquisition swap response")
                claims.db.close()
                claims = ClaimStore(claim_dir)
                collect_acquisition(claims, viewer, acquisition_id, evidence, acquired, keyset, {keyset:keys})
                collect_acquisition(claims, None, acquisition_id, evidence, acquired, keyset, {keyset:keys})
                owned = claims.reserve_payout(str(uuid.uuid4()),str(uuid.uuid4()),"ad"*32,2,1,
                    {acquisition_id: {"source":"acquisition","consumed_sats":3,"evidence_digest":evidence}})
                assert sum(p["amount"] for p in owned) == 3
                assert all(wallet_state(viewer,p) == "SPENT" for p in acquired)
                # Model three 1-sat reservations with one real 2-sat fee refund.
                refund_id = str(uuid.uuid4())
                change = [p for p in owned if p["amount"] == 2]
                assert len(change) == 1
                with claims.write():
                    claims.db.execute("INSERT INTO proof_disposition VALUES (?,'cyberherd_payout','{}')",(refund_id,))
                    claims.db.execute("INSERT INTO payout_invoice VALUES (?,?)",("ae"*32,refund_id))
                    claims.db.execute("INSERT INTO payout_melt VALUES (?,?,'[]')",(refund_id,refund_id))
                    claims.db.execute("INSERT INTO payout_melt_submission VALUES (?)",(refund_id,))
                    for source in (funded_id,conversion_id,acquisition_id):
                        claims.db.execute("INSERT INTO payout_claim_allocation VALUES (?,?,?,1,1)",
                                          (refund_id,payout_feed,source))
                claims.record_melt_result(refund_id,0,"fixture",change)
                try:
                    recover_payout_refund(claims, LostSplitResponse(), refund_id, keyset, {keyset:keys})
                except httpx.ReadTimeout:
                    pass
                else:
                    raise AssertionError("fixture did not discard refund swap response")
                claims.db.close()
                claims = ClaimStore(claim_dir)
                recover_payout_refund(claims, viewer, refund_id, keyset, {keyset:keys})
                recover_payout_refund(claims, None, refund_id, keyset, {keyset:keys})
                credits = claims.db.execute("SELECT returned_sat FROM payout_claim_refund WHERE disposition_id=?",
                                            (refund_id,)).fetchall()
                assert credits == [(1,), (1,)] and wallet_state(viewer,change[0]) == "SPENT"
                for field in ("claim_status", "contact_claim_status"):
                    try:
                        create({field: conversion_id}, claims, None, None, None)
                    except ValueError:
                        pass
                    else:
                        raise AssertionError("conversion appeared as a new contribution or contact fee")
                claims.db.close()
            finally:
                viewer.close()
                process.terminate()
                process.wait(timeout=15)
    print("Nutshell fixture acceptance passed: mint, swap, NUT-20, restore, pending melt and change")


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        details = repr(exc) + "\n" + "\n".join(str(frame) for frame in traceback.extract_tb(exc.__traceback__, limit=50))
        os.write(2, details.encode() + b"\n")
        sys.exit(1)
