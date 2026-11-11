#!/usr/bin/env python3
"""Incident containment: with core stopped, preserve its config and select shadow.

Suspend the optional contact route, whose configuration requires active mode.
No database, credential or gateway operation. Never restore active mode here.
Install as a root-owned non-writable file before running as root.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import time
import tomllib


def main():
    own = Path(__file__).resolve().stat()
    if os.geteuid() != 0 or own.st_uid != 0 or own.st_mode & 0o022:
        raise SystemExit('requires a protected root-owned installed script')
    unit = 'lightning-goats-receive.service'
    active = subprocess.check_output(['systemctl','show',unit,'-p','ActiveState','--value'],text=True).strip()
    pid = subprocess.check_output(['systemctl','show',unit,'-p','MainPID','--value'],text=True).strip()
    if active != 'inactive' or pid != '0':
        raise SystemExit('core must already be stopped; no running configuration mutation')
    path = Path('/etc/lightning-goats/config.receive.toml')
    old_stat = path.lstat()
    if not stat.S_ISREG(old_stat.st_mode) or old_stat.st_uid != 0 or old_stat.st_mode & 0o022:
        raise SystemExit('unexpected configuration ownership/type')
    original = path.read_bytes()
    before = tomllib.loads(original.decode())
    if before['service']['mode'] not in ('active','shadow'):
        raise SystemExit('expected active or partially contained shadow mode')
    if before['service']['mode']=='shadow' and 'contact' not in before:
        raise SystemExit('already contained; no implicit second mutation')
    lines = original.decode().splitlines(keepends=True)
    service = False
    changed = 0
    contact = False
    for i,line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith('['):
            service = stripped == '[service]'
            contact = stripped == '[contact]' or stripped.startswith('[contact.')
        if contact:
            lines[i]=''
            continue
        if service and re.fullmatch(r'\s*mode\s*=\s*"active"\s*(?:#.*)?',line.rstrip('\r\n')):
            lines[i] = line.replace('"active"','"shadow"',1)
            changed += 1
    candidate = ''.join(lines).encode()
    expected = dict(before)
    expected['service'] = dict(before['service'],mode='shadow')
    expected.pop('contact',None)
    expected_changes=1 if before['service']['mode']=='active' else 0
    if changed != expected_changes or tomllib.loads(candidate.decode()) != expected:
        raise SystemExit('candidate must only select shadow and suspend incompatible contact')
    base = Path('/var/lib/lightning-goats-upgrades')
    bs = base.lstat()
    if not stat.S_ISDIR(bs.st_mode) or bs.st_uid != 0 or bs.st_mode & 0o022:
        raise SystemExit('unexpected protected review base')
    review = base / ('feeder-cap-incident-' + time.strftime('%Y%m%dT%H%M%SZ',time.gmtime()))
    review.mkdir(mode=0o700)
    def write(destination,data,mode):
        fd = os.open(destination,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,mode)
        with os.fdopen(fd,'wb') as output:
            output.write(data);output.flush();os.fsync(output.fileno())
    write(review/'config.original.toml',original,0o600)
    write(review/'config.shadow.candidate.toml',candidate,0o600)
    receipt = {'incident':'per-payment-three-feeding-cap','unit':unit,'configuration_only':True,
               'original_sha256':hashlib.sha256(original).hexdigest(),
               'candidate_sha256':hashlib.sha256(candidate).hexdigest(),
               'mode':'shadow','contact_suspended':'contact' in before,'journals_opened':False,'provider_operations':0,'gateway_operations':0}
    write(review/'PREPARED.json',(json.dumps(receipt,sort_keys=True)+'\n').encode(),0o600)
    for directory in (review,base):
        fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY)
        try:os.fsync(fd)
        finally:os.close(fd)
    new_stat=path.lstat()
    fields=('st_dev','st_ino','st_uid','st_gid','st_mode','st_size','st_mtime_ns')
    if path.read_bytes() != original or any(getattr(new_stat,k)!=getattr(old_stat,k) for k in fields):
        raise SystemExit('configuration changed during containment preparation')
    temporary = path.with_name('.config.receive.incident-shadow.tmp')
    write(temporary,candidate,stat.S_IMODE(old_stat.st_mode))
    os.chown(temporary,old_stat.st_uid,old_stat.st_gid)
    os.replace(temporary,path)
    fd=os.open(path.parent,os.O_RDONLY|os.O_DIRECTORY)
    try:os.fsync(fd)
    finally:os.close(fd)
    write(review/'APPLIED.json',(json.dumps(receipt,sort_keys=True)+'\n').encode(),0o600)
    print(json.dumps({'contained':True,'mode':'shadow','review_directory':str(review),'services_started':False,
                      'journals_opened':False,'provider_operations':0,'gateway_operations':0}))


if __name__=='__main__':
    main()
