#!/usr/bin/env python3
"""Create a fresh, unlinked owner-v2 runtime fixture. Default plan; never live owner.

No credential output, owner replacement, cleanup of prior evidence or physical
Item commands. Fixture bootstrap persists only its separate empty ledger.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parent))
import home_gateway_safety as safety
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('canary', Path(__file__).with_name('prepare-home-openhab-canary.py'))
canary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(canary)
PREFIX = 'LightningGoatsOwnerV2Test'
RULE = 'lightning_goats_owner_v2_fixture'
NAMES = {key: PREFIX + key for key in ('Request', 'Result', 'Ledger', 'Actuator', 'Counter', 'Bootstrap', 'Deliveries')}
ITEMS = {'Request': 'String', 'Result': 'String', 'Ledger': 'String', 'Actuator': 'Switch',
         'Counter': 'Number', 'Bootstrap': 'Switch', 'Deliveries': 'Number'}


def rule(uid, item, script):
    return {'uid': uid, 'name': uid, 'tags': [], 'conditions': [],
            'triggers': [{'id': '1', 'type': 'core.ItemCommandTrigger', 'configuration': {'itemName': item}}],
            'actions': [{'id': '2', 'type': 'script.ScriptAction', 'configuration': {'type': 'application/javascript', 'script': script}}]}


def definitions():
    source = (ROOT / 'deploy/openhab/feeder-owner-v2.js').read_text()
    bindings = {'GoatFeeder_ManualRequest': NAMES['Request'], 'GoatFeeder_ManualResult': NAMES['Result'],
                'GoatFeeder_OwnerLedgerV2': NAMES['Ledger'], 'Goat_Plugs_Outlet2_Switch': NAMES['Actuator'],
                'GoatFeedings': NAMES['Counter'], 'earthship.feeder-owner.v2.guard': 'lightning-goats.owner-v2-fixture.guard',
                'earthship.feeder-owner.last-start-ms': 'lightning-goats.owner-v2-fixture.last-start-ms'}
    for original, replacement in bindings.items():
        if source.count("'" + original + "'") != 1:
            raise ValueError('owner fixture source binding changed')
        source = source.replace("'" + original + "'", "'" + replacement + "'")
    if any(value in source for value in bindings):
        raise ValueError('physical binding survived fixture rendering')
    bootstrap = "const {items}=require('openhab'); items.getItem('" + NAMES['Ledger'] + "').persistence.persist('jdbc');"
    deliveries = """const {items,cache}=require('openhab');
if (event.receivedCommand === 'ON') {
 const AtomicLong=Java.type('java.util.concurrent.atomic.AtomicLong');
 const item=items.getItem('""" + NAMES['Deliveries'] + """');
 const count=cache.shared.get('lightning-goats.owner-v2-fixture.deliveries',()=>new AtomicLong(Number(item.state.toString())));
 item.postUpdate(String(count.incrementAndGet()));
}
"""
    return [rule(RULE, NAMES['Request'], source), rule(RULE + '_bootstrap', NAMES['Bootstrap'], bootstrap),
            rule(RULE + '_deliveries', NAMES['Actuator'], deliveries)]


def client(env):
    auth = 'Basic ' + base64.b64encode((canary.token_from_file(env) + ':').encode()).decode()
    opener = safety.local_opener()
    def request(path, method='GET', body=None, absent=False):
        data = json.dumps(body).encode() if isinstance(body, dict) else body
        kind = 'application/json' if isinstance(body, dict) else 'text/plain'
        req = urllib.request.Request('http://127.0.0.1:8080/rest/' + path, method=method, data=data,
                                     headers={'Authorization': auth, 'Content-Type': kind})
        try:
            with opener.open(req, timeout=5) as response:
                raw = response.read(4*1024*1024)
                return json.loads(raw) if raw and response.headers.get_content_type() == 'application/json' else raw.decode()
        except urllib.error.HTTPError as error:
            if absent and error.code == 404:
                return None
            raise ValueError('fixture REST returned HTTP ' + str(error.code)) from None
    return request


def prepare(env, apply=False):
    request = client(env)
    rules = definitions()
    for name in NAMES.values():
        if request('items/' + name, absent=True) is not None:
            raise ValueError('existing fixture Item requires separate review')
    for definition in rules:
        if request('rules/' + definition['uid'], absent=True) is not None:
            raise ValueError('existing fixture rule requires separate review')
    for found in request('rules'):
        detail = json.dumps(request('rules/' + urllib.parse.quote(found['uid'], safe='')))
        if any(name in detail for name in NAMES.values()):
            raise ValueError('other rule already references fixture Items')
    if any(name in json.dumps(request('links')) for name in NAMES.values()):
        raise ValueError('fixture Item already channel-linked')
    if apply:
        for key, kind in ITEMS.items():
            request('items/' + NAMES[key], 'PUT', {'name':NAMES[key], 'type':kind,'groupNames':[], 'tags':[]})
            if key in ('Counter', 'Deliveries'):
                request('items/' + NAMES[key] + '/state', 'PUT', b'0')
            if key in ('Actuator', 'Bootstrap'):
                request('items/' + NAMES[key] + '/state', 'PUT', b'OFF')
        request('items/' + NAMES['Ledger'] + '/metadata/autoupdate', 'PUT', {'value':'false','config':{}})
        ledger = json.dumps({'version':'feeder-request-ledger/v2','entries':[]}).encode()
        request('items/' + NAMES['Ledger'] + '/state','PUT',ledger)
        expected_states = {'Ledger': ledger.decode(), 'Counter':'0', 'Deliveries':'0', 'Actuator':'OFF', 'Bootstrap':'OFF'}
        for attempt in range(50):
            if all(request('items/' + NAMES[key] + '/state') == value for key, value in expected_states.items()):
                break
            time.sleep(.1)
        else:
            raise ValueError('fixture initial state not visible; no bootstrap sent')
        for key, kind in ITEMS.items():
            observed = request('items/' + NAMES[key])
            if observed.get('type') != kind or observed.get('groupNames') or observed.get('tags'):
                raise ValueError('unexpected fixture Item readback')
        if any(name in json.dumps(request('links')) for name in NAMES.values()):
            raise ValueError('fixture unexpectedly channel-linked')
        for definition in rules:
            request('rules', 'POST', definition)
            canary.verify_rule(request('rules/' + definition['uid']), definition)
        # Only the fixture bootstrap Item, never the physical command Item.
        request('items/' + NAMES['Bootstrap'], 'POST', b'ON')
    return {'applied':apply, 'items':NAMES, 'rule_sha256':hashlib.sha256(rules[0]['actions'][0]['configuration']['script'].encode()).hexdigest(),
            'physical_items_changed':False, 'owner_source_sha256':hashlib.sha256((ROOT / 'deploy/openhab/feeder-owner-v2.js').read_bytes()).hexdigest()}


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--provisioning-env',type=Path,required=True)
    parser.add_argument('--apply',action='store_true')
    args=parser.parse_args()
    print(json.dumps(prepare(args.provisioning_env,args.apply),indent=2))
