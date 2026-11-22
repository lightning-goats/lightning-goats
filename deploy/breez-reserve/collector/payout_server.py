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
from operator_ecash import transfer


def dispatch_payout(store, client, request, keyset, keys):
    if set(request) == {"operator_ecash"}:
        return transfer(store, client, request["operator_ecash"], keyset, keys)
    return execute_payout(store, client, request, keyset, keys)


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
                return dispatch_payout(store, client, request, args.keyset, keys)
            fields = {"acquisition_id", "evidence_digest", "proofs"}
            report_amount = set(request) == fields | {"report_collected_sats"} and request["report_collected_sats"] is True
            if set(request) != fields and not report_amount:
                raise ValueError("invalid acquisition handoff")
            collected = collect_acquisition(store, client, request["acquisition_id"], request["evidence_digest"],
                                            request["proofs"], args.keyset, keys)
            result = {"acquisition_id": request["acquisition_id"], "evidence_digest": request["evidence_digest"]}
            if report_amount:
                result["collected_sats"] = collected
            return result

        while True:
            connection, _ = listener.accept()
            with connection:
                try:
                    handle_request(connection, peer_uid, dispatch, 1048576 if args.role == "acquisition" else 65536)
                except (TimeoutError, ConnectionError):
                    continue


if __name__ == "__main__":
    main()
