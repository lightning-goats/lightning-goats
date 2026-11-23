"""Deliver existing recipient-locked operator proofs using a protected NUT-18 request."""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import ipaddress
import json
from pathlib import Path
import socket
import sqlite3
import ssl
import subprocess
import time
from urllib.parse import urlsplit

import bech32
from cashu.core.nuts import nut18
from coincurve import PublicKey
import websocket

MINT = "https://mint.lightning-goats.com"
HERE = Path(__file__).resolve().parent


def decode64(text):
    return base64.b64decode(text + "=" * (-len(text) % 4), altchars=b"-_", validate=True)


def destination(text):
    if not isinstance(text, str) or not 5 < len(text) <= 8192:
        raise ValueError("invalid wallet payment request")
    request = nut18.deserialize(text).model_dump(exclude_none=True)
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
    if (len(recipients) != 1 or len(recipients[0]) != 64 or not 1 <= len(relays) <= 8
            or len(lock["d"]) != 66 or lock["d"][:2] not in ("02", "03")):
        raise ValueError("invalid receiving key or Nostr transport")
    wallet_key = PublicKey(bytes.fromhex(lock["d"])).format().hex()
    PublicKey(bytes.fromhex("02" + recipients[0]))
    return request.get("i"), wallet_key, recipients[0], relays


def locked_proofs(token, amount, lock):
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
    return proofs


def public_reward_tags(token, amount, assignment):
    route = assignment["destination"]
    if (route["method"] != "public_nutzap" or route["member"] != assignment["member_key"]
            or not route["receiving_key"].startswith("02") or route["mint"].rstrip("/") != MINT):
        raise ValueError("public reward differs from frozen advertisement")
    proofs = locked_proofs(token, amount, route["receiving_key"])
    for proof in proofs:
        if not isinstance(proof.get("dleq"), dict) or set(proof["dleq"]) != {"e","s","r"}:
            raise ValueError("public reward requires wallet-verifiable DLEQ")
    return [["p",route["member"]],["u",route["mint"]],["unit","sat"],
            *[["proof",json.dumps(proof,separators=(",",":"))] for proof in proofs]]


def envelope(token, amount, request_id, lock, transport_key, wrapper):
    payload = {"mint": MINT, "unit": "sat", "proofs": locked_proofs(token, amount, lock)}
    if request_id is not None:
        payload["id"] = request_id
    result = subprocess.run(["/usr/bin/node", str(wrapper)], input=json.dumps({"recipient":transport_key, "payload":payload}),
                            text=True, capture_output=True, timeout=12, check=True)
    return result.stdout


def public_relay_socket(url):
    parsed = urlsplit(url)
    if (parsed.scheme != "wss" or not parsed.hostname or parsed.username is not None
            or parsed.password is not None or parsed.fragment or "%" in parsed.hostname):
        raise ValueError("invalid member relay")
    addresses = socket.getaddrinfo(parsed.hostname, parsed.port or 443, type=socket.SOCK_STREAM)
    for _, _, _, _, endpoint in addresses:
        address = ipaddress.ip_address(endpoint[0])
        if (not address.is_global or address.is_multicast
                or address.version == 6 and (address not in ipaddress.ip_network("2000::/3")
                    or address in ipaddress.ip_network("2002::/16")
                    or address in ipaddress.ip_network("2001::/23"))
                or address in ipaddress.ip_network("192.0.0.0/24")):
            raise ValueError("member relay requires public unicast")
    context = ssl.create_default_context()
    for family, kind, protocol, _, endpoint in addresses:
        with socket.socket(family, kind, protocol) as raw:
            raw.settimeout(5)
            try:
                raw.connect(endpoint)
                return context.wrap_socket(raw, server_hostname=parsed.hostname)
            except OSError:
                continue
    raise ConnectionError("member relay unavailable")


def publish(url, event, *, public_only=False):
    conn = None
    stream = None
    try:
        stream = public_relay_socket(url) if public_only else None
        conn = websocket.create_connection(url, timeout=5, redirect_limit=0, suppress_origin=True, socket=stream)
        if conn.getstatus() != 101:
            return False
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
        if stream is not None:
            stream.close()
    return False


def deliver_token(journal, feed, token, amount, recipient, resolve_destination, wrapper, send):
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
            rid, lock, transport_key, relays = resolve_destination()
            if recipient != lock:
                return False
            message, urls = envelope(token, amount, rid, lock, transport_key, wrapper), relays
            journal.execute("INSERT INTO delivery VALUES(?,?,?,?,NULL)", (feed,stamp,message,json.dumps(urls)))
    event = json.loads(message)
    with ThreadPoolExecutor(max_workers=len(urls)) as pool:
        accepted = list(pool.map(lambda url: send(url,event), urls))
    if any(accepted):
        with journal:
            journal.execute("UPDATE delivery SET ack_at=COALESCE(ack_at,unixepoch()) WHERE feed=?", (feed,))
        return True
    return False


def delivery_rows(source, query):
    # Release the collector read transaction before doing relay I/O. A relay
    # outage must not hold a read lock across a swap or a claim commit.
    after = 0
    while True:
        cursor = source.execute(query,(after,))
        try:
            batch = cursor.fetchall()
        finally:
            cursor.close()
        if not batch:
            return
        for after,*fields in batch:
            yield fields


def deliver(source, journal, request, wrapper=HERE/"operator_wrap.cjs", send=publish):
    rows = delivery_rows(source,"""
        SELECT e.rowid,e.feed,e.token,e.amount_sat,p.binding FROM operator_ecash e
        JOIN proof_disposition p ON p.id=e.feed WHERE e.token IS NOT NULL AND e.rowid>? ORDER BY e.rowid LIMIT 32
    """)
    for feed, token, amount, binding in rows:
        try:
            deliver_token(journal, feed, token, amount, json.loads(binding)["recipient"], lambda: destination(request), wrapper, send)
        except (ValueError, TypeError, KeyError, subprocess.SubprocessError):
            # A held envelope must not block delivery for later confirmed feeds.
            # Keep token material and recipient details out of the service log.
            print("Operator delivery deferred for one feed; original token retained.", flush=True)


def publish_member(url, event):
    return publish(url, event, public_only=True)


def deliver_private_members(source, journal, wrapper=HERE/"operator_wrap.cjs", send=publish_member):
    rows = delivery_rows(source,"""
        SELECT m.rowid,m.feed,m.token,m.amount_sat,p.binding FROM member_ecash m
        JOIN proof_disposition p ON p.id=m.feed WHERE m.token IS NOT NULL
        AND json_extract(p.binding,'$.assignment.destination.method')='private_ecash' AND m.rowid>? ORDER BY m.rowid LIMIT 32
    """)
    for disposition, token, amount, binding in rows:
        try:
            terms = json.loads(binding)
            frozen = terms["assignment"]["destination"]
            route = frozen.get("payment_id"), frozen["receiving_key"], frozen["transport_key"], frozen["relays"]
            deliver_token(journal, disposition, token, amount, terms["recipient"], lambda: route, wrapper, send)
        except (ValueError, TypeError, KeyError, subprocess.SubprocessError):
            print("Private member delivery deferred; original token retained.", flush=True)


def deliver_public_members(source, journal, send=publish_member):
    journal.execute("CREATE TABLE IF NOT EXISTS delivery(feed TEXT PRIMARY KEY,token_hash TEXT NOT NULL,event TEXT NOT NULL,relays TEXT NOT NULL,ack_at INTEGER)")
    journal.execute("CREATE TABLE IF NOT EXISTS public_relay_ack(feed TEXT NOT NULL,relay TEXT NOT NULL,PRIMARY KEY(feed,relay))")
    rows = delivery_rows(source,"""
        SELECT m.rowid,m.feed,m.token,b.signed_event,p.binding FROM member_publications b JOIN member_ecash m USING(feed)
        JOIN proof_disposition p ON p.id=m.feed WHERE b.signed_event IS NOT NULL AND m.rowid>? ORDER BY m.rowid LIMIT 32
    """)
    for disposition, token, message, binding in rows:
        try:
            deliver_public_event(journal, disposition, token, message, binding, send)
        except (ValueError, TypeError, KeyError, OSError):
            print("Public member delivery deferred for one reward; original event retained.", flush=True)


def deliver_public_event(journal, disposition, token, message, binding, send):
    route = json.loads(binding)["assignment"]["destination"]
    urls = list(dict.fromkeys(route["token_relays"] + route["read_relays"]))
    stamp = hashlib.sha256(token.encode()).hexdigest()
    with journal:
        journal.execute("INSERT INTO delivery VALUES(?,?,?,?,NULL) ON CONFLICT(feed) DO NOTHING",
                        (disposition,stamp,message,json.dumps(urls)))
        saved = journal.execute("SELECT token_hash,event,relays FROM delivery WHERE feed=?",(disposition,)).fetchone()
        if saved != (stamp,message,json.dumps(urls)):
            raise ValueError("public delivery differs from original")
    acked = {url for url, in journal.execute("SELECT relay FROM public_relay_ack WHERE feed=?",(disposition,))}
    pending = [url for url in urls if url not in acked]
    if not pending:
        return
    event = json.loads(message)
    with ThreadPoolExecutor(max_workers=min(8,len(pending))) as pool:
        for url,accepted in zip(pending,pool.map(lambda relay: send(relay,event),pending),strict=True):
            if accepted:
                with journal:
                    journal.execute("INSERT OR IGNORE INTO public_relay_ack VALUES(?,?)",(disposition,url))
                    if url in route["token_relays"]:
                        journal.execute("UPDATE delivery SET ack_at=COALESCE(ack_at,unixepoch()) WHERE feed=?",(disposition,))



if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", type=Path, required=True)
    args = parser.parse_args()
    source = sqlite3.connect("file:/var/lib/lightning-goats-project-collector/project-claims.sqlite?mode=ro",uri=True)
    journal = sqlite3.connect("/var/lib/lightning-goats-operator-delivery/outbox.sqlite")
    while True:
        try:
            deliver(source, journal, args.request.read_text().strip())
        except (OSError, ValueError, sqlite3.Error):
            print("Operator delivery deferred; original token retained.", flush=True)
        for deliver_members in (deliver_private_members, deliver_public_members):
            try:
                deliver_members(source, journal)
            except (OSError, ValueError, sqlite3.Error):
                print("Member delivery deferred; original reward retained.", flush=True)
        time.sleep(5)
