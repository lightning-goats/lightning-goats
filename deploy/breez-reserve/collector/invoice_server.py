"""Receive project invoice requests over a systemd-owned Unix socket."""

import argparse
import hashlib
import json
import os
import re
import socket
import ssl
import struct
from pathlib import Path

import httpx

from claim_store import ClaimStore
from project_quote import issue

MAX_FRAME = 65536


def create(request, store, client, token, keyset):
    if set(request) == {"claim_status"}:
        row = store.db.execute("""
            SELECT id,payment_hash,terms,proofs FROM project_claim
            WHERE id=? AND invoice IS NOT NULL
        """, (request["claim_status"],)).fetchone()
        if row is None:
            raise ValueError("unknown project invoice")
        return {"request_id": row[0], "payment_hash": row[1],
                "amount_sat": json.loads(row[2])["amount"], "proof_digest": hashlib.sha256(row[3].encode()).hexdigest() if row[3] else None}
    if set(request) != {"request_id", "amount_sat", "user", "credit_pool", "description", "zap"}:
        raise ValueError("invalid invoice request")
    if (not re.fullmatch(r"[a-z0-9._-]{1,64}", request["user"])
            or request["user"] in (".", "..")
            or request["credit_pool"] != "herd" or type(request["zap"]) is not bool):
        raise ValueError("invalid invoice attribution")
    raw = request["description"]
    if not isinstance(raw, str) or not raw:
        raise ValueError("missing description")
    attribution = {"user": request["user"], "credit_pool": request["credit_pool"]}
    if request["zap"]:
        attribution["zap_request"] = raw
    invoice = issue(store, client, token, request["request_id"], request["amount_sat"],
                    raw.encode(), keyset, attribution)
    return {"invoice": invoice}


def handle(connection, peer_uid, store, client, token, keyset):
    _, uid, _ = struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
    if uid != peer_uid:
        return
    connection.settimeout(10)
    with connection.makefile("rb") as reader:
        header = reader.read(4)
        if len(header) != 4:
            return
        size, = struct.unpack("!I", header)
        if not 0 < size <= MAX_FRAME:
            return
        body = reader.read(size)
        if len(body) != size:
            return
        try:
            reply = create(json.loads(body), store, client, token, keyset)
        except (KeyError, TypeError, ValueError, httpx.HTTPError):
            reply = {"error": "project invoice unavailable"}
        encoded = json.dumps(reply).encode()
        connection.sendall(struct.pack("!I", len(encoded)) + encoded)


def home_tls_name(request):
    request.extensions["sni_hostname"] = "mint-home.internal"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-directory", required=True, type=Path)
    parser.add_argument("--core-uid", required=True, type=int)
    parser.add_argument("--keyset", required=True)
    args = parser.parse_args()
    if int(os.environ.get("LISTEN_PID", 0)) != os.getpid() or os.environ.get("LISTEN_FDS") != "1":
        raise RuntimeError("one systemd listen socket required")
    if args.core_uid <= 0:
        raise ValueError("non-root core uid required")
    credentials = Path(os.environ["CREDENTIALS_DIRECTORY"])
    context = ssl.create_default_context(cafile=str(credentials / "home-ca.pem"))
    context.load_cert_chain(credentials / "client.pem", credentials / "client.key")
    token = (credentials / "project-quote-token").read_text().strip()
    store = ClaimStore(args.state_directory)
    with socket.socket(fileno=3) as listener, httpx.Client(
        base_url="https://10.8.0.6:8793", verify=context, timeout=15, trust_env=False,
        headers={"Host": "mint-home.internal:8793"}, event_hooks={"request": [home_tls_name]}
    ) as client:
        while True:
            connection, _ = listener.accept()
            with connection:
                try:
                    handle(connection, args.core_uid, store, client, token, args.keyset)
                except (TimeoutError, ConnectionError):
                    continue


if __name__ == "__main__":
    main()
