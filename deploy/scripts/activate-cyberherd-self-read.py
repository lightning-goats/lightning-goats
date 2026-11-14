#!/usr/bin/env python3
"""Configure signed CyberHerd self reads with an empty operator allowlist.

Authenticated root-owned launcher only. One member configuration change, current
native four-journal backup, unchanged paid images and separately verified resume.
No operator key, public ingress, site flag, program, credential or financial
policy is installed. There is no migration, restore or automatic failure retry.
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
from urllib.parse import urlsplit

spec = importlib.util.spec_from_file_location('self_read_maintenance_support', Path(__file__).with_name('maintain-contact-upgrade.py'))
U = importlib.util.module_from_spec(spec)
spec.loader.exec_module(U)
U.verified_tool_identity = lambda: verified_tool_identity()
C, Q, S, M, TX = U.C, U.Q, U.S, U.M, U.TX


def member_config():
    return Q.ETC / 'cyberherd/config.toml'


def configuration_scope(original, candidate, origin):
    url = urlsplit(origin)
    if (len(origin) > 512 or url.scheme != 'https' or not url.hostname or url.username is not None
            or url.password is not None or url.path or url.query or url.fragment
            or original.get('enabled') is not True or original.get('view', {}).get('enabled') is not True
            or 'auth' in original['view']):
        raise ValueError('unchanged enabled member/view and a new canonical HTTPS self-read origin required')
    expected = copy.deepcopy(original)
    expected['view']['auth'] = {'public_origin': origin, 'operators': []}
    if candidate != expected:
        raise ValueError('only the exact self-read origin and empty operator allowlist may be added')


def validate(baseline, original, candidate, origin):
    configuration_scope(tomllib.loads(original.read_text()), tomllib.loads(candidate.read_text()), origin)
    program = Q.BIN / 'cyberherdd'
    C.protected(program)
    if TX.fingerprint(program) != baseline['files'][str(program)]:
        raise ValueError('installed native member checker changed')
    component = baseline['identities']['membership']
    C.protected(Path('/run'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-self-read-check-', dir='/run'))
    carrier.chmod(0o711)
    try:
        config = carrier / 'config.toml'
        Q.I.publish(config, candidate.read_bytes(), 0o440)
        os.chown(config, 0, component['gid'])
        result = C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
            '--reuid=' + str(component['uid']), '--regid=' + str(component['gid']), '--clear-groups',
            '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
            str(program), '--config', str(config), '--check', timeout=30)
        if result.strip() != 'CyberHerd configuration is valid; no runtime started':
            raise ValueError('native self-read configuration validation refused')
    finally:
        carrier.chmod(0o700)
        TX.sync_directory(carrier)
    if TX.fingerprint(program) != baseline['files'][str(program)]:
        raise ValueError('native member checker changed during validation')
    return {'accepted': True, 'configuration_only': True, 'only_self_read_auth_added': True,
            'empty_operator_allowlist': True, 'original_uid_and_isolated_network': True,
            'journals_opened': False, 'credentials_loaded': False}


def prepare(directory, origin):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    if directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None:
        raise ValueError('new direct private upgrade-base child required')
    baseline = U.P.baseline(current_schema=True)
    if (baseline['states']['core']['FragmentPath'] != str(C.UNIT)
            or baseline['states']['membership']['FragmentPath'] != str(Q.UNITS / 'cyberherd.service')
            or (U.MASKS / 'cyberherd.service').exists() or (U.MASKS / 'cyberherd.service').is_symlink()):
        raise ValueError('original running core and membership unit topology required')
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    member = member_config()
    TX.copy_durable(member, directory / 'original-member.toml', baseline['files'][str(member)])
    TX.copy_durable(C.UNIT, directory / 'original-unit', baseline['files'][str(C.UNIT)])
    original = tomllib.loads((directory / 'original-member.toml').read_text())
    candidate = copy.deepcopy(original)
    candidate.setdefault('view', {})['auth'] = {'public_origin': origin, 'operators': []}
    configuration_scope(original, candidate, origin)
    Q.I.publish(directory / 'candidate-member.toml', S.W.toml_text(candidate).encode(), 0o600)
    accepted = validate(baseline, directory / 'original-member.toml', directory / 'candidate-member.toml', origin)
    payload = directory / 'payload'
    payload.mkdir(mode=0o700)
    program = Q.BIN / 'cyberherd-statectl'
    TX.copy_durable(program, payload / program.name, baseline['files'][str(program)])

    def fresh():
        if U.P.baseline(current_schema=True) != baseline:
            raise ValueError('installation or process changed during self-read preparation')

    rows = TX.prepare(directory / 'configuration', [
        (member, directory / 'candidate-member.toml', baseline['files'][str(member)])
    ], fresh)
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    fresh()
    C.write_json(directory / 'PREPARED.json', {
        'version': 1, 'tool': verified_tool_identity(), 'baseline': baseline,
        'self_read_auth_only': True, 'operator_access_enabled': False, 'public_origin': origin,
        'native_validation': accepted, 'configurations': rows,
        'configuration_manifest_sha256': U.digest(directory / 'configuration/TRANSACTION.json'),
        'candidate_sha256': U.digest(directory / 'candidate-member.toml'),
        'prepared_only': True, 'services_changed': False, 'journals_opened': False,
        'public_ingress_changed': False,
    })
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'self_read_auth_only': True, 'operator_access_enabled': False,
            'services_changed': False, 'journals_opened': False, 'public_ingress_changed': False}


def load(directory):
    C.root()
    U.private_directory(directory)
    if (directory / 'INCOMPLETE').exists():
        raise ValueError('incomplete self-read preparation retained')
    a = C.read_json(directory / 'PREPARED.json')
    if (a.get('version') != 1 or a.get('tool') != verified_tool_identity()
            or a.get('self_read_auth_only') is not True or a.get('prepared_only') is not True
            or any(a.get(key) is not False for key in ('operator_access_enabled', 'services_changed',
                                                     'journals_opened', 'public_ingress_changed'))):
        raise ValueError('self-read-only preparation identity changed')
    member = member_config()
    for name, path in [('original-member.toml', member), ('original-unit', C.UNIT)]:
        C.protected(directory / name)
        if TX.fingerprint(directory / name) != a['baseline']['files'][str(path)]:
            raise ValueError('original member configuration/unit backup changed')
    C.protected(directory / 'candidate-member.toml')
    if U.digest(directory / 'candidate-member.toml') != a['candidate_sha256']:
        raise ValueError('self-read candidate changed')
    configuration_scope(tomllib.loads((directory / 'original-member.toml').read_text()),
        tomllib.loads((directory / 'candidate-member.toml').read_text()), a['public_origin'])
    if U.digest(directory / 'configuration/TRANSACTION.json') != a['configuration_manifest_sha256']:
        raise ValueError('self-read configuration transaction changed')
    old = a['baseline']['files'][str(member)]
    expected = [{'destination': str(member), 'old': old, 'new': dict(old, sha256=a['candidate_sha256'])}]
    transaction = C.read_json(directory / 'configuration/TRANSACTION.json')
    if transaction['version'] != 1 or transaction['files'] != expected or a['configurations'] != expected:
        raise ValueError('only the exact member configuration may be replaced')
    for version in ('old', 'new'):
        if TX.fingerprint(directory / 'configuration' / ('0.' + version)) != expected[0][version]:
            raise ValueError('configuration transaction payload changed')
    program = directory / 'payload/cyberherd-statectl'
    C.protected(program)
    if TX.fingerprint(program) != a['baseline']['files'][str(Q.BIN / program.name)]:
        raise ValueError('native state verifier changed')
    return a, {'programs': expected, 'review': str(directory)}, {'baseline': a['baseline']}


def native_validation(directory, a, candidate):
    if validate(a['baseline'], directory / 'original-member.toml', candidate, a['public_origin']) != a['native_validation']:
        raise ValueError('native self-read validation changed')


def apply(directory, expected, approved):
    if not approved:
        raise ValueError('explicit exclusive maintenance approval required')
    a, m, r = load(directory)
    if U.digest(directory / 'PREPARED.json') != expected:
        raise ValueError('independently reviewed preparation digest required')
    with M.exclusive(directory):
        if (directory / 'MAINTENANCE-STARTED.json').exists():
            raise ValueError('self-read maintenance already began; preserve evidence and inspect')
        if U.P.baseline(current_schema=True) != a['baseline']:
            raise ValueError('reviewed installation or process changed')
        native_validation(directory, a, directory / 'candidate-member.toml')
        M.note(directory, 'MAINTENANCE-STARTED.json', {'exclusive_maintenance': True, 'database_restore_authorized': False})
        U.quiesce(m, r)
        observed = U.state_files(r)
        U.native(m, 'snapshot', '--destination', directory / 'backup', '--writers-stopped')
        accepted = U.native(m, 'verify', '--bundle', directory / 'backup')
        U.sources_match(m, r, observed)
        M.note(directory, 'BACKUP-ACCEPTED.json', {'native': accepted, 'originals': observed,
            'manifest_sha256': U.digest(directory / 'backup/MANIFEST.json')})
        TX.execute(directory / 'configuration', lambda: U.sources_match(m, r, observed, installed=None))
        U.sources_match(m, r, observed, installed=True)
        native_validation(directory, a, member_config())
        M.note(directory, 'APPLIED.json', {'native_backup': accepted, 'live_images': observed,
            'backup_manifest_sha256': U.digest(directory / 'backup/MANIFEST.json'),
            'self_read_auth_only': True, 'operator_access_enabled': False,
            'configuration_sha256': m['programs'][0]['new']['sha256'], 'services_started': False})
    return {'self_reads_configured': True, 'operator_access_enabled': False,
            'current_four_journal_backup_verified': True, 'financial_images_unchanged': True,
            'services_started': False}


def resume(directory, approved):
    if not approved:
        raise ValueError('explicit service resumption approval required')
    a, m, r = load(directory)
    with M.exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists():
            raise ValueError('resumption already began; inspect instead of restarting')
        applied = C.read_json(directory / 'APPLIED.json')
        if applied.get('self_read_auth_only') is not True or applied.get('operator_access_enabled') is not False:
            raise ValueError('self-read application receipt changed')
        U.sources_match(m, r, applied['live_images'], installed=True)
        if (U.digest(directory / 'backup/MANIFEST.json') != applied['backup_manifest_sha256']
                or U.native(m, 'verify', '--bundle', directory / 'backup') != applied['native_backup']):
            raise ValueError('accepted self-read backup changed')
        native_validation(directory, a, member_config())
        M.note(directory, 'RESUME-REQUESTED.json', {'approved': True, 'operator_access_enabled': False, 'payouts_enabled': False})
        M.restore_unit(directory / 'original-unit', a['baseline']['files'][str(C.UNIT)])
        mask = U.MASKS / 'cyberherd.service'
        if not M.masked(mask):
            raise ValueError('membership mask changed')
        mask.unlink()
        TX.sync_directory(mask.parent)
        C.command('/usr/bin/systemctl', 'daemon-reload')
        U.stable(m, r, installed=True)
        for role in ('core', 'membership'):
            name = U.P.ROLE_NAMES[role] + '.service'
            value = C.status(name)
            before = a['baseline']['states'][role]
            if (value['FragmentPath'] != before['FragmentPath'] or value['DropInPaths'] != before['DropInPaths']
                    or value['NeedDaemonReload'] != 'no'):
                raise ValueError('effective service definition changed')
            M.start_identity(value, 'lightning-goats' if role == 'core' else 'cyberherd')
        for unit, user, program, health in [
            (C.UNIT_NAME, 'lightning-goats', 'lightning-goatsd', 'http://127.0.0.1:8787/healthz'),
            ('cyberherd.service', 'cyberherd', 'cyberherdd', 'http://127.0.0.1:3340/api/v1/cyberherd'),
        ]:
            C.command('/usr/bin/systemctl', 'start', '--', unit, timeout=120)
            M.running(unit, user, Q.BIN / program)
            M.health(health)
        for role in ('cyberherd-payout', 'cyberherd-signer'):
            if not M.masked(U.MASKS / (role + '.service')) or C.status(role + '.service')['MainPID'] != '0':
                raise ValueError('outgoing role changed')
        M.note(directory, 'RESUMED.json', {'core_running': True, 'membership_running': True,
            'self_reads_configured': True, 'operator_access_enabled': False, 'payouts_enabled': False})
    return {'core_running': True, 'membership_running': True, 'self_reads_configured': True,
            'operator_access_enabled': False, 'payouts_enabled': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    preparing = commands.add_parser('prepare')
    preparing.add_argument('directory', type=Path)
    preparing.add_argument('--public-origin', required=True)
    applying = commands.add_parser('apply')
    applying.add_argument('directory', type=Path)
    applying.add_argument('--review-sha256', required=True)
    applying.add_argument('--exclusive-maintenance', required=True, action='store_true')
    resuming = commands.add_parser('resume')
    resuming.add_argument('directory', type=Path)
    resuming.add_argument('--approve-resumption', required=True, action='store_true')
    args = parser.parse_args()
    if args.operation == 'prepare':
        result = prepare(args.directory, args.public_origin)
    elif args.operation == 'apply':
        result = apply(args.directory, args.review_sha256, args.exclusive_maintenance)
    else:
        result = resume(args.directory, args.approve_resumption)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
