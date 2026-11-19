#!/usr/bin/env python3
"""Operator client for a receive-only Unix socket or the private mTLS edge."""

import argparse
import hashlib
import json
import ssl
from pathlib import Path
from uuid import UUID

import bolt11
import httpx


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--socket", type=Path)
    target.add_argument("--private-url", help="HTTPS private edge origin followed by /wallet")
    parser.add_argument("--ca")
    parser.add_argument("--cert")
    parser.add_argument("--key")
    parser.add_argument("--wallet", required=True, help="expected reserve public identity")
    parser.add_argument("--network", required=True, choices=("mainnet", "regtest"))
    commands = parser.add_subparsers(dest="action", required=True)
    commands.add_parser("health")
    create = commands.add_parser("create")
    create.add_argument("operation_id", type=UUID)
    create.add_argument("amount_sat", type=int)
    create.add_argument("--expiry", type=int, default=900)
    description = create.add_mutually_exclusive_group(required=True)
    description.add_argument("--memo")
    description.add_argument("--description-file", type=Path)
    create.add_argument("--claim-pubkey")
    status = commands.add_parser("status")
    status.add_argument("payment_hash")
    args = parser.parse_args()

    if args.socket:
        origin = "http://reserve"
        transport = httpx.HTTPTransport(uds=str(args.socket))
    else:
        if not args.private_url.startswith("https://") or not all((args.ca, args.cert, args.key)):
            parser.error("the private HTTPS edge requires an explicit CA, client certificate and key")
        origin = args.private_url.rstrip("/")
        context = ssl.create_default_context(cafile=args.ca)
        context.load_cert_chain(args.cert, args.key)
        transport = httpx.HTTPTransport(verify=context)

    with httpx.Client(transport=transport, timeout=15, trust_env=False) as client:
        response = client.get(f"{origin}/health").raise_for_status().json()
        if (response["wallet"], response["network"]) != (args.wallet, args.network):
            raise SystemExit("reserve wallet or network mismatch")
        if args.action == "health":
            assert client.get(f"{origin}/v1/funding/balance").status_code == 404
        elif args.action == "status":
            if len(args.payment_hash) != 64 or any(char not in "0123456789abcdef" for char in args.payment_hash):
                parser.error("payment hash must be 32 bytes encoded as lower-case hex")
            response = client.get(f"{origin}/v1/invoices/{args.payment_hash}").raise_for_status().json()
            if response["payment_hash"] != args.payment_hash:
                raise SystemExit("reserve returned a different payment hash")
        else:
            if args.description_file:
                if not args.claim_pubkey:
                    parser.error("--description-file requires --claim-pubkey")
                digest = hashlib.sha256(args.description_file.read_bytes()).hexdigest()
                desc = {"purpose": "project_quote", "description_hash": digest, "claim_pubkey": args.claim_pubkey}
            else:
                desc = {"purpose": "mint_quote", "memo": args.memo}
            response = client.post(f"{origin}/v1/invoices", json={
                "operation_id": str(args.operation_id), "amount_sat": args.amount_sat,
                "expiry_secs": args.expiry, "description": desc,
            }).raise_for_status().json()
            invoice = bolt11.decode(response["invoice"])
            if (response["wallet"], response["network"], response["operation_id"], response["payment_hash"], response["expires_at"]) != (
                args.wallet, args.network, str(args.operation_id), invoice.payment_hash, invoice.expiry_time
            ) or invoice.currency != {"mainnet": "bc", "regtest": "bcrt"}[args.network] or (
                int(invoice.amount_msat), invoice.expiry
            ) != (args.amount_sat * 1000, args.expiry):
                raise SystemExit("signed invoice fields differ from the original request")
            if args.description_file and invoice.description_hash != digest:
                raise SystemExit("signed invoice description hash differs from the input bytes")
            if not args.description_file and invoice.description != args.memo:
                raise SystemExit("signed invoice memo differs from the original request")
    print(json.dumps(response, indent=2))


if __name__ == "__main__":
    main()
