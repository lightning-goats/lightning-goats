#!/usr/bin/env python3
"""Bounded local held/release check; never changes existing evidence or remote enable.

Default read-only. --apply requires a new exclusive evidence file, written before
its one request. The intent file is immutable; stage snapshots use its .progress
sibling. On interruption use control-held-canary --release with that UUID;
never automatically create a replacement request.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import uuid
spec=importlib.util.spec_from_file_location('control',Path(__file__).with_name('control-held-canary.py'))
control=importlib.util.module_from_spec(spec)
spec.loader.exec_module(control)


def sync_parent(path):
    fd=os.open(path.parent,os.O_RDONLY|os.O_DIRECTORY)
    try:os.fsync(fd)
    finally:os.close(fd)


def check(env, evidence=None):
    request=control.held.base.client(env)
    digest=control.held.inspect(request)
    before=control.snapshot(request)
    if before['hold']!='ON' or before['remote_enabled']!='OFF' or any(row['status']=='held' for row in before['deliveries']):
        raise ValueError('local check requires hold ON, remote OFF and no unresolved fixture request')
    if evidence is None: return {'apply':False,'source_sha256':digest,'before':before}
    report={'source_sha256':digest,'request_id':str(uuid.uuid4()),'before':before,'stage':'prepared','physical_commands':0}
    evidence=Path(evidence)
    progress=Path(str(evidence)+'.progress')
    # Reserve both names before dispatch. Never truncate the immutable intent.
    # Any partial preparation is preserved and requires operator inspection.
    for path in (progress,evidence):
        fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
        with os.fdopen(fd,'w') as out:
            json.dump(report,out,indent=2);out.flush();os.fsync(out.fileno())
    sync_parent(evidence)
    def record(stage):
        report['stage']=stage
        fd,name=tempfile.mkstemp(prefix='.'+evidence.name+'.',dir=evidence.parent)
        try:
            with os.fdopen(fd,'w') as out:
                json.dump(report,out,indent=2);out.flush();os.fsync(out.fileno())
            os.replace(name,progress)
            sync_parent(progress)
        finally:
            # A caught failure leaves the previous complete snapshot intact.
            # Abrupt process death may leave a partial temporary file; ignore it.
            if os.path.exists(name):os.unlink(name)
    control.held.inspect(request)
    request('items/'+control.held.PREFIX+'Request','POST',report['request_id'].encode())
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        try: observed=control.snapshot(request)
        except ValueError:
            time.sleep(.1);continue
        if observed['count']==before['count']+1 and any(row['requestId']==report['request_id'] and row['status']=='held' for row in observed['deliveries']):break
        time.sleep(.1)
    else:raise ValueError('held delivery unavailable; preserve evidence, no automatic retry')
    report['held']=observed;record('held')
    time.sleep(6) # longer than the shipped five-second gateway acknowledgement timeout
    observed=control.snapshot(request)
    if observed['ack']==report['request_id'] or observed['count']!=before['count']+1:
        raise ValueError('fixture did not retain authoritative hold')
    report['released']=control.control(env,report['request_id']);record('released')
    report['release_replay']=control.control(env,report['request_id']);record('passed')
    return report


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--provisioning-env',required=True,type=Path)
    p.add_argument('--apply',action='store_true')
    p.add_argument('--evidence',type=Path)
    args=p.parse_args()
    if args.apply != bool(args.evidence):p.error('--apply requires a new --evidence file')
    print(json.dumps(check(args.provisioning_env,args.evidence),indent=2))
