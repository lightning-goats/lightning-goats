#!/usr/bin/env python3
"""Validate ONLY the installed loopback UUID canary. Plan by default.

--apply briefly enables only the canary switch, tests safety/replay/concurrency
and restarts only its service; always returns its remote switch to OFF. Requires
existing admin provisioning env for target/rule verification and canary switch.
Never changes physical Items, sends production requests or resets any database.
"""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import home_gateway_safety as safety
import subprocess
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('canary', ROOT / 'deploy/scripts/prepare-home-openhab-canary.py')
canary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(canary)


def verify_other_consumers(oh):
    for rule in json.loads(oh('rules')):
        if rule['uid'] == canary.RULE:
            continue
        full = json.dumps(json.loads(oh('rules/' + rule['uid'])))
        if any(item in full for item in canary.ITEMS):
            raise ValueError('another rule references canary Items')


def wait_verified():
    for attempt in range(30):
        try:
            safety.verify_running()
            return
        except (OSError, ValueError):
            if attempt == 29:
                raise
            time.sleep(.2)


def apply(env):
    if os.geteuid() != 0:
        raise ValueError('--apply requires root for effective process/listener validation')
    safety.installed_source()
    auth = 'Basic ' + base64.b64encode((canary.token_from_file(env) + ':').encode()).decode()
    opener = safety.local_opener()

    def oh(path, method='GET', data=None):
        req = urllib.request.Request('http://127.0.0.1:8080/rest/' + path, method=method, data=data,
                                     headers={'Authorization': auth, 'Content-Type': 'text/plain'})
        with opener.open(req, timeout=5) as r:
            return r.read().decode()

    def gateway(path, method='GET'):
        req = urllib.request.Request('http://127.0.0.1:8790/' + path, method=method,
                                     data=b'' if method == 'POST' else None)
        try:
            response = opener.open(req, timeout=10)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return {'http': response.code, 'body': json.loads(response.read())}

    def count():
        return int(oh('items/LightningGoatsCanaryCount/state'))

    def feed(request_id):
        response = gateway('v1/feeder/request/' + request_id, 'POST')
        if response['body'].get('request_id') != request_id:
            raise ValueError('uncorrelated response')
        return response

    live = json.loads(oh('rules/' + canary.RULE))
    canary.verify_rule(live, canary.rule_definition())
    for item, (kind, _) in canary.ITEMS.items():
        value = json.loads(oh('items/' + item))
        if value['type'] != kind or value.get('groupNames') or value.get('tags'):
            raise ValueError('canary Item has unexpected type/group/tag')
    verify_other_consumers(oh)
    if any(item in oh('links') for item in canary.ITEMS):
        raise ValueError('canary Item is channel-linked')
    if oh('items/LightningGoatsCanaryRemoteEnabled/state') != 'OFF':
        raise ValueError('canary must begin remote-disabled')
    if oh('items/LightningGoatsCanaryOverride/state') != 'OFF':
        raise ValueError('unexpected canary override')
    # Reload the verified config into the canary process before any gateway POST.
    # No daemon-reload: stale or overridden loaded units are rejected above.
    subprocess.run(['systemctl', 'restart', safety.UNIT.name], check=True)
    wait_verified()
    before_real = oh('items/FeederOverride/state')
    start = count()
    evidence = {'initial_count': start, 'safety': gateway('v1/feeder/override')}
    refused = str(uuid.uuid4())
    evidence['remote_off'] = feed(refused)
    assert evidence['remote_off']['body']['status'] == 'not_dispatched' and count() == start
    try:
        oh('items/LightningGoatsCanaryRemoteEnabled/state', 'PUT', b'ON')
        evidence['refusal_replay'] = feed(refused)
        assert evidence['refusal_replay'] == evidence['remote_off'] and count() == start
        time.sleep(5.1)
        request_id = str(uuid.uuid4())
        evidence['confirmed'] = feed(request_id)
        assert evidence['confirmed']['body']['status'] == 'confirmed'
        evidence['duplicate'] = feed(request_id)
        assert evidence['duplicate'] == evidence['confirmed'] and count() == start + 1
        safety.installed_source()
        subprocess.run(['systemctl', 'restart', safety.UNIT.name], check=True)
        wait_verified()
        evidence['restart_replay'] = feed(request_id)
        assert evidence['restart_replay'] == evidence['confirmed'] and count() == start + 1
        time.sleep(5.1)
        with ThreadPoolExecutor(max_workers=2) as pool:
            evidence['concurrent'] = list(pool.map(feed, [str(uuid.uuid4()), str(uuid.uuid4())]))
        assert sorted(x['body']['status'] for x in evidence['concurrent']) == ['confirmed', 'not_dispatched']
        assert count() == start + 2
    finally:
        oh('items/LightningGoatsCanaryRemoteEnabled/state', 'PUT', b'OFF')
    evidence['final_count'] = count()
    evidence['remote_final'] = oh('items/LightningGoatsCanaryRemoteEnabled/state')
    evidence['physical_override_unchanged'] = before_real == oh('items/FeederOverride/state')
    assert evidence['physical_override_unchanged']
    evidence['rule_sha256'] = hashlib.sha256(live['actions'][0]['configuration']['script'].encode()).hexdigest()
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--provisioning-env', type=Path, required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    if args.apply:
        apply(args.provisioning_env)
    else:
        print(json.dumps({'apply': False, 'gateway': 'http://127.0.0.1:8790',
                          'protocol': 'uuid_canary', 'remote_switch_final': 'OFF'}))
