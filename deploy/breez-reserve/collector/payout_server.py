"""Serve the selected wallet handoff or payout role on its own authenticated socket."""

import argparse
import json
import os
import pwd
import socket
from pathlib import Path

import httpx

from claim_store import ClaimStore
from collect import collect_acquisition, execute_payout
from invoice_server import handle_request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-directory", required=True, type=Path)
    parser.add_argument("--pinned-keysets", required=True, type=Path)
    parser.add_argument("--keyset", required=True)
    parser.add_argument("--role", choices=("payout", "acquisition"), default="payout")
    args = parser.parse_args()
    account = "cyberherd-payout" if args.role == "payout" else "lightning-goats-cashu"
    peer_uid = pwd.getpwnam(account).pw_uid
    if peer_uid <= 0 or int(os.environ.get("LISTEN_PID", 0)) != os.getpid() or os.environ.get("LISTEN_FDS") != "1":
        raise RuntimeError("payout worker identity and one systemd socket required")
    keys = json.loads(args.pinned_keysets.read_text())
    if args.keyset not in keys:
        raise ValueError("missing payout change keyset")
    store = ClaimStore(args.state_directory)
    with socket.socket(fileno=3) as listener, httpx.Client(base_url="https://mint.lightning-goats.com",
            timeout=15, follow_redirects=False, trust_env=False) as client:
        def dispatch(request):
            if args.role == "payout":
                return execute_payout(store, client, request, args.keyset, keys)
            if set(request) != {"acquisition_id", "evidence_digest", "proofs"}:
                raise ValueError("invalid acquisition handoff")
            collect_acquisition(store, client, request["acquisition_id"], request["evidence_digest"],
                                request["proofs"], args.keyset, keys)
            return {"acquisition_id":request["acquisition_id"], "evidence_digest":request["evidence_digest"]}

        while True:
            connection, _ = listener.accept()
            with connection:
                try:
                    handle_request(connection, peer_uid, dispatch, 1048576 if args.role == "acquisition" else 65536)
                except (TimeoutError, ConnectionError):
                    continue


if __name__ == "__main__":
    main()
