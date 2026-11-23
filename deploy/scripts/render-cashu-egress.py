#!/usr/bin/env python3
"""Render an offline Cashu systemd and nftables egress policy from reviewed IPs."""
import argparse
import ipaddress
from pathlib import Path
import re


V6_GLOBAL_UNICAST = ipaddress.ip_network("2000::/3")
SPECIAL_USE = tuple(map(ipaddress.ip_network, (
    "192.0.0.0/24", "192.88.99.0/24", "2001::/23", "2002::/16", "3fff::/20",
)))
ROLES = ("wallet", "signer")


def addresses(values, *, resolvers=False):
    result = set()
    for value in values:
        address = ipaddress.ip_address(value)
        if "%" in value:
            raise ValueError("scoped addresses are prohibited")
        if resolvers and address.is_loopback:
            result.add(address)
            continue
        if (not address.is_global or address.is_multicast
                or (address.version == 6 and address not in V6_GLOBAL_UNICAST)
                or any(address in network for network in SPECIAL_USE)):
            raise ValueError("use public unicast destinations or an exact loopback DNS resolver")
        result.add(address)
    return sorted(result, key=lambda address: (address.version, int(address)))


def render(args):
    uids = {"wallet": args.wallet_uid, "signer": args.signer_uid}
    if len(set(uids.values())) != 2 or any(not 0 < uid < 2**32 - 1 for uid in uids.values()):
        raise ValueError("distinct non-root runtime UIDs are required")
    if not re.fullmatch(r"[A-Za-z0-9_.-]{1,15}", args.uplink) or args.uplink in ("lo", ".", ".."):
        raise ValueError("a concrete non-loopback uplink name is required")
    resolver = addresses(args.resolver, resolvers=True)
    endpoints = {
        "wallet": addresses(args.wallet_https),
        "signer": addresses(args.signer_https),
    }
    lines = [
        "destroy table inet lightning_goats_cashu_egress",
        "table inet lightning_goats_cashu_egress {",
        "    chain output {",
        "        type filter hook output priority -5; policy accept;",
    ]
    output = {}
    for role in ROLES:
        dns = resolver if endpoints[role] else []
        for address in dns:
            interface = "lo" if address.is_loopback else args.uplink
            family = "ip" if address.version == 4 else "ip6"
            lines.append(
                f'        meta skuid {uids[role]} oifname "{interface}" {family} daddr {address}'
                " meta l4proto { tcp, udp } th dport 53 accept"
            )
        for address in endpoints[role]:
            family = "ip" if address.version == 4 else "ip6"
            lines.append(
                f'        meta skuid {uids[role]} oifname "{args.uplink}" {family} daddr {address}'
                " tcp dport 443 accept"
            )
        allow = "".join(f"IPAddressAllow={address}/{address.max_prefixlen}\n"
                        for address in sorted(set(dns + endpoints[role]),
                                              key=lambda address: (address.version, int(address))))
        output[f"{role}.conf"] = f"[Service]\nIPAddressAllow=\n{allow}"
    lines.extend([f"        meta skuid {{ {uids['wallet']}, {uids['signer']} }} counter reject",
                  "    }", "}"])
    output["cashu-egress.nft"] = "\n".join(lines) + "\n"
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new review directory")
    parser.add_argument("--wallet-uid", type=int, required=True)
    parser.add_argument("--signer-uid", type=int, required=True)
    parser.add_argument("--uplink", required=True)
    parser.add_argument("--resolver", action="append", default=[])
    parser.add_argument("--wallet-https", action="append", default=[])
    parser.add_argument("--signer-https", action="append", default=[])
    args = parser.parse_args()
    try:
        files = render(args)
        args.destination.mkdir(mode=0o700)
        for name, data in files.items():
            with (args.destination / name).open("x") as stream:
                stream.write(data)
    except (ValueError, OSError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
