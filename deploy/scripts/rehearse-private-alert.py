#!/usr/bin/env python3
"""Disposable encrypted-credential alert-unit rehearsal; synthetic I/O only."""
import sys
sys.dont_write_bytecode = True
import argparse
from contextlib import ExitStack
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import threading
import time
import uuid

SPEC = importlib.util.spec_from_file_location('sandbox', Path(__file__).with_name('rehearse-systemd.py'))
SANDBOX = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SANDBOX)
INSTALL = SANDBOX.INSTALL

PROBE = r'''
import json,os,stat,sys
from pathlib import Path
state,code,source,namespace,names,other_unit = sys.argv[1:]
credentials=Path(os.environ['CREDENTIALS_DIRECTORY'])
assert sorted(p.name for p in credentials.iterdir()) == sorted(names.split(','))
for p in credentials.iterdir():
 m=p.stat()
 assert stat.S_IMODE(m.st_mode)==0o400 and m.st_uid==os.geteuid()
 assert p.is_file() and p.read_bytes() and not os.access(p,os.W_OK)
assert os.geteuid()!=0 and os.getegid()!=0
assert os.access(state,os.W_OK)
assert not os.access(code,os.W_OK)
assert not os.access(source,os.R_OK)
assert not os.access(Path('/run/credentials')/other_unit,os.R_OK)
assert os.readlink('/proc/self/ns/net')==namespace
fields=dict(l.split(':',1) for l in Path('/proc/self/status').read_text().splitlines() if ':' in l)
assert fields['NoNewPrivs'].strip()=='1' and int(fields['CapEff'].strip(),16)==0
Path(state,'probe.json').write_text(json.dumps({'credential_modes_owner_verified':True,'source_inaccessible':True,'other_active_unit_credentials_inaccessible':True,'code_nonwritable':True,'nonroot_no_caps':True,'isolated_network':True}))
'''


def rehearse(archive, source):
    INSTALL.require_isolation()
    user = INSTALL.identity('daemon')  # temporary substitute, never create/adopt production accounts
    key = Path('/var/lib/systemd/credential.secret')
    if not key.is_file() or key.is_symlink() or key.stat().st_uid != 0 or key.stat().st_mode & 0o077:
        raise ValueError('protected staging credential store must already be initialized')
    prefix = 'lg-alert-rehearsal-' + uuid.uuid4().hex
    units=[]
    def stop_and_collect(process):
        process.terminate()
        SANDBOX.command(['systemctl','reset-failed',process.unit],check=False)
        for _ in range(50):
            if SANDBOX.command(['systemctl','show',process.unit,'--property=LoadState','--value'],check=False).stdout.strip()=='not-found': return
            time.sleep(.1)
        raise ValueError('temporary alert unit was not collected')
    with tempfile.TemporaryDirectory(prefix=prefix,dir='/run') as directory, ExitStack() as cleanup:
        root=Path(directory); root.chmod(0o755)
        cleanup.callback(lambda: print((root/'unit.log').read_text()[-8192:],file=sys.stderr) if (root/'unit.log').exists() else None)
        package=root/'package'; package.mkdir(); package.chmod(0o755)
        INSTALL.RELEASE.verify_archive(archive,source,package)
        binary=package/'lightning-goatsctl'
        binary.chmod(0o755)
        template=package/'deploy/systemd/lightning-goats-private-alert.service'
        state=Path('/var/lib')/prefix
        if state.exists(): raise ValueError('unexpected existing state')
        cleanup.callback(lambda: shutil.rmtree(state,ignore_errors=True))
        plain=root/'plain'; plain.mkdir(mode=0o700)
        encrypted=root/'encrypted'; encrypted.mkdir(mode=0o700)
        probe=root/'probe.py'; INSTALL.root_file(probe,PROBE)
        stub=root/'nak-stub'
        fixture=Path(__file__).resolve().parents[2]/'tests/fixtures/private_nak_stub.py'
        INSTALL.root_file(stub,fixture.read_text(),mode=0o755)
        # Match executable destination labels on disposable files only.
        if Path('/sys/fs/selinux/enforce').exists():
            for path in [binary,stub]:
                label=SANDBOX.command(['matchpathcon','-n','/usr/local/bin/'+path.name]).stdout.strip()
                SANDBOX.command(['chcon',label,str(path)])
        reads=[]
        class Provider(BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path!='/v1/balances' or self.headers.get('Authorization')!='Bearer synthetic-balance':
                    self.send_error(403); return
                reads.append(self.path)
                body=b'[{"currency":"BTC","current":"0.00000100"}]'
                self.send_response(200); self.send_header('Content-Length',str(len(body))); self.end_headers(); self.wfile.write(body)
            def log_message(self,*args): pass
        server=ThreadingHTTPServer(('127.0.0.1',0),Provider)
        cleanup.callback(server.server_close); cleanup.callback(server.shutdown)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        runtime={'database_url':f'sqlite://{state}/alerts.db','strike_api_url':f'http://127.0.0.1:{server.server_port}/v1/',
                 'nak_path':str(stub),'nak_config_path':str(state/'nak'),'bunker_pubkey':'ab'*32,'bunker_relays':['wss://signer.invalid']}
        values={'private-alert-policy':json.dumps({'threshold_sats':100,'recipient':'ef'*32,'inbox_relays':['wss://inbox.invalid'],'account_binding':'synthetic'}),
                'private-alert-runtime':json.dumps(runtime),'private-alert-strike-key':'synthetic-balance','private-alert-nostr-key':'synthetic-nip46'}
        for name,value in values.items():
            p=plain/name; INSTALL.root_file(p,value,mode=0o600)
            SANDBOX.command(['systemd-creds','encrypt','--with-key=host',f'--name={name}',str(p),str(encrypted/name)])
            (encrypted/name).chmod(0o600)
        shutil.rmtree(plain) # all subsequent reads must use decrypted credentials
        other_user=INSTALL.identity('nobody')
        if other_user.pw_uid==user.pw_uid or other_user.pw_gid==user.pw_gid: raise ValueError('distinct fixture identities required')
        foreign=root/'foreign'; INSTALL.root_file(foreign,'synthetic-other-unit',mode=0o600)
        SANDBOX.command(['systemd-creds','encrypt','--with-key=host','--name=foreign',str(foreign),str(encrypted/'foreign')])
        foreign.unlink()
        other_unit=f'{prefix}-other.service'
        other=SANDBOX.UnitProcess(other_unit); units.append(other_unit)
        cleanup.callback(stop_and_collect,other)
        SANDBOX.command(['systemd-run','--quiet','--no-ask-password',f'--unit={other_unit}',
                         f'--property=User={other_user.pw_name}',f'--property=Group={other_user.pw_gid}',
                         f'--property=LoadCredentialEncrypted=foreign:{encrypted/"foreign"}',
                         f'--property=NetworkNamespacePath=/proc/{os.getpid()}/ns/net',
                         '--property=NoNewPrivileges=yes','--','/usr/bin/sleep','180'])
        for _ in range(50):
            if other.properties().get('ActiveState')=='active': break
            time.sleep(.1)
        else: raise ValueError('other credential fixture did not start')
        preserved=[]
        def launch(action,index):
            unit=f'{prefix}-{index}.service'; units.append(unit)
            process=SANDBOX.UnitProcess(unit)
            cleanup.callback(stop_and_collect,process)
            names=sorted(values if action=='run' else ['private-alert-policy','private-alert-runtime'])
            changes={'User':user.pw_name,'Group':str(user.pw_gid),'StateDirectory':prefix,'RuntimeDirectory':prefix}
            if action!='run': changes.update({'Type':'oneshot','Restart':'no'})
            props=[]
            for name,value in SANDBOX.service_properties(template):
                if name=='ExecStart': continue
                if name=='LoadCredentialEncrypted':
                    cred=value.split(':',1)[0]
                    if cred not in names: continue
                    value=f'{cred}:{encrypted/cred}'
                elif name in changes: value=changes[name]
                else: preserved.append(f'{name}={value}')
                props.append((name,value))
            props += [('NetworkNamespacePath',f'/proc/{os.getpid()}/ns/net'),('CollectMode','inactive'),
                      ('ExecStartPre',' '.join(['/usr/bin/python3',str(probe),str(state),str(binary),str(encrypted),os.readlink('/proc/self/ns/net'),','.join(names),other_unit])),
                      ('StandardOutput',f'append:{root}/unit.log'),('StandardError',f'append:{root}/unit.log')]
            # Offline preparation needs a terminal status to inspect; runtime keeps
            # the shipped Type/Restart and automatic restarts are explicitly checked.
            props += [('RemainAfterExit','yes')]  # retain successful exit status until explicit stop
            args=['systemd-run','--quiet','--no-ask-password',f'--unit={unit}']
            args += [f'--property={k}={v}' for k,v in props]
            SANDBOX.command([*args,'--',str(binary),'private-alert',action])
            return process
        for action in ['initialize','check']:
            p=launch(action,action)
            deadline=time.monotonic()+15
            while p.properties().get('ActiveState')=='activating' and time.monotonic()<deadline: time.sleep(.1)
            status=p.properties()
            if status.get('ActiveState')!='active' or status.get('ExecMainStatus')!='0':
                raise ValueError(f'offline {action} failed: {status}')
            p.terminate()
        if reads: raise ValueError('offline preparation contacted provider')
        nak=state/'nak'; INSTALL.private_directory(nak,user)
        INSTALL.root_file(nak/'mode','success',group=user.pw_gid,mode=0o640)
        def calls():
            p=nak/'calls.jsonl'
            return [json.loads(x) for x in p.read_text().splitlines()] if p.exists() else []
        probes=[]
        for index in [0,1]:
            before=len(reads); p=launch('run',str(index)); deadline=time.monotonic()+30
            while time.monotonic()<deadline:
                status=p.properties()
                if status.get('NRestarts')!='0' or status.get('ActiveState') not in ['active','activating']:
                    raise ValueError(f'worker exited/restarted: {status}')
                if (index==0 and any(c['phase']=='publish' for c in calls())) or (index==1 and len(reads)>before): break
                time.sleep(.1)
            else: raise ValueError('synthetic observation/publication timed out')
            if other.properties().get('ActiveState')!='active': raise ValueError('other credential fixture not active during probe')
            probes.append(json.loads((state/'probe.json').read_text()))
            SANDBOX.command(['systemctl','kill','--kill-whom=main','--signal=TERM',p.unit])
            deadline=time.monotonic()+10
            while p.pid and time.monotonic()<deadline: time.sleep(.1)
            status=p.properties()
            if p.pid or status.get('ExecMainStatus')!='0' or status.get('NRestarts')!='0':
                raise ValueError('unclean worker shutdown')
            p.terminate()
        recorded=calls()
        if sum(c['phase']=='wrap' for c in recorded)!=1 or sum(c['phase']=='publish' for c in recorded)!=1:
            raise ValueError('expected one exact high-episode wrapper/publication across restart')
        for unit in units:
            SANDBOX.command(['systemctl','stop',unit],check=False)
            SANDBOX.command(['systemctl','reset-failed',unit],check=False)
        evidence={'source':source,'template_sha256':hashlib.sha256(template.read_bytes()).hexdigest(),
                  'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'synthetic_fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),
                  'preserved_properties':sorted(set(preserved)),'sandbox_probes':probes,'balance_gets':len(reads),
                  'wrapper_count':1,'publication_count':1,'restart_preserved_episode':True,'offline_no_provider_calls':True,
                  'credentials_encrypted_and_plaintext_removed':True,'limitations':['synthetic non-cryptographic nak fixture','temporary daemon identity substitutes for final alert account','RemainAfterExit retains status for controlled shutdown inspection','loopback-only namespace is not final egress acceptance']}
    evidence['temporary_files_removed_and_units_collected']=True
    return evidence

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive',type=Path); parser.add_argument('source')
    args=parser.parse_args()
    print(json.dumps(rehearse(args.archive,args.source),indent=2,sort_keys=True))
