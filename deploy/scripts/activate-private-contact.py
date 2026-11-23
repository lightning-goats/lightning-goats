#!/usr/bin/env python3
"""Prepare and explicitly activate only private contact configuration.

Authenticated root-owned launcher only. Native validation permits one added
contact table and no other configuration change. Apply fences all four writers,
verifies a current schema-24 backup and replaces only the core configuration.
Resume preserves existing programs, payment/feeder policy and outgoing masks.
No nginx, static flag, credential, monetary mutation or database restore exists.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import importlib.util
import json
from pathlib import Path
import re
import tomllib


spec = importlib.util.spec_from_file_location('contact_schema_support', Path(__file__).with_name('maintain-contact-upgrade.py'))
U = importlib.util.module_from_spec(spec)
spec.loader.exec_module(U)
U.verified_tool_identity = lambda: verified_tool_identity()
C, Q, S, M, TX = U.C, U.Q, U.S, U.M, U.TX


def validate(validator, original, candidate):
    value = json.loads(C.command(str(validator), str(original), str(candidate)))
    expected = {'accepted': True, 'configuration_only': True, 'only_contact_configuration_added': True,
                'public_payment_policy_unchanged': True, 'ledger_opened': False, 'credentials_loaded': False,
                'provider_operations': 0, 'services_changed': False}
    if (set(value) != set(expected)
            or any(type(value[name]) is not type(wanted) or value[name] != wanted for name, wanted in expected.items())):
        raise ValueError('native contact-only configuration validation refused')
    return value


def prepare(directory, validator, expected, relays):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    if (directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None
            or re.fullmatch('[0-9a-f]{64}', expected) is None or not 1 <= len(relays) <= 8):
        raise ValueError('new private directory, independent checker hash and bounded inbox policy required')
    C.protected(validator)
    if U.digest(validator) != expected or TX.fingerprint(validator)['mode'] != 0o755:
        raise ValueError('native checker identity differs')
    baseline = U.P.baseline()
    if (baseline['states']['core']['FragmentPath'] != str(C.UNIT)
            or baseline['states']['membership']['FragmentPath'] != str(Q.UNITS / 'cyberherd.service')
            or (U.MASKS / 'cyberherd.service').exists() or (U.MASKS / 'cyberherd.service').is_symlink()):
        raise ValueError('original core and membership unit topology required')
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    TX.copy_durable(C.CONFIG, directory / 'original-core.toml', baseline['files'][str(C.CONFIG)])
    TX.copy_durable(C.UNIT, directory / 'original-unit', baseline['files'][str(C.UNIT)])
    core = tomllib.loads((directory / 'original-core.toml').read_text())
    core['contact'] = {'recipient': baseline['policy']['owner'], 'inbox_relays': relays}
    Q.I.publish(directory / 'candidate-core.toml', S.W.toml_text(core).encode(), 0o600)
    accepted = validate(validator, directory / 'original-core.toml', directory / 'candidate-core.toml')
    payload = directory / 'payload'
    payload.mkdir(mode=0o700)
    program = Q.BIN / 'cyberherd-statectl'
    TX.copy_durable(program, payload / program.name, baseline['files'][str(program)])

    def fresh():
        if U.P.baseline() != baseline:
            raise ValueError('installation or process changed during contact preparation')

    rows = TX.prepare(directory / 'configuration', [
        (C.CONFIG, directory / 'candidate-core.toml', baseline['files'][str(C.CONFIG)])
    ], fresh)
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    fresh()
    C.write_json(directory / 'PREPARED.json', {
        'version': 1, 'tool': verified_tool_identity(), 'baseline': baseline,
        'validator': str(validator), 'validator_sha256': expected, 'native_validation': accepted,
        'configurations': rows, 'configuration_manifest_sha256': U.digest(directory / 'configuration/TRANSACTION.json'),
        'candidate_sha256': U.digest(directory / 'candidate-core.toml'),
        'prepared_only': True, 'services_changed': False, 'journals_opened': False, 'contact_enabled': False,
    })
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'services_changed': False, 'journals_opened': False, 'contact_enabled': False}


def load(directory):
    C.root()
    U.private_directory(directory)
    if (directory / 'INCOMPLETE').exists():
        raise ValueError('incomplete activation preparation retained')
    a = C.read_json(directory / 'PREPARED.json')
    if (a['version'] != 1 or a['tool'] != verified_tool_identity() or a['prepared_only'] is not True
            or any(a[key] is not False for key in ('services_changed', 'journals_opened', 'contact_enabled'))):
        raise ValueError('contact activation preparation identity changed')
    validator = Path(a['validator'])
    C.protected(validator)
    if U.digest(validator) != a['validator_sha256']:
        raise ValueError('native checker changed')
    for name, fingerprint in [('original-core.toml', a['baseline']['files'][str(C.CONFIG)]),
                              ('original-unit', a['baseline']['files'][str(C.UNIT)])]:
        C.protected(directory / name)
        if TX.fingerprint(directory / name) != fingerprint:
            raise ValueError('original configuration/unit backup changed')
    if U.digest(directory / 'candidate-core.toml') != a['candidate_sha256']:
        raise ValueError('candidate configuration changed')
    if U.digest(directory / 'configuration/TRANSACTION.json') != a['configuration_manifest_sha256']:
        raise ValueError('configuration transaction changed')
    tx = C.read_json(directory / 'configuration/TRANSACTION.json')
    old = a['baseline']['files'][str(C.CONFIG)]
    new = dict(old, sha256=a['candidate_sha256'])
    expected = [{'destination': str(C.CONFIG), 'old': old, 'new': new}]
    if tx['version'] != 1 or tx['files'] != expected or a['configurations'] != expected:
        raise ValueError('only the exact core configuration may be replaced')
    for version in ('old', 'new'):
        if TX.fingerprint(directory / 'configuration' / ('0.' + version)) != expected[0][version]:
            raise ValueError('configuration transaction payload changed')
    program = directory / 'payload/cyberherd-statectl'
    C.protected(program)
    if TX.fingerprint(program) != a['baseline']['files'][str(Q.BIN / 'cyberherd-statectl')]:
        raise ValueError('native state verifier changed')
    # Reuse the qualified maintenance file/writer fences with one caller-validated
    # configuration row. No arbitrary replacement path enters these adapters.
    m = {'programs': expected, 'review': str(directory)}
    r = {'baseline': a['baseline']}
    return a, m, r


def apply(directory, expected, approved):
    if not approved:
        raise ValueError('explicit exclusive maintenance approval required')
    a, m, r = load(directory)
    if U.digest(directory / 'PREPARED.json') != expected:
        raise ValueError('independently reviewed preparation digest required')
    with M.exclusive(directory):
        if (directory / 'MAINTENANCE-STARTED.json').exists():
            raise ValueError('activation already began; preserve evidence and inspect')
        if U.P.baseline() != a['baseline']:
            raise ValueError('reviewed installation or process changed')
        if validate(Path(a['validator']), directory / 'original-core.toml', directory / 'candidate-core.toml') != a['native_validation']:
            raise ValueError('native validation changed')
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
        if validate(Path(a['validator']), directory / 'original-core.toml', C.CONFIG) != a['native_validation']:
            raise ValueError('installed contact configuration failed native validation')
        M.note(directory, 'APPLIED.json', {'native_backup': accepted, 'live_images': observed,
                'backup_manifest_sha256': U.digest(directory / 'backup/MANIFEST.json'),
                'configuration_sha256': m['programs'][0]['new']['sha256'], 'services_started': False})
    return {'configured': True, 'current_four_journal_backup_verified': True,
            'financial_images_unchanged': True, 'services_started': False}


def resume(directory, approved):
    if not approved:
        raise ValueError('explicit service resumption approval required')
    a, m, r = load(directory)
    with M.exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists():
            raise ValueError('resumption already began; inspect instead of restarting')
        applied = C.read_json(directory / 'APPLIED.json')
        U.sources_match(m, r, applied['live_images'], installed=True)
        if (U.digest(directory / 'backup/MANIFEST.json') != applied['backup_manifest_sha256']
                or U.native(m, 'verify', '--bundle', directory / 'backup') != applied['native_backup']):
            raise ValueError('current activation backup changed')
        if validate(Path(a['validator']), directory / 'original-core.toml', C.CONFIG) != a['native_validation']:
            raise ValueError('native activation validation changed')
        M.note(directory, 'RESUME-REQUESTED.json', {'approved': True, 'contact_enabled': True, 'payouts_enabled': False})
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
            old = a['baseline']['states'][role]
            if (value['FragmentPath'] != old['FragmentPath'] or value['DropInPaths'] != old['DropInPaths']
                    or value['NeedDaemonReload'] != 'no'):
                raise ValueError('effective service definition changed')
            M.start_identity(value, 'lightning-goats' if role == 'core' else 'cyberherd')
        C.command('/usr/bin/systemctl', 'start', '--', C.UNIT_NAME, timeout=120)
        M.running(C.UNIT_NAME, 'lightning-goats', Q.BIN / 'lightning-goatsd')
        M.health('http://127.0.0.1:8787/healthz')
        C.command('/usr/bin/systemctl', 'start', '--', 'cyberherd.service', timeout=120)
        M.running('cyberherd.service', 'cyberherd', Q.BIN / 'cyberherdd')
        M.health('http://127.0.0.1:3340/api/v1/cyberherd')
        for role in ('cyberherd-payout', 'cyberherd-signer'):
            if not M.masked(U.MASKS / (role + '.service')) or C.status(role + '.service')['MainPID'] != '0':
                raise ValueError('outgoing role changed')
        M.note(directory, 'RESUMED.json', {'core_running': True, 'membership_running': True,
                'contact_enabled': True, 'payouts_enabled': False})
    return {'core_running': True, 'membership_running': True, 'contact_enabled': True, 'payouts_enabled': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    preparation = commands.add_parser('prepare')
    preparation.add_argument('directory', type=Path)
    preparation.add_argument('--validator', type=Path, required=True)
    preparation.add_argument('--validator-sha256', required=True)
    preparation.add_argument('--inbox-relay', action='append', required=True)
    applying = commands.add_parser('apply')
    applying.add_argument('directory', type=Path)
    applying.add_argument('--review-sha256', required=True)
    applying.add_argument('--exclusive-maintenance', required=True, action='store_true')
    resuming = commands.add_parser('resume')
    resuming.add_argument('directory', type=Path)
    resuming.add_argument('--approve-resumption', required=True, action='store_true')
    args = parser.parse_args()
    if args.operation == 'prepare':
        result = prepare(args.directory, args.validator, args.validator_sha256, args.inbox_relay)
    elif args.operation == 'apply':
        result = apply(args.directory, args.review_sha256, args.exclusive_maintenance)
    else:
        result = resume(args.directory, args.approve_resumption)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
