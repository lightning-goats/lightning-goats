#!/usr/bin/env python3
"""One-way local sync report. Never import this program into the bridge process.
Only get_info/get_height/get_address are sent to explicit loopback RPC endpoints.
The operator must verify MoneroPay uses this SAME dedicated wallet/account0/node.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

LIMIT = 65536

def strict_json(data):
    def pairs(items):
        out={}
        for key,value in items:
            if key in out: raise ValueError("duplicate JSON field")
            out[key]=value
        return out
    def constant(value): raise ValueError("nonfinite JSON value")
    return json.loads(data,object_pairs_hook=pairs,parse_constant=constant)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("RPC redirect refused")

def read_private(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as file:
        info = os.fstat(file.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o077 or info.st_uid != os.geteuid():
            raise ValueError("private configuration permissions")
        data = file.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise ValueError("private configuration too large")
    return strict_json(data)

def endpoint(url, path):
    u = urllib.parse.urlsplit(url)
    if u.scheme != "http" or u.hostname != "127.0.0.1" or u.username or u.password or u.query or u.fragment or u.path != path or not u.port or u.port == 5000:
        raise ValueError("only explicit local RPC endpoints are allowed")
    return url

def rpc(url, method, params, credentials=None):
    if method not in {"get_info", "get_height", "get_address"}:
        raise ValueError("non-read-only RPC refused")
    endpoint(url, "/json_rpc")
    handlers = [urllib.request.ProxyHandler({}), NoRedirect()]
    if credentials:
        p = urllib.request.HTTPPasswordMgrWithDefaultRealm()
        p.add_password(None, url, credentials["username"], credentials["password"])
        handlers.append(urllib.request.HTTPDigestAuthHandler(p))
    request = urllib.request.Request(url, json.dumps({"jsonrpc":"2.0", "id":"lg-sync", "method":method, "params":params}).encode(), {"Content-Type":"application/json"})
    with urllib.request.build_opener(*handlers).open(request, timeout=3) as response:
        if response.status != 200 or response.headers.get_content_type() != "application/json":
            raise ValueError("RPC response rejected")
        data = response.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise ValueError("RPC body limit")
    value = strict_json(data)
    if value.get("jsonrpc") != "2.0" or value.get("id") != "lg-sync" or "error" in value or not isinstance(value.get("result"), dict):
        raise ValueError("RPC envelope rejected")
    return value["result"]

def integer(v):
    return type(v) is int and 0 <= v <= (1 << 63) - 1

def inspect(c, call=rpc, clock=time.time):
    start = int(clock())
    if c["network"] not in {"mainnet", "testnet", "stagenet"} or not integer(c["max_wallet_lag"]) or c["max_wallet_lag"] > 10:
        raise ValueError("invalid synchronization policy")
    endpoint(c["provider_url"], "/")
    wallet = read_private(c["wallet_credentials_file"]) if c.get("wallet_credentials_file") else None
    daemon = read_private(c["daemon_credentials_file"]) if c.get("daemon_credentials_file") else None
    node = call(c["daemon_rpc_url"], "get_info", {}, daemon)
    address = call(c["wallet_rpc_url"], "get_address", {"account_index":0, "address_index":[0]}, wallet)
    height = call(c["wallet_rpc_url"], "get_height", {}, wallet).get("height")
    confirm = call(c["daemon_rpc_url"], "get_info", {}, daemon)
    for n in [node, confirm]:
        if n.get("status") != "OK" or n.get("nettype") != c["network"] or n.get("synchronized") is not True or n.get("offline") is not False or n.get("busy_syncing") is not False or n.get("untrusted") is not False:
            raise ValueError("node is not synchronized/trusted/online")
        if not integer(n.get("height")) or not integer(n.get("target_height")) or n["height"] == 0 or n["target_height"] > n["height"]:
            raise ValueError("node height incomplete")
        incoming=n.get("incoming_connections_count")
        outgoing=n.get("outgoing_connections_count")
        if not integer(incoming) or not integer(outgoing):
            raise ValueError("invalid peer counts")
        peers=incoming+outgoing
        if not integer(peers) or peers == 0:
            raise ValueError("no network peers")
    if not integer(height) or height == 0 or confirm["height"] < node["height"] or height > confirm["height"] or confirm["height"] - height > c["max_wallet_lag"]:
        raise ValueError("wallet height is not current")
    primary = address.get("address")
    if not isinstance(primary, str) or len(primary) != 95:
        raise ValueError("wallet identity missing")
    scope = hashlib.sha256(primary.encode("ascii")).hexdigest()
    if scope != c["account_scope"]:
        raise ValueError("wallet identity mismatch")
    end = int(clock())
    if end < start or end-start > 20:
        raise ValueError("sync sample clock or duration invalid")
    return {"version":"monero-sync-v1", "network":c["network"], "account_scope":scope, "provider_url":c["provider_url"], "checked_at":start, "wallet_height":height, "daemon_height":confirm["height"], "synchronized":True}

def write_report(path, report):
    parent = Path(path).parent
    info = parent.stat()
    if info.st_uid != os.geteuid() or info.st_mode & 0o022:
        raise ValueError("unprotected report directory")
    fd, temp = tempfile.mkstemp(prefix=".sync-", dir=parent)
    try:
        with os.fdopen(fd, "w") as file:
            os.fchmod(file.fileno(), 0o640)
            json.dump(report, file, sort_keys=True)
            file.flush(); os.fsync(file.fileno())
        os.replace(temp, path)
        dfd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
        try: os.fsync(dfd)
        finally: os.close(dfd)
    finally:
        if os.path.exists(temp): os.unlink(temp)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    args = parser.parse_args()
    try:
        c = read_private(args.config)
        try:
            report = inspect(c)
        except Exception:
            report = {"version":"monero-sync-v1", "network":c["network"], "account_scope":c["account_scope"], "provider_url":c["provider_url"], "checked_at":int(time.time()), "wallet_height":0, "daemon_height":0, "synchronized":False}
        write_report(c["report_file"], report)
        if not report["synchronized"]:
            raise ValueError("synchronization unavailable")
    except Exception:
        # Never leak RPC credentials, URLs, address, response body or stack locals.
        parser.exit(1, "Monero sync report unavailable; bridge remains fail-closed.\n")
if __name__ == "__main__": main()
