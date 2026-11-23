#!/usr/bin/env python3
"""Prepare a separate unlinked held canary; fresh-only --apply, no existing changes."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import time
import urllib.parse

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('owner_fixture', Path(__file__).with_name('prepare-owner-v2-fixture.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
PREFIX = 'LightningGoatsHeldCanary2'
RULE = 'lightning_goats_held_canary2'
ITEMS = {'Request':('String',None), 'Ack':('String',None), 'Release':('String',None),
         'Journal':('String',json.dumps({'version':'held-canary/v1','deliveries':[]})),
         'Count':('Number','0'), 'Hold':('Switch','ON'), 'Fault':('Switch','OFF'),
         'Override':('Switch','OFF'), 'RemoteEnabled':('Switch','OFF'), 'Bootstrap':('Switch','OFF')}


def definitions():
    source = (ROOT / 'deploy/openhab/gateway-held-canary.js').read_text()
    if any(value in source for value in ('sendCommand', 'GoatFeeder_', 'Goat_Plugs_', 'GoatFeedings', 'http://', 'https://')):
        raise ValueError('unexpected held fixture operation')
    main = base.rule(RULE, PREFIX+'Request', source)
    main['triggers'].append({'id':'3','type':'core.ItemCommandTrigger','configuration':{'itemName':PREFIX+'Release'}})
    bootstrap = "const {items}=require('openhab'); for (const name of ['Count','Journal','Fault']) items.getItem('LightningGoatsHeldCanary2'+name).persistence.persist('jdbc');"
    return [main, base.rule(RULE+'_bootstrap', PREFIX+'Bootstrap', bootstrap)]


def inspect(request):
    expected=definitions()
    for definition in expected:
        base.canary.verify_rule(request('rules/'+definition['uid']),definition)
    for suffix,(kind,_) in ITEMS.items():
        observed=request('items/'+PREFIX+suffix+'?metadata=autoupdate')
        if observed.get('type') != kind or observed.get('groupNames') or observed.get('tags'):
            raise ValueError('held fixture Item drift')
        if suffix in ('Journal','Count','Fault') and observed.get('metadata',{}).get('autoupdate',{}).get('value') != 'false':
            raise ValueError('held fixture persistence Item autoupdate drift')
    if any(PREFIX+name in json.dumps(request('links')) for name in ITEMS):
        raise ValueError('held fixture channel link present')
    allowed={rule['uid'] for rule in expected}
    for rule in request('rules'):
        if rule['uid'] not in allowed:
            detail=json.dumps(request('rules/'+urllib.parse.quote(rule['uid'],safe='')))
            if any(PREFIX+name in detail for name in ITEMS):
                raise ValueError('unexpected held fixture consumer')
    return hashlib.sha256(expected[0]['actions'][0]['configuration']['script'].encode()).hexdigest()


def prepare(env, apply=False):
    request=base.client(env)
    for name in ITEMS:
        if request('items/'+PREFIX+name,absent=True) is not None:
            raise ValueError('held fixture already exists; preserve evidence and inspect separately')
    expected=definitions()
    for rule in request('rules'):
        detail=json.dumps(request('rules/'+urllib.parse.quote(rule['uid'],safe='')))
        if rule['uid'] in {entry['uid'] for entry in expected} or any(PREFIX+name in detail for name in ITEMS):
            raise ValueError('held fixture rule/consumer already exists')
    if any(PREFIX+name in json.dumps(request('links')) for name in ITEMS):
        raise ValueError('held fixture link already exists')
    if apply:
        for suffix,(kind,initial) in ITEMS.items():
            name=PREFIX+suffix
            request('items/'+name,'PUT',{'name':name,'type':kind,'groupNames':[],'tags':[]})
            if suffix in ('Journal','Count','Fault'):
                request('items/'+name+'/metadata/autoupdate','PUT',{'value':'false','config':{}})
            if initial is not None:
                request('items/'+name+'/state','PUT',initial.encode())
        for _ in range(50):
            if all(initial is None or request('items/'+PREFIX+suffix+'/state') == initial for suffix,(_,initial) in ITEMS.items()):
                break
            time.sleep(.1)
        else:
            raise ValueError('held initial state readback failed')
        for rule in expected:
            request('rules','POST',rule)
        inspect(request)
        # Only initializes persistence for these new unlinked fixture Items.
        request('items/'+PREFIX+'Bootstrap','POST',b'ON')
    return {'applied':apply,'source_sha256':hashlib.sha256((ROOT/'deploy/openhab/gateway-held-canary.js').read_bytes()).hexdigest(),
            'items':[PREFIX+key for key in ITEMS], 'hold':'ON','remote_enabled':'OFF','existing_fixture_changed':False}


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--provisioning-env',required=True,type=Path)
    parser.add_argument('--apply',action='store_true')
    parser.add_argument('--inspect',action='store_true')
    args=parser.parse_args()
    if args.inspect and args.apply: parser.error('inspect cannot apply')
    result={'source_sha256':inspect(base.client(args.provisioning_env))} if args.inspect else prepare(args.provisioning_env,args.apply)
    print(json.dumps(result,indent=2))
