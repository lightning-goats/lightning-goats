#!/usr/bin/env python3
"""Authorize the existing messaging identity and seed three seats, no DB editing.

Authenticated root-owned launcher only. The explicit apply restarts membership
only after its installed native parser accepts the exact candidate. Core payment/
feeding, outgoing roles, credentials and public ingress remain unchanged. Actual
stored capacity is subsequently changed through the signed operator API.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import re
import tempfile
import tomllib

SPEC = importlib.util.spec_from_file_location('project_operator_support', Path(__file__).with_name('maintain-contact-upgrade.py'))
U = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(U)
U.verified_tool_identity = lambda: verified_tool_identity()
C, Q, S, M, TX = U.C, U.Q, U.S, U.M, U.TX


def render(original, project):
    if re.fullmatch('[0-9a-f]{64}', project) is None:
        raise ValueError('existing project public identity required')
    candidate = copy.deepcopy(original)
    auth = candidate['view']['auth']
    if (original['project_pubkey'] != project or original.get('enabled') is not True
            or original['view'].get('enabled') is not True or auth.get('public_origin') != 'https://lightning-goats.com'
            or project in auth['operators']):
        raise ValueError('existing self reads and a newly authorized project operator required')
    auth['operators'].append(project)
    candidate['settings']['max_members'] = 3
    return candidate


def validate(directory, r, selected):
    identity = r['baseline']['identities']['membership']
    program = Q.BIN / 'cyberherdd'
    if TX.fingerprint(program) != r['baseline']['files'][str(program)]:
        raise ValueError('installed native member program changed')
    carrier = Path(tempfile.mkdtemp(prefix='lg-project-operator-', dir='/run'))
    carrier.chmod(0o711)
    try:
        config = carrier / 'config.toml'
        Q.I.publish(config, selected.read_bytes(), 0o440)
        os.chown(config, 0, identity['gid'])
        result = C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
            '--reuid=' + str(identity['uid']), '--regid=' + str(identity['gid']), '--clear-groups',
            '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
            str(program), '--config', str(config), '--check', timeout=30)
        if result.strip() != 'CyberHerd configuration is valid; no runtime started':
            raise ValueError('native project operator configuration refused')
    finally:
        carrier.chmod(0o700)
        TX.sync_directory(carrier)
    if TX.fingerprint(program) != r['baseline']['files'][str(program)]:
        raise ValueError('installed native program changed during validation')


def prepare(directory, project):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    if directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None:
        raise ValueError('new direct private review directory required')
    baseline = U.P.baseline(current_schema=True)
    current = Q.ETC / 'cyberherd/config.toml'
    original = tomllib.loads(current.read_text())
    core = tomllib.loads(C.CONFIG.read_text())
    if core['nostr']['bunker_pubkey'] != project:
        raise ValueError('operator must be the existing messaging public identity')
    candidate = render(original, project)
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    TX.copy_durable(current, directory / 'original-member.toml', baseline['files'][str(current)])
    Q.I.publish(directory / 'candidate-member.toml', S.W.toml_text(candidate).encode(), 0o600)
    r = {'version': 1, 'tool': verified_tool_identity(), 'baseline': baseline, 'project': project,
         'candidate_sha256': U.digest(directory / 'candidate-member.toml'), 'configuration_only': True}
    validate(directory, r, directory / 'candidate-member.toml')
    def fresh():
        if U.P.baseline(current_schema=True) != baseline:
            raise ValueError('original zero-budget installation or process changed')
    rows = TX.prepare(directory / 'configuration', [(current, directory / 'candidate-member.toml',
                      baseline['files'][str(current)])], fresh)
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    fresh()
    r.update(configurations=rows, transaction_sha256=U.digest(directory / 'configuration/TRANSACTION.json'),
             services_changed=False, direct_journal_access=False)
    C.write_json(directory / 'PREPARED.json', r)
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'existing_project_operator': True, 'capacity_seed': 3,
            'services_changed': False, 'direct_journal_access': False}


def load(directory, expected):
    C.root(); U.private_directory(directory)
    if (re.fullmatch('[0-9a-f]{64}', expected) is None
            or U.digest(directory / 'PREPARED.json') != expected or (directory / 'INCOMPLETE').exists()):
        raise ValueError('independently reviewed operator preparation digest required')
    r = C.read_json(directory / 'PREPARED.json')
    if (r.get('version') != 1 or r.get('tool') != verified_tool_identity()
            or r.get('configuration_only') is not True or r.get('services_changed') is not False
            or r.get('direct_journal_access') is not False):
        raise ValueError('matching operator configuration-only preparation required')
    target = Q.ETC / 'cyberherd/config.toml'
    original = directory / 'original-member.toml'
    C.protected(original)
    if TX.fingerprint(original) != r['baseline']['files'][str(target)]:
        raise ValueError('original member configuration changed')
    candidate = directory / 'candidate-member.toml'
    C.protected(candidate)
    if (U.digest(candidate) != r['candidate_sha256']
            or tomllib.loads(candidate.read_text()) != render(tomllib.loads(original.read_text()), r['project'])):
        raise ValueError('only the selected project operator and three-seat seed may change')
    old = r['baseline']['files'][str(target)]
    rows = [{'destination': str(target), 'old': old, 'new': dict(old, sha256=r['candidate_sha256'])}]
    if (r['configurations'] != rows or U.digest(directory / 'configuration/TRANSACTION.json') != r['transaction_sha256']
            or C.read_json(directory / 'configuration/TRANSACTION.json') != {'version': 1, 'files': rows}):
        raise ValueError('operator configuration transaction changed')
    for version in ('old', 'new'):
        if TX.fingerprint(directory / 'configuration' / ('0.' + version)) != rows[0][version]:
            raise ValueError('operator configuration replacement evidence changed')
    return r, {'programs': rows}


def apply(directory, expected, approved):
    if not approved:
        raise ValueError('explicit selected operator configuration and membership restart approval required')
    r, m = load(directory, expected)
    with M.exclusive(directory):
        if (directory / 'APPLY-REQUESTED.json').exists():
            raise ValueError('operator configuration application already began; inspect before retry')
        if U.P.baseline(current_schema=True) != r['baseline']:
            raise ValueError('original running installation changed')
        validate(directory, r, directory / 'candidate-member.toml')
        M.note(directory, 'APPLY-REQUESTED.json', {'approved': True, 'membership_restart_only': True})
        TX.execute(directory / 'configuration', lambda: U.stable(m, r, installed=None))
        U.stable(m, r, installed=True)
        validate(directory, r, Q.ETC / 'cyberherd/config.toml')
        C.command('/usr/bin/systemctl', 'restart', '--', 'cyberherd.service', timeout=120)
        M.running('cyberherd.service', 'cyberherd', Q.BIN / 'cyberherdd')
        M.health('http://127.0.0.1:3340/api/v1/cyberherd')
        U.stable(m, r, installed=True)
        if C.status(C.UNIT_NAME) != r['baseline']['states']['core']:
            raise ValueError('original receive/feeding service changed during member-only restart')
        for role in ('payout', 'signer'):
            if C.status(U.P.ROLE_NAMES[role] + '.service') != r['baseline']['states'][role]:
                raise ValueError('outgoing service changed during operator configuration')
        M.note(directory, 'APPLIED.json', {'existing_project_operator': True, 'capacity_seed': 3,
            'membership_restarted': True, 'core_process_unchanged': True, 'outgoing_roles_unchanged': True,
            'direct_journal_access': False, 'stored_capacity_update_pending': True})
    return {'existing_project_operator': True, 'capacity_seed': 3, 'membership_restarted': True,
            'core_process_unchanged': True, 'outgoing_roles_unchanged': True,
            'direct_journal_access': False, 'stored_capacity_update_pending': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    p = commands.add_parser('prepare'); p.add_argument('directory', type=Path)
    p.add_argument('--project-public-key', required=True)
    p = commands.add_parser('apply'); p.add_argument('directory', type=Path)
    p.add_argument('--review-sha256', required=True)
    p.add_argument('--approve-project-operator', required=True, action='store_true')
    args = parser.parse_args()
    result = (prepare(args.directory, args.project_public_key) if args.operation == 'prepare'
              else apply(args.directory, args.review_sha256, args.approve_project_operator))
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
