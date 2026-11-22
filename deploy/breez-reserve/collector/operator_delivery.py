"""Deliver existing recipient-locked operator proofs using a protected NUT-18 request."""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import time
from urllib.parse import urlsplit

import bech32
import cbor2
import websocket

MINT = "https://mint.lightning-goats.com"
HERE = Path(__file__).resolve().parent


def decode64(text):
    return base64.b64decode(text + "=" * (-len(text) % 4), altchars=b"-_", validate=True)


def destination(text):
    if not text.startswith("creqA") or len(text) > 8192:
        raise ValueError("invalid wallet payment request")
    request = cbor2.loads(decode64(text[5:]))
    lock = request["nut10"]
    if (request.get("u") != "sat" or request.get("a") is not None
            or request.get("s") or request.get("sm")
            or request.get("m") is not None and MINT not in request["m"]
            or set(lock) != {"k", "d"} or lock["k"] != "P2PK"):
        raise ValueError("expected reusable amountless Goats-mint P2PK request")
    transport, = [t for t in request["t"] if t["t"] == "nostr" and ["n", "17"] in t.get("g", [])]
    prefix, data = bech32.bech32_decode(transport["a"])
    if prefix != "nprofile":
        raise ValueError("expected NIP-19 profile")
    raw = bytes(bech32.convertbits(data, 5, 8, False))
    relays = []
    recipients = []
    while raw:
        tag, length = raw[:2]
        value, raw = raw[2:2+length], raw[2+length:]
        if len(value) != length:
            raise ValueError("truncated profile")
        if tag == 0:
            recipients.append(value.hex())
        if tag == 1:
            url = value.decode()
            parsed = urlsplit(url)
            if parsed.scheme != "wss" or not parsed.hostname or parsed.username or parsed.password or parsed.fragment:
                raise ValueError("expected secure wallet relay")
            if url not in relays:
                relays.append(url)
    if (len(recipients) != 1 or len(recipients[0]) != 64 or lock["d"][:2] not in ("02","03")
            or recipients[0] != lock["d"][2:] or not 1 <= len(relays) <= 8):
        raise ValueError("wallet receiving and transport keys differ or missing relays")
    return request.get("i"), lock["d"], relays


def envelope(token, amount, request_id, lock, wrapper):
    if not token.startswith("cashuA"):
        raise ValueError("unexpected operator token")
    decoded = json.loads(decode64(token[6:]))
    entry, = decoded["token"]
    proofs = entry["proofs"]
    if decoded["unit"] != "sat" or entry["mint"] != MINT or sum(p["amount"] for p in proofs) != amount:
        raise ValueError("operator token accounting mismatch")
    for proof in proofs:
        kind, fields = json.loads(proof["secret"])
        if kind != "P2PK" or set(fields) != {"nonce","data"} or fields["data"] != lock:
            raise ValueError("token differs from configured recipient")
    payload = {"mint": MINT, "unit": "sat", "proofs": proofs}
    if request_id is not None:
        payload["id"] = request_id
    result = subprocess.run(["/usr/bin/node", str(wrapper)], input=json.dumps({"recipient":lock[2:], "payload":payload}),
                            text=True, capture_output=True, timeout=12, check=True)
    return result.stdout


def publish(url, event):
    conn = None
    try:
        conn = websocket.create_connection(url, timeout=5, redirect_limit=0, suppress_origin=True)
        conn.send(json.dumps(["EVENT", event]))
        deadline = time.monotonic() + 6
        while time.monotonic() < deadline:
            conn.settimeout(max(0.1, deadline-time.monotonic()))
            reply = json.loads(conn.recv())
            if isinstance(reply,list) and len(reply)==4 and reply[:2]==["OK",event["id"]]:
                return reply[2] is True or (reply[2] is False and str(reply[3]).startswith("duplicate:"))
    except (OSError, ValueError, websocket.WebSocketException):
        pass
    finally:
        if conn is not None:
            conn.close(timeout=1)
    return False


def deliver_token(journal, feed, token, amount, recipient, request, wrapper=HERE/"operator_wrap.cjs", send=publish):
    rid, lock, relays = destination(request)
    journal.execute("CREATE TABLE IF NOT EXISTS delivery(feed TEXT PRIMARY KEY,token_hash TEXT NOT NULL,event TEXT NOT NULL,relays TEXT NOT NULL,ack_at INTEGER)")
    stamp = hashlib.sha256(token.encode()).hexdigest()
    with journal:
        journal.execute("BEGIN IMMEDIATE")
        row = journal.execute("SELECT token_hash,event,relays,ack_at FROM delivery WHERE feed=?", (feed,)).fetchone()
        if row:
            if row[0] != stamp:
                raise ValueError("issued token changed")
            if row[3] is not None:
                return True
            message, urls = row[1], json.loads(row[2])
        else:
            # Rotation applies to matching tokens only. Existing envelopes keep their route.
            if recipient != lock:
                return False
            message, urls = envelope(token, amount, rid, lock, wrapper), relays
            journal.execute("INSERT INTO delivery VALUES(?,?,?,?,NULL)", (feed,stamp,message,json.dumps(urls)))
    event = json.loads(message)
    with ThreadPoolExecutor(max_workers=len(urls)) as pool:
        accepted = list(pool.map(lambda url: send(url,event), urls))
    if any(accepted):
        with journal:
            journal.execute("UPDATE delivery SET ack_at=COALESCE(ack_at,unixepoch()) WHERE feed=?", (feed,))
        return True
    return False


def deliver(source, journal, request, wrapper=HERE/"operator_wrap.cjs", send=publish):
    rows = source.execute("""
        SELECT e.feed,e.token,e.amount_sat,p.binding FROM operator_ecash e
        JOIN proof_disposition p ON p.id=e.feed WHERE e.token IS NOT NULL
    """)
    for feed, token, amount, binding in rows:
        deliver_token(journal, feed, token, amount, json.loads(binding)["recipient"], request, wrapper, send)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", type=Path, required=True)
    args = parser.parse_args()
    source = sqlite3.connect("file:/var/lib/lightning-goats-project-collector/project-claims.sqlite?mode=ro",uri=True)
    journal = sqlite3.connect("/var/lib/lightning-goats-operator-delivery/outbox.sqlite")
    while True:
        try:
            deliver(source, journal, args.request.read_text().strip())
        except Exception:
            print("Operator delivery deferred; original token retained.", flush=True)
        time.sleep(5)
