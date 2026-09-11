#!/usr/bin/python3
"""Offline subprocess fixture. No network and no cryptographic verification.

All event signatures/credentials are synthetic; this tests process and durable
outbox contracts, not real nak/NIP-46/relay acceptance.
"""
import hashlib
import json
import os
from pathlib import Path
import sys
import time

args = sys.argv[1:]
assert args[0] == "--config-path"
root = Path(args[1])
mode = (root / "mode").read_text().strip()
args = args[2:]
(root / "pid").write_text(str(os.getpid()))
if mode == "stall_before_stdin":
    time.sleep(30)
    sys.exit(0)
payload = json.loads(sys.stdin.read())
phase = "verify" if args[0] == "verify" else ("publish" if "--sec" in args else "sign")
with (root / "calls.jsonl").open("a") as stream:
    stream.write(json.dumps({"phase": phase, "payload": payload,
        "has_client_key": bool(os.environ.get("NOSTR_CLIENT_KEY")),
        "has_signer_uri": bool(os.environ.get("NOSTR_SECRET_KEY"))}) + "\n")
if mode == "nonzero_secret":
    sys.stderr.write(os.environ.get("NOSTR_CLIENT_KEY", "") + "\n")
    sys.exit(7)
if mode == "verify_failure" and phase == "verify":
    sys.exit(2)
if mode == "stderr_oversize":
    sys.stderr.write("e" * 20000)
if phase == "verify":
    sys.stdout.write("valid\n")
    sys.exit(0)
if phase == "publish":
    if mode == "publish_fail_once" and not (root / "failed-once").exists():
        (root / "failed-once").write_text("yes")
        sys.exit(3)
    if mode == "mutated_publish":
        payload["content"] = "different published event"
    print(json.dumps(payload))
    sys.exit(0)
# Simulate a signer echoing the exact requested message; not a real signature.
event = dict(payload)
event.update(pubkey="ab" * 32, created_at=1700000000, sig="02" * 64)
if mode == "mutated_content":
    event["content"] = "wrong message"
if mode == "mutated_tags":
    event["tags"] = [["t", "wrong-tag"]]
if mode == "stdout_oversize":
    event["content"] = "x" * 70000
if mode == "malformed_secret":
    event["created_at"] = os.environ.get("NOSTR_CLIENT_KEY", "")
event["id"] = hashlib.sha256(json.dumps(event, sort_keys=True).encode()).hexdigest()
print(json.dumps(event))
