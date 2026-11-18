#!/usr/bin/env python3
"""Exercise the Cashu egress renderer and kernel policy in disposable network namespaces."""
from contextlib import ExitStack
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


WALLET, SIGNER = 61001, 61002
MINT, MINT6 = "1.1.1.11", "2606:4700:4700::a"
RELAY, DNS = "9.9.9.99", "8.8.8.19"
HOUSEHOLD, METADATA = "10.8.0.6", "169.254.169.254"
OTHER6 = "fd00::6"
LISTENER = """
import json, socket, sys, threading

def serve(listener):
    while True:
        if listener.type == socket.SOCK_DGRAM:
            data, peer = listener.recvfrom(32)
            listener.sendto(data, peer)
        else:
            connection, _ = listener.accept()
            with connection:
                connection.sendall(connection.recv(32))

for address, port, protocol in json.loads(sys.argv[1]):
    family = socket.AF_INET6 if ':' in address else socket.AF_INET
    sock = socket.socket(family, socket.SOCK_STREAM if protocol == 'tcp' else socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind((address, port))
    if protocol == 'tcp':
        sock.listen()
    threading.Thread(target=serve, args=(sock,), daemon=True).start()
print('ready', flush=True)
threading.Event().wait(90)
"""
CLIENT = """
import json, socket, sys

for address, port, protocol, expected in json.loads(sys.argv[1]):
    family = socket.AF_INET6 if ':' in address else socket.AF_INET
    kind = socket.SOCK_STREAM if protocol == 'tcp' else socket.SOCK_DGRAM
    try:
        with socket.socket(family, kind) as sock:
            sock.settimeout(.4)
            sock.connect((address, port))
            sock.sendall(b'qualification')
            allowed = sock.recv(32) == b'qualification'
    except OSError:
        allowed = False
    assert allowed == expected, (address, port, protocol, allowed)
"""


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, timeout=20, **kwargs)


def stop(process):
    process.terminate()
    process.wait(timeout=5)


def run_as(uid, cases):
    run("setpriv", f"--reuid={uid}", f"--regid={uid}", "--clear-groups",
        "--inh-caps=-all", "--ambient-caps=-all", "--bounding-set=-all", "--no-new-privs",
        sys.executable, "-B", "-c", CLIENT, json.dumps(cases))


def main():
    namespace = os.readlink("/proc/self/ns/net")
    if (os.geteuid() != 0 or namespace == os.readlink("/proc/1/ns/net")
            or [link["ifname"] for link in json.loads(run("ip", "-j", "link"))] != ["lo"]):
        raise SystemExit("requires root in a fresh network namespace with only loopback")
    renderer = Path(__file__).resolve().parents[1] / "scripts/render-cashu-egress.py"
    with tempfile.TemporaryDirectory() as temporary, ExitStack() as cleanup:
        review = Path(temporary) / "review"
        command = (sys.executable, str(renderer), str(review), "--wallet-uid", str(WALLET),
                   "--signer-uid", str(SIGNER), "--uplink", "pilot-wan", "--resolver", DNS,
                   "--resolver", "127.0.0.53", "--wallet-https", MINT, "--wallet-https", MINT6,
                   "--signer-https", RELAY)
        for forbidden in (HOUSEHOLD, METADATA, OTHER6, "127.0.0.53", "::ffff:8.8.8.8", "ff02::1",
                          "100.64.0.9", "2002::8", "8.8.8.0/24", '8.8.8.8"'):
            refused = subprocess.run([*command, "--wallet-https", forbidden],
                                     capture_output=True, timeout=5)
            assert refused.returncode and not review.exists(), forbidden
        run(*command)
        assert (review / "wallet.conf").read_text().splitlines() == [
            "[Service]", "IPAddressAllow=", f"IPAddressAllow={MINT}/32",
            f"IPAddressAllow={DNS}/32", "IPAddressAllow=127.0.0.53/32",
            f"IPAddressAllow={MINT6}/128",
        ]
        assert (review / "signer.conf").read_text().splitlines() == [
            "[Service]", "IPAddressAllow=", f"IPAddressAllow={DNS}/32",
            f"IPAddressAllow={RELAY}/32", "IPAddressAllow=127.0.0.53/32",
        ]

        peer = subprocess.Popen(["unshare", "--net", "--", "sleep", "100"])
        cleanup.callback(stop, peer)
        for _ in range(100):
            if os.readlink(f"/proc/{peer.pid}/ns/net") != namespace:
                break
            time.sleep(.02)
        else:
            raise RuntimeError("peer network namespace missing")
        remote = ["nsenter", "-t", str(peer.pid), "-n"]
        run("ip", "link", "set", "lo", "up")
        run(*remote, "ip", "link", "set", "lo", "up")
        for interface, other, subnet in (("pilot-wan", "fixture-wan", "100"), ("pilot-tunnel", "fixture-tun", "101")):
            run("ip", "link", "add", interface, "type", "veth", "peer", "name", other)
            run("ip", "link", "set", other, "netns", str(peer.pid))
            run("ip", "addr", "add", f"192.0.2.{subnet}/24", "dev", interface)
            run("ip", "link", "set", interface, "up")
            run(*remote, "ip", "link", "set", other, "up")
        # Source addresses on a genuinely different route allow the fixture to
        # prove that the kernel matches the interface as well as the destination.
        run(*remote, "ip", "addr", "add", "192.0.2.200/24", "dev", "fixture-wan")
        run(*remote, "ip", "addr", "add", "192.0.2.201/24", "dev", "fixture-tun")
        run("ip", "route", "replace", "192.0.2.201/32", "dev", "pilot-tunnel")
        run(*remote, "ip", "route", "replace", "192.0.2.101/32", "dev", "fixture-tun")
        for address in (MINT, RELAY, DNS, HOUSEHOLD, METADATA):
            run(*remote, "ip", "addr", "add", f"{address}/32", "dev", "lo")
            run("ip", "route", "add", f"{address}/32", "via", "192.0.2.200", "dev", "pilot-wan")
        run("ip", "-6", "addr", "add", "2001:db8::1/64", "dev", "pilot-wan", "nodad")
        run(*remote, "ip", "-6", "addr", "add", "2001:db8::2/64", "dev", "fixture-wan", "nodad")
        for address in (MINT6, OTHER6):
            run(*remote, "ip", "-6", "addr", "add", f"{address}/128", "dev", "lo", "nodad")
            run("ip", "-6", "route", "add", f"{address}/128", "via", "2001:db8::2", "dev", "pilot-wan")

        listeners = [(MINT, 443, "tcp"), (MINT, 80, "tcp"), (MINT6, 443, "tcp"),
                     (RELAY, 443, "tcp"), (DNS, 53, "udp"), (DNS, 53, "tcp"), (DNS, 443, "tcp"),
                     (HOUSEHOLD, 443, "tcp"), (METADATA, 443, "tcp"), (OTHER6, 443, "tcp")]
        local_listeners = [("127.0.0.53", 53, "udp"), ("127.0.0.53", 443, "tcp")]
        for prefix, bindings in ((remote, listeners), ([], local_listeners)):
            server = subprocess.Popen([*prefix, sys.executable, "-B", "-c", LISTENER, json.dumps(bindings)],
                                      text=True, stdout=subprocess.PIPE)
            cleanup.callback(stop, server)
            assert server.stdout.readline().strip() == "ready"
        all_listeners = listeners + local_listeners
        run_as(WALLET, [(*item, True) for item in all_listeners])
        policy = str(review / "cashu-egress.nft")
        run("nft", "--check", "-f", policy)
        for _ in range(2):
            run("nft", "-f", policy)
        for uid, web in ((WALLET, {MINT, MINT6}), (SIGNER, {RELAY})):
            permitted = {(address, 443, "tcp") for address in web}
            permitted.update(((DNS, 53, "udp"), (DNS, 53, "tcp"), ("127.0.0.53", 53, "udp")))
            run_as(uid, [(*item, item in permitted) for item in all_listeners])
        run("ip", "route", "replace", f"{MINT}/32", "via", "192.0.2.201", "dev", "pilot-tunnel", "src", "192.0.2.101")
        run_as(61003, [(MINT, 443, "tcp", True), (HOUSEHOLD, 443, "tcp", True)])
        run_as(WALLET, [(MINT, 443, "tcp", False)])
    print(json.dumps({"synthetic": True, "actual_nftables": True, "atomic_reload": True,
                      "ipv4_and_ipv6": True, "role_separation": True, "resolver_ports_only": True,
                      "household_metadata_and_other_interface_denied": True, "unrelated_uid_preserved": True}))


if __name__ == "__main__":
    main()
