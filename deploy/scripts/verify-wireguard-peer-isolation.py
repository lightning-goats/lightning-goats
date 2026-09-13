#!/usr/bin/env python3
"""Isolated cryptokey-routing experiment; never touches existing interfaces."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


def run(*args, input=None, check=True):
    return subprocess.run(args, input=input, text=True, capture_output=True, check=check)


def main():
    assert os.geteuid() == 0, "Requires namespaces and synthetic WireGuard devices"
    prefix = f"lgck{os.getpid()}"
    ns = {name: f"{prefix}-{name}" for name in ("home", "hub", "stage")}
    created = []
    results = []

    def cmd(name, *args, **kwargs):
        return run("ip", "netns", "exec", ns[name], *args, **kwargs)

    def observed():
        data = json.loads(cmd("home", "nft", "-j", "list", "counter", "inet", "observe", "stage").stdout)
        return next(x["counter"]["packets"] for x in data["nftables"] if "counter" in x)

    def probe(label, sender, source, success, packets):
        before = observed()
        p = cmd(sender, "ping", "-n", "-c", "1", "-W", "2", "-I", source,
                "10.77.0.6", check=False)
        delta = observed() - before
        assert (p.returncode == 0) == success, f"{label}: unexpected ping exit {p.returncode}"
        assert delta == packets, f"{label}: home input observed {delta}, expected {packets}"
        results.append({"case": label, "echo_reply": success, "home_input_stage_packets": delta})

    try:
        with tempfile.TemporaryDirectory(prefix="lg-cryptokey-") as tmp:
            pubs = {}
            for name in ns:
                run("ip", "netns", "add", ns[name])
                created.append(ns[name])
                cmd(name, "ip", "link", "set", "lo", "up")
                cmd(name, "ip", "link", "add", "wgtest", "type", "wireguard")
                private = run("wg", "genkey").stdout.strip()
                pubs[name] = run("wg", "pubkey", input=private + "\n").stdout.strip()
                keyfile = Path(tmp) / name
                keyfile.write_text(private + "\n")
                keyfile.chmod(0o600)
                cmd(name, "wg", "set", "wgtest", "private-key", str(keyfile), "listen-port", "51820")
                del private

            for peer, hostip, peerip in (("hub", "192.0.2.1", "192.0.2.2"),
                                         ("stage", "192.0.2.5", "192.0.2.6")):
                # Create both veth ends inside the new home namespace, never the host.
                cmd("home", "ip", "link", "add", f"to-{peer}", "type", "veth", "peer", "name", "uplink")
                cmd("home", "ip", "link", "set", "uplink", "netns", ns[peer])
                cmd("home", "ip", "addr", "add", hostip + "/30", "dev", f"to-{peer}")
                cmd("home", "ip", "link", "set", f"to-{peer}", "up")
                cmd(peer, "ip", "addr", "add", peerip + "/30", "dev", "uplink")
                cmd(peer, "ip", "link", "set", "uplink", "up")
                cmd(peer, "wg", "set", "wgtest", "peer", pubs["home"],
                    "allowed-ips", "10.77.0.6/32", "endpoint", hostip + ":51820")

            for name, suffix in (("home", 6), ("hub", 1), ("stage", 12)):
                cmd(name, "ip", "addr", "add", f"10.77.0.{suffix}/32", "dev", "wgtest")
                cmd(name, "ip", "link", "set", "wgtest", "up")
                cmd(name, "ip", "route", "add", "10.77.0.0/24", "dev", "wgtest")
            cmd("hub", "ip", "addr", "add", "10.77.0.12/32", "dev", "wgtest")
            cmd("home", "wg", "set", "wgtest", "peer", pubs["hub"],
                "allowed-ips", "10.77.0.0/24", "endpoint", "192.0.2.2:51820")
            cmd("home", "nft", "-f", "-", input='''table inet observe {
                counter stage { }
                chain input {
                    type filter hook input priority 0; policy accept;
                    iifname "wgtest" ip saddr 10.77.0.12 icmp type echo-request counter name stage
                }
            }
            ''')
            probe("legacy_hub_before", "hub", "10.77.0.1", True, 0)
            probe("spoof_before_specific_peer", "hub", "10.77.0.12", True, 1)
            cmd("home", "wg", "set", "wgtest", "peer", pubs["stage"],
                "allowed-ips", "10.77.0.12/32", "endpoint", "192.0.2.6:51820")
            probe("spoof_after_specific_peer", "hub", "10.77.0.12", False, 0)
            probe("authenticated_staging", "stage", "10.77.0.12", True, 1)
            probe("legacy_hub_preserved", "hub", "10.77.0.1", True, 0)
            cmd("home", "wg", "set", "wgtest", "peer", pubs["stage"], "remove")
            probe("spoof_after_peer_rollback", "hub", "10.77.0.12", True, 1)
            print(json.dumps({"classification": "isolated Linux WireGuard lab, synthetic keys and addresses",
                              "observed_at_unix": int(time.time()), "kernel": os.uname().release,
                              "cases": results, "live_network_acceptance": False}, indent=2))
    finally:
        for name in reversed(created):
            run("ip", "netns", "del", name)


if __name__ == "__main__":
    main()
