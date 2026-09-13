#!/usr/bin/env python3
"""Validate only the source-bound unlinked owner fixture; --apply sends one UUID twice.

Never changes rules, links, ledger contents or physical Items. Retains all receipts.
"""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import importlib.util
import json
from pathlib import Path
import time
import urllib.parse
import uuid

spec = importlib.util.spec_from_file_location('fixture', Path(__file__).with_name('prepare-owner-v2-fixture.py'))
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


def inspect(request):
    expected = fixture.definitions()
    for definition in expected:
        fixture.canary.verify_rule(request('rules/' + definition['uid']), definition)
    for key, kind in fixture.ITEMS.items():
        item = request('items/' + fixture.NAMES[key])
        if item.get('type') != kind or item.get('groupNames') or item.get('tags'):
            raise ValueError('fixture Item binding changed')
    if any(name in json.dumps(request('links')) for name in fixture.NAMES.values()):
        raise ValueError('fixture channel link present')
    allowed = {definition['uid'] for definition in expected}
    for rule in request('rules'):
        if rule['uid'] not in allowed:
            detail = json.dumps(request('rules/' + urllib.parse.quote(rule['uid'], safe='')))
            if any(name in detail for name in fixture.NAMES.values()):
                raise ValueError('unexpected fixture consumer')
    metadata = request('items/' + fixture.NAMES['Ledger'] + '?metadata=autoupdate').get('metadata', {})
    if metadata.get('autoupdate', {}).get('value') != 'false':
        raise ValueError('ledger autoupdate enabled')
    return hashlib.sha256(expected[0]['actions'][0]['configuration']['script'].encode()).hexdigest()


def check(env, apply=False, resume_request_id=None):
    request = fixture.client(env)
    digest = inspect(request)
    def state(key):
        return request('items/' + fixture.NAMES[key] + '/state')
    def wait_for(predicate):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(.1)
        raise ValueError('fixture bounded readback timed out; preserve state, do not retry automatically')
    before = {key: int(state(key)) for key in ('Counter', 'Deliveries')}
    ledger = json.loads(state('Ledger'))
    if state('Actuator') != 'OFF' or ledger.get('version') != 'feeder-request-ledger/v2' or any(e['status'] != 'complete' for e in ledger['entries']):
        raise ValueError('fixture not quiescent; reconcile existing evidence')
    result = {'applied': apply, 'fixture_sha256': digest, 'physical_commands': 0}
    if not apply:
        return result
    previous = next((e for e in ledger['entries'] if e['requestId'] == resume_request_id), None)
    if resume_request_id and previous is None:
        raise ValueError('resume requires an existing complete receipt; no fresh command sent')
    request_id = resume_request_id or str(uuid.uuid4())
    start = (datetime.fromisoformat(previous['at'].replace('Z', '+00:00')) if previous else datetime.now(timezone.utc)) - timedelta(seconds=5)
    command = json.dumps({'version':'feeder-request-v2', 'requestId':request_id,
                          'requestedAt':datetime.now(timezone.utc).isoformat()}).encode()
    if previous is None:
        request('items/' + fixture.NAMES['Request'], 'POST', command)
    def completed():
        try:
            receipt = json.loads(state('Result'))
            return receipt.get('requestId') == request_id and receipt.get('status') == 'complete'
        except json.JSONDecodeError:
            return False
    delta = 0 if previous else 1
    if previous is None:
        wait_for(completed)
    wait_for(lambda: all(int(state(key)) == value + delta for key, value in before.items()))
    complete = json.loads(state('Ledger'))
    query = urllib.parse.urlencode({'serviceId':'jdbc', 'starttime':start.isoformat(),
        'endtime':(datetime.now(timezone.utc)+timedelta(seconds=5)).isoformat(),
        'boundary':'false','itemState':'false','displayState':'false','pagelength':100,'page':0})
    rows = request('persistence/items/' + fixture.NAMES['Ledger'] + '?' + query).get('data', [])
    if not any(json.loads(row['state']) == complete for row in rows):
        raise ValueError('complete ledger absent from explicit JDBC query')
    inspect(request)
    request('items/' + fixture.NAMES['Request'], 'POST', command)
    time.sleep(2)
    if not completed() or state('Actuator') != 'OFF' or any(int(state(key)) != value + delta for key, value in before.items()) or json.loads(state('Ledger')) != complete:
        raise ValueError('duplicate changed actuation count or durable receipt')
    return {**result, 'request_id':request_id, 'request_deliveries':1 if previous else 2,
            'fixture_on_commands':delta, 'counter_delta':delta, 'jdbc_complete':True, 'duplicate_no_actuation':True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--provisioning-env', required=True, type=Path)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--resume-request-id', help='verify and replay only an existing complete fixture UUID')
    args = parser.parse_args()
    print(json.dumps(check(args.provisioning_env, args.apply, args.resume_request_id), indent=2))
