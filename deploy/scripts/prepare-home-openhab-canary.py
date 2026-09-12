#!/usr/bin/env python3
"""Create only fresh unlinked canary Items and their harmless rule; default plan.

Uses an existing provisioning token from a protected local env file, never stdout.
Does not enable the canary remote switch, start services, or touch physical Items.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import stat
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
RULE = 'lightning_goats_gateway_canary'
ITEMS = {'LightningGoatsCanaryRequest': ('String', 'NULL'),
         'LightningGoatsCanaryAck': ('String', 'NULL'),
         'LightningGoatsCanaryCount': ('Number', '0'),
         'LightningGoatsCanaryOverride': ('Switch', 'OFF'),
         'LightningGoatsCanaryRemoteEnabled': ('Switch', 'OFF')}


def token_from_file(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o077 or info.st_uid not in (0, os.geteuid()):
        raise ValueError('provisioning env must be a private owned regular file')
    for line in path.read_text().splitlines():
        if line.startswith('OPENHAB_TOKEN='):
            value = line.split('=', 1)[1].strip().strip('\"\'')
            if value:
                return value
    raise ValueError('OPENHAB_TOKEN missing')


def rule_definition():
    return {'uid': RULE, 'name': 'Lightning Goats harmless gateway canary',
            'description': 'Unlinked UUID echo and every-command count; no physical actions.',
            'tags': [], 'conditions': [],
            'triggers': [{'id': '1', 'type': 'core.ItemCommandTrigger',
                          'configuration': {'itemName': 'LightningGoatsCanaryRequest'}}],
            'actions': [{'id': '2', 'type': 'script.ScriptAction',
                         'configuration': {'type': 'application/javascript',
                                           'script': (ROOT / 'deploy/openhab/gateway-canary.js').read_text()}}]}


def verify_rule(observed, expected):
    actions = [{k: v for k, v in a.items() if k != 'inputs' or v} for a in observed['actions']]
    if actions != expected['actions'] or observed['triggers'] != expected['triggers'] or observed.get('conditions') != []:
        raise ValueError('rule readback mismatch')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--provisioning-env', type=Path, required=True)
    p.add_argument('--apply', action='store_true')
    args = p.parse_args()
    auth = 'Basic ' + base64.b64encode((token_from_file(args.provisioning_env) + ':').encode()).decode()
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None

    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def request(path, method='GET', body=None, content_type='application/json', absent=False):
        data = json.dumps(body).encode() if content_type == 'application/json' and body is not None else body
        req = urllib.request.Request('http://127.0.0.1:8080/rest/' + path, data=data, method=method,
                                     headers={'Authorization': auth, 'Content-Type': content_type})
        try:
            with opener.open(req, timeout=10) as r:
                raw = r.read(4 * 1024 * 1024)
                return json.loads(raw) if raw and r.headers.get_content_type() == 'application/json' else raw
        except urllib.error.HTTPError as e:
            if absent and e.code == 404:
                return None
            raise ValueError(f'OpenHAB {method} {path}: HTTP {e.code}') from None

    # Require absence before any mutation. Never overwrite a previous run.
    for item in ITEMS:
        if request('items/' + item, absent=True) is not None:
            raise ValueError(f'existing Item needs explicit reuse review: {item}')
    if request('rules/' + RULE, absent=True) is not None:
        raise ValueError('existing canary rule needs explicit reuse review')
    rules = request('rules')
    for rule in rules:
        full = request('rules/' + rule['uid'])
        if any(item in json.dumps(full) for item in ITEMS):
            raise ValueError('existing rule references canary Items')
    if any(item in json.dumps(request('links')) for item in ITEMS):
        raise ValueError('existing channel link references canary Items')
    definition = rule_definition()
    if args.apply:
        for name, (kind, initial) in ITEMS.items():
            request('items/' + name, 'PUT', {'type': kind, 'name': name, 'label': name,
                                            'groupNames': [], 'tags': []})
            if initial != 'NULL':
                request('items/' + name + '/state', 'PUT', initial.encode(), 'text/plain')
        request('rules', 'POST', definition)
        observed = request('rules/' + RULE)
        verify_rule(observed, definition)
        for name, (kind, initial) in ITEMS.items():
            observed = request('items/' + name)
            if observed['type'] != kind or observed.get('groupNames') or observed.get('tags'):
                raise ValueError('Item readback mismatch')
    print(json.dumps({'applied': args.apply, 'rule_uid': RULE,
                      'script_sha256': hashlib.sha256(definition['actions'][0]['configuration']['script'].encode()).hexdigest(),
                      'items': list(ITEMS), 'remote_enabled': False, 'physical_items_changed': False}, indent=2))


if __name__ == '__main__':
    main()
