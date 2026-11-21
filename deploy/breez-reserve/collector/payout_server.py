"""Receive authorized CyberHerd payouts on the payout-only systemd socket."""

import argparse
import json
import os
import pwd
import socket
from pathlib import Path

import httpx

from claim_store import ClaimStore
from collect import execute_payout
from invoice_server import handle_request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-directory", required=True, type=Path)
    parser.add_argument("--pinned-keysets", required=True, type=Path)
    parser.add_argument("--keyset", required=True)
    args = parser.parse_args()
    peer_uid = pwd.getpwnam("cyberherd-payout").pw_uid
    if peer_uid <= 0 or int(os.environ.get("LISTEN_PID", 0)) != os.getpid() or os.environ.get("LISTEN_FDS") != "1":
        raise RuntimeError("payout worker identity and one systemd socket required")
    keys = json.loads(args.pinned_keysets.read_text())
    if args.keyset not in keys:
        raise ValueError("missing payout change keyset")
    store = ClaimStore(args.state_directory)
    with socket.socket(fileno=3) as listener, httpx.Client(base_url="https://mint.lightning-goats.com",
            timeout=15, follow_redirects=False, trust_env=False) as client:
        def dispatch(request):
            return execute_payout(store, client, request, args.keyset, keys)

        while True:
            connection, _ = listener.accept()
            with connection:
                try:
                    handle_request(connection, peer_uid, dispatch)
                except (TimeoutError, ConnectionError):
                    continue


if __name__ == "__main__":
    main()
