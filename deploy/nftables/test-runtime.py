#!/usr/bin/env python3
"""Test service-only firewall in a fresh loopback-only network namespace.

Run: sudo unshare --net -- python3 -B deploy/nftables/test-runtime.py
No host firewall, service, WireGuard peer or provider is contacted.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import pwd
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--display", action="store_true", help="test the separate display-writer policy")
args = parser.parse_args()

if os.geteuid() != 0 or os.readlink("/proc/self/ns/net") == os.readlink("/proc/1/ns/net"):
    raise SystemExit("requires root in a fresh network namespace")
links = json.loads(subprocess.check_output(["ip", "-j", "link", "show"]))
if [link["ifname"] for link in links] != ["lo"]:
    raise SystemExit("requires a namespace containing only loopback")
subprocess.run(["ip", "link", "set", "lo", "up"], check=True)
for address in ("10.8.0.6/32", "10.8.0.7/32", "203.0.113.1/32"):
    subprocess.run(["ip", "addr", "add", address, "dev", "lo"], check=True)
subprocess.run(["ip", "-6", "addr", "add", "fd00::6/128", "dev", "lo"], check=True)
runtime = pwd.getpwnam("lightning-goats")
assert runtime.pw_uid != 0
table = "lightning_goats_display" if args.display else "lightning_goats_runtime"
filename = "lightning-goats-display.nft.example" if args.display else "lightning-goats-runtime.nft.example"
policy = Path(__file__).with_name(filename).read_text().replace("@UID@", str(runtime.pw_uid))
with tempfile.NamedTemporaryFile(mode="w") as file:
    file.write(policy); file.flush()
    for _ in range(2):
        subprocess.run(["nft", "-f", file.name], check=True)
    rules = json.loads(subprocess.check_output(["nft", "-j", "list", "table", "inet", table]))
    assert sum("rule" in item for item in rules["nftables"]) == (7 if args.display else 9)


class Backend(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        self.server.calls += 1
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")


servers = []
for host, port in (("10.8.0.6", 8789), ("10.8.0.6", 8080), ("10.8.0.6", 5000),
                   ("10.8.0.7", 8789), ("127.0.0.1", 5432), ("203.0.113.1", 443)):
    server = ThreadingHTTPServer((host, port), Backend)
    server.calls = 0
    threading.Thread(target=server.serve_forever, daemon=True).start()
    servers.append(server)
class IPv6Backend(ThreadingHTTPServer):
    address_family = socket.AF_INET6
server = IPv6Backend(("fd00::6", 8080), Backend)
server.calls = 0
threading.Thread(target=server.serve_forever, daemon=True).start()
servers.append(server)

code = '''
import socket,urllib.request
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
for address in ['http://10.8.0.6:8789/healthz','http://203.0.113.1:443/']:
 with opener.open(address,timeout=2) as r: assert r.status==200
for host,port in [('10.8.0.6',8080),('10.8.0.6',5000),('10.8.0.7',8789),('127.0.0.1',5432),('fd00::6',8080)]:
 try: socket.create_connection((host,port),timeout=2)
 except OSError: pass
 else: raise AssertionError('private path was not blocked')
'''
as_runtime = ["setpriv", f"--reuid={runtime.pw_uid}", f"--regid={runtime.pw_gid}", "--clear-groups", "--no-new-privs", "--"]
subprocess.run([*as_runtime, "python3", "-c", code], check=True)
assert [s.calls for s in servers] == [1, 0, 0, 0, 0, 1, 0]
# Unrelated root/admin traffic is outside the service's policy.
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open("http://10.8.0.6:8080/", timeout=2) as response:
    assert response.status == 200
if not args.display:
    # nginx's root-side loopback request must receive a reply from runtime port 8787.
    server_code = '''
    from http.server import HTTPServer,BaseHTTPRequestHandler
    class Handler(BaseHTTPRequestHandler):
     def do_GET(self):
      self.send_response(200);self.end_headers();self.wfile.write(b'ok')
    HTTPServer(('127.0.0.1',8787),Handler).handle_request()
    '''
    child = subprocess.Popen([*as_runtime, "python3", "-c", server_code], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(40):
            try:
                with opener.open("http://127.0.0.1:8787/", timeout=1) as response:
                    assert response.status in (200, 404)
                break
            except ConnectionError:
                time.sleep(.05)
            except urllib.error.URLError:
                time.sleep(.05)
        else:
            raise AssertionError("service loopback replies failed")
        child.wait(timeout=3)
    finally:
        if child.poll() is None:
            child.kill(); child.wait()
for server in servers:
    server.shutdown(); server.server_close()
print("PASS: gateway/public egress, five private-path denials, unrelated traffic and atomic reload; " + ("display writer has no loopback backend allowance." if args.display else "runtime loopback replies accepted."))
