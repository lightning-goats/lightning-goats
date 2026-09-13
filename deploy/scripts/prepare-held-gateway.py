#!/usr/bin/env python3
"""Fresh inactive held gateway, loopback only. Reuses guarded HOME installer.

The separate Unix user/state shares only the existing canary OpenHAB USER
credential. No claim of distinct OpenHAB principals; no credential is exported.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(Path(__file__).resolve().parent))
import home_gateway_safety as safety
spec=importlib.util.spec_from_file_location('installer',Path(__file__).with_name('prepare-home-gateway.py'))
installer=importlib.util.module_from_spec(spec);spec.loader.exec_module(installer)
STEM='lightning-goats-held-canary'
installer.USERS=(STEM,)
CREDENTIAL=Path('/etc/credstore.encrypted/lightning-goats-gateway-canary-openhab')


def files(binary):
    config=(ROOT/'deploy/gateway/config.held-canary.toml.example').read_bytes()
    parsed=tomllib.loads(config.decode())
    expected={'url':'http://127.0.0.1:8080/','request_item':'LightningGoatsHeldCanary2Request',
              'ack_item':'LightningGoatsHeldCanary2Ack','protocol':'uuid_held_canary',
              'override_item':'LightningGoatsHeldCanary2Override','remote_enabled_item':'LightningGoatsHeldCanary2RemoteEnabled'}
    if parsed['openhab'] != expected or parsed['service']!={'listen':'127.0.0.1:8791'} or parsed['database']!={'url':f'sqlite:///var/lib/{STEM}/gateway.db'}:
        raise ValueError('unexpected held gateway target configuration')
    return {f'/usr/local/bin/{STEM}':(binary,0o755),f'/etc/{STEM}/config.toml':(config,0o644),
            f'/etc/systemd/system/{STEM}.service':((ROOT/f'deploy/systemd/{STEM}.service').read_bytes(),0o644)}


def verify(generated, running=False):
    for name,(content,mode) in generated.items():
        path=Path(name);info=safety.root_regular(path)
        if info.st_mode&0o777!=mode or path.read_bytes()!=content:
            raise ValueError('installed held artifact mismatch')
    safety.root_regular(CREDENTIAL)
    props=safety.properties(STEM+'.service')
    expected={'FragmentPath':f'/etc/systemd/system/{STEM}.service','DropInPaths':'',
              'NeedDaemonReload':'no','User':STEM,'Group':STEM,'Environment':'RUST_LOG=info',
              'EnvironmentFiles':'','ActiveState':'active' if running else 'inactive'}
    if any(props.get(key,'')!=value for key,value in expected.items()):
        raise ValueError('effective held service drift')
    state=subprocess.run(['systemctl','is-enabled',STEM+'.service'],capture_output=True,text=True).stdout.strip()
    if state!='disabled':raise ValueError('held unit unexpectedly enabled')
    if running:
        pid=int(props['MainPID'])
        if pid<=1:raise ValueError('held service process absent')
        proc=Path('/proc')/str(pid)
        args=(proc/'cmdline').read_bytes().rstrip(b'\0').split(b'\0')
        if args!=[f'/usr/local/bin/{STEM}'.encode(),b'--config',f'/etc/{STEM}/config.toml'.encode()]:
            raise ValueError('held process command mismatch')
        executable=os.stat(proc/'exe');installed=os.stat('/usr/local/bin/'+STEM)
        if (executable.st_dev,executable.st_ino)!=(installed.st_dev,installed.st_ino):
            raise ValueError('held running executable mismatch')
        status=dict(line.split(':',1) for line in (proc/'status').read_text().splitlines() if ':' in line)
        uid=installer.pwd.getpwnam(STEM).pw_uid
        if set(status['Uid'].split())!={str(uid)} or int(status['CapEff'].strip(),16)!=0 or status['NoNewPrivs'].strip()!='1':
            raise ValueError('held runtime privilege mismatch')
    return {'verified':True,'running':running,'enabled_at_boot':False}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',required=True,type=Path)
    p.add_argument('--sha256',required=True)
    p.add_argument('--apply',action='store_true')
    p.add_argument('--verify',action='store_true')
    p.add_argument('--running',action='store_true')
    args=p.parse_args()
    if args.apply and args.verify:p.error("apply and verify are separate")
    if args.running and not args.verify:p.error("running requires verify")
    if args.binary.is_symlink() or not args.binary.is_file():raise ValueError('binary must be regular non-symlink')
    binary=args.binary.read_bytes()
    if hashlib.sha256(binary).hexdigest()!=args.sha256 or not binary.startswith(b'\x7fELF'):raise ValueError('binary identity mismatch')
    generated=files(binary)
    if args.apply:
        if os.geteuid()!=0:raise ValueError('apply requires root')
        safety.root_regular(CREDENTIAL)
        state=subprocess.run(['systemctl','is-enabled',STEM+'.service'],capture_output=True,text=True).stdout.strip()
        if state not in ('','not-found'):raise ValueError('existing unit enablement requires separate review')
        previous=os.umask(0o022)
        try:installer.install(generated)
        finally:os.umask(previous)
        verify(generated)
    if args.verify:
        print(json.dumps(verify(generated,args.running)))
        return
    print(json.dumps({'applied':args.apply,'services_started':False,'services_enabled':False,
                      'files':{name:hashlib.sha256(value[0]).hexdigest() for name,value in generated.items()}},indent=2))


if __name__=='__main__':main()
