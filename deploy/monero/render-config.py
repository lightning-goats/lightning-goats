#!/usr/bin/env python3
"""Render only non-secret INACTIVE files; never contacts or changes a server."""
import argparse
import ipaddress
import json
import re
from pathlib import Path
import tomllib

def render(values, output):
    required={"account_scope","network","bridge_uid","home_ipv4","vps_ipv4","bridge_port","callback_port","provider_port","wallet_port","daemon_port","mtls_port","max_amount_atomic","server_cert","server_key","client_ca"}
    if set(values)!=required: raise ValueError("exact deployment fields required")
    if not re.fullmatch("[0-9a-f]{64}",values["account_scope"]):raise ValueError("account fingerprint required")
    if values["network"] not in {"mainnet","testnet","stagenet"}:raise ValueError("network")
    if type(values["bridge_uid"]) is not int or not 1<=values["bridge_uid"]<=2**31-1:raise ValueError("non-root bridge uid")
    if type(values["max_amount_atomic"]) is not int or not 1<=values["max_amount_atomic"]<=2**63-1:raise ValueError("explicit amount bound")
    ports=[values[k] for k in required if k.endswith("_port")]
    if any(type(p) is not int or not 1024<=p<=65535 or p==5000 for p in ports) or len(set(ports))!=len(ports):raise ValueError("distinct unprivileged ports, never weather5000")
    for key in ["home_ipv4","vps_ipv4"]:
        ip=ipaddress.IPv4Address(values[key])
        if ip not in ipaddress.IPv4Network("10.8.0.0/24") or ip.packed[-1] in {0,255}:raise ValueError("review a different VPN range explicitly")
    if values["home_ipv4"]==values["vps_ipv4"]:raise ValueError("distinct host identities")
    for key in ["server_cert","server_key","client_ca"]:
        if not re.fullmatch(r"/[A-Za-z0-9_./-]+",values[key]) or ".." in Path(values[key]).parts:raise ValueError("safe absolute TLS path required")
    src=Path(__file__).resolve().parent
    c=tomllib.loads((src/"config.toml.example").read_text())
    c.update(network=values["network"],account_scope=values["account_scope"],listen=f'127.0.0.1:{values["bridge_port"]}',callback_listen=f'127.0.0.1:{values["callback_port"]}',provider_url=f'http://127.0.0.1:{values["provider_port"]}/',max_amount_atomic=values["max_amount_atomic"])
    text="\n".join(f'{key} = {json.dumps(value)}' for key,value in c.items())+"\n"
    # JSON scalars are valid TOML for these explicit strings/integers/booleans.
    assert tomllib.loads(text)["create_enabled"] is False
    sync=json.loads((src/"sync.json.example").read_text())
    sync.update(network=c["network"],account_scope=c["account_scope"],provider_url=c["provider_url"],wallet_rpc_url=f'http://127.0.0.1:{values["wallet_port"]}/json_rpc',daemon_rpc_url=f'http://127.0.0.1:{values["daemon_port"]}/json_rpc')
    substitutions={"HOME_WG_IPV4":values["home_ipv4"],"VPS_WG_IPV4":values["vps_ipv4"],"MTLS_PORT":values["mtls_port"],"SERVER_CERT":values["server_cert"],"SERVER_KEY":values["server_key"],"PROJECT_CLIENT_CA":values["client_ca"],"BRIDGE_PORT":values["bridge_port"],"CALLBACK_PORT":values["callback_port"],"BRIDGE_UID":values["bridge_uid"],"PROVIDER_PORT":values["provider_port"]}
    files={"config.toml":text,"sync.json":json.dumps(sync,indent=2)+"\n"}
    for source,target in [("nginx-mtls.conf.example","nginx-mtls.conf"),("monero-egress.nft.example","monero-egress.nft")]:
        data=(src/source).read_text()
        for key,value in substitutions.items():data=data.replace("__"+key+"__",str(value))
        if re.search(r"__[A-Z_]+__",data):raise ValueError("unresolved placeholder")
        files[target]=data
    output=Path(output);output.mkdir(mode=0o700)
    for name,data in files.items():
        p=output/name;p.write_text(data);p.chmod(0o600)
    return {"files":len(files),"create_enabled":False,"activated":False}

if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__);p.add_argument("values_json");p.add_argument("output");a=p.parse_args()
    try:print(json.dumps(render(json.loads(Path(a.values_json).read_text()),a.output)))
    except Exception:p.exit(1,"Invalid deployment values or existing output; no host change performed.\n")
