#!/usr/bin/env python3
"""Inspect the fixed unlinked held canary, or explicitly release one recorded UUID.

No Request command, remote enable, fault reset, history deletion or physical action.
"""
import argparse
from decimal import Decimal
import importlib.util
import json
from pathlib import Path
import time
import uuid
spec=importlib.util.spec_from_file_location('held',Path(__file__).with_name('prepare-held-canary.py'))
held=importlib.util.module_from_spec(spec)
spec.loader.exec_module(held)


def snapshot(request):
    state=lambda key: request('items/'+held.PREFIX+key+'/state')
    journal=json.loads(state('Journal'))
    number=Decimal(state('Count'))
    if not number.is_finite() or number < 0 or number != number.to_integral_value() or number > 9007199254740991:
        raise ValueError('invalid held delivery count')
    count=int(number)
    if journal.get('version') != 'held-canary/v1' or count != len(journal['deliveries']):
        raise ValueError('delivery journal gap; preserve all state and reconcile')
    if state('Fault') != 'OFF':
        raise ValueError('fixture fault; no automatic reset')
    return {'count':count,'hold':state('Hold'),'remote_enabled':state('RemoteEnabled'),
            'ack':state('Ack'),'deliveries':journal['deliveries']}


def control(env, release=None):
    request=held.base.client(env)
    digest=held.inspect(request)
    before=snapshot(request)
    if release is None:
        return {'source_sha256':digest,**before}
    target=str(uuid.UUID(release))
    if not any(row['requestId']==target and row['status'] in ('held','released') for row in before['deliveries']):
        raise ValueError('release UUID is not recorded; no command sent')
    if held.inspect(request) != digest:
        raise ValueError('fixture changed before release')
    request('items/'+held.PREFIX+'Release','POST',target.encode())
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        observed=snapshot(request)
        if observed['count'] != before['count']:
            raise ValueError('delivery count changed during release; preserve evidence')
        if observed['ack']==target and all(row['status']=='released' for row in observed['deliveries'] if row['requestId']==target):
            return {'source_sha256':digest,'released':target,'request_commands':0,**observed}
        time.sleep(.1)
    raise ValueError('release result unavailable; do not submit a fresh request')


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--provisioning-env',required=True,type=Path)
    p.add_argument('--release',type=str)
    args=p.parse_args()
    print(json.dumps(control(args.provisioning_env,args.release),indent=2))
