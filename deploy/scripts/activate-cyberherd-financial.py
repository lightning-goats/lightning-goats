#!/usr/bin/env python3
"""First financial configuration transaction; no program/schema/credential upgrade.

Authenticated root-owned launcher only. Plan consumes a sealed financial candidate
and a separately reviewed completed six-program upgrade. Checkpoint stops/masks
four writers and takes a fresh native backup/reward review. Applying configuration
and starting financial roles require separate explicit operator steps. Approved
financial resumption can cause bounded payouts/signatures and resume existing
feeding. The coordinator itself has no provider/signing/feeder RPC, database
restore or recovery-clearance command.
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
import os
from pathlib import Path
import re
import tempfile
import tomllib
import uuid

SPEC = importlib.util.spec_from_file_location('financial_candidate_support', Path(__file__).with_name('prepare-cyberherd-financial-stage.py'))
F = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(F)
F.verified_tool_identity = lambda: verified_tool_identity()
U, C, Q, S, TX = F.U, F.C, F.Q, F.S, F.TX
M = U.M
PROGRAMS = ('lightning-goatsd', 'lightning-goatsctl', 'cyberherd-statectl',
            'cyberherdd', 'cyberherd-payoutd', 'cyberherd-signerd')
WORKERS = {'membership': ('cyberherdd', 'CyberHerd configuration is valid; no runtime started'),
           'payout': ('cyberherd-payoutd', 'Payout configuration is valid; no runtime started'),
           'signer': ('cyberherd-signerd', 'Outgoing signer configuration is valid; no runtime started')}


def configurations():
    return {'core': C.CONFIG, **{role: Q.ETC / name / 'config.toml'
                                 for role, name in U.P.ROLE_NAMES.items() if role != 'core'}}


def sealed(path, expected):
    U.private_directory(path)
    if (re.fullmatch('[0-9a-f]{64}', expected) is None
            or U.digest(path / 'PREPARED.json') != expected or (path / 'INCOMPLETE').exists()):
        raise ValueError('independently reviewed complete preparation digest required')
    return C.read_json(path / 'PREPARED.json')


def program_review(path, expected, baseline):
    """Read an older qualified receipt as data, never interpret its tool code."""
    r = sealed(path, expected)
    if (r.get('version') != 1 or r.get('program_upgrade_only') is not True
            or any(r.get(k) is not False for k in ('activation_authorized', 'schema_upgrade_authorized',
                                                  'services_changed', 'journals_opened', 'credentials_loaded'))
            or re.fullmatch('[0-9a-f]{40}', r.get('source_commit', '')) is None
            or set(r['selected_program_sha256']) != set(PROGRAMS)
            or (path / 'ROLLBACK-REQUESTED.json').exists()):
        raise ValueError('reviewed selected-program maintenance receipt required')
    resumed = C.read_json(path / 'RESUMED.json')
    if (resumed.get('core_running') is not True or resumed.get('membership_running') is not True
            or any(resumed.get(k) is not False for k in ('original_programs', 'payouts_enabled', 'configurations_changed'))
            or r['baseline']['identities'] != baseline['identities']
            or any(r['baseline']['policy'][k] != baseline['policy'][k] for k in ('owner', 'epoch', 'provider'))):
        raise ValueError('completed program upgrade with original financial identity required')
    for name in PROGRAMS:
        fp = baseline['files'][str(Q.BIN / name)]
        if fp['sha256'] != r['selected_program_sha256'][name]:
            raise ValueError('installed program differs from the separately qualified program review')
    return r


def candidate_review(path, expected):
    r = sealed(path, expected)
    if (r.get('version') != 1 or r.get('tool') != verified_tool_identity()
            or r.get('financial_candidate_only') is not True
            or any(r.get(k) is not False for k in ('activation_authorized', 'services_changed', 'journals_opened'))
            or r.get('backlog_review_required') is not True or r.get('provider_qualification_required') is not True):
        raise ValueError('matching explicit financial candidate required')
    F.validate_limits(r['limits'])
    original, candidate, rows = {}, {}, []
    for role, target in configurations().items():
        saved, selected = path / ('original-' + role + '.toml'), path / ('candidate-' + role + '.toml')
        C.protected(saved); C.protected(selected)
        if TX.fingerprint(saved) != r['baseline']['files'][str(target)]:
            raise ValueError('financial candidate original changed')
        original[role], candidate[role] = tomllib.loads(saved.read_text()), tomllib.loads(selected.read_text())
        rows.append({'destination': str(target), 'original': r['baseline']['files'][str(target)],
                     'candidate_sha256': U.digest(selected)})
    if (r['configurations'] != rows
            or F.render(original, r['baseline']['identities'], r['plans_group_gid'], r['limits']) != candidate
            or F.check_report(json.dumps(r['native_validation'])) != r['native_validation']):
        raise ValueError('financial candidate differs from its exact bounded rendering')
    C.protected(path / 'native-checker')
    if TX.fingerprint(path / 'native-checker') != r['native_checker']:
        raise ValueError('independently selected financial checker changed')
    U.P.configuration_boundary(*(original[k] for k in U.P.ROLE_NAMES), r['baseline']['policy'], current_schema=True)
    return r


def fresh(r):
    if U.P.baseline(current_schema=True) != r['baseline']:
        raise ValueError('original running zero-budget installation changed')
    if C.grp.getgrnam('cyberherd-plans').gr_gid != r['plans_group_gid']:
        raise ValueError('original plan-export group changed')
    F.observed_allocation(r['baseline']['policy']['owner'], r['limits']['member_percent'])


def native_checks(directory, r):
    F.native_validation(directory, r['baseline'], directory / 'native-checker', r['native_checker'])
    C.protected(Path('/run'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-financial-workers-', dir='/run'))
    carrier.chmod(0o711)
    try:
        for role, (name, positive) in WORKERS.items():
            identity = r['baseline']['identities'][role]
            installed = Q.BIN / name
            C.protected(installed)
            fp = r['baseline']['files'][str(installed)]
            if TX.fingerprint(installed) != fp:
                raise ValueError('qualified installed native worker changed')
            program, config = carrier / name, carrier / (role + '.toml')
            TX.copy_durable(installed, program, fp)
            selected = directory / ('candidate-' + role + '.toml')
            Q.I.publish(config, selected.read_bytes(), 0o440)
            os.chown(config, 0, identity['gid'])
            result = C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
                '--reuid=' + str(identity['uid']), '--regid=' + str(identity['gid']), '--clear-groups',
                '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
                str(program), '--config', str(config), '--check', timeout=30)
            if (result.strip() != positive or TX.fingerprint(installed) != fp
                    or config.read_bytes() != selected.read_bytes()):
                raise ValueError('qualified native worker did not accept the exact financial candidate')
    finally:
        carrier.chmod(0o700)
        TX.sync_directory(carrier)
    return {'accepted': True, 'four_role_candidate_checked': True, 'original_worker_uids': True,
            'journals_opened': False, 'credentials_loaded': False}


def plan(candidate, candidate_sha, programs, program_sha, directory):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    r = candidate_review(candidate, candidate_sha)
    program_review(programs, program_sha, r['baseline'])
    if directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None:
        raise ValueError('new direct private financial execution directory required')
    fresh(r)
    if (r['baseline']['states']['core']['FragmentPath'] != str(C.UNIT)
            or r['baseline']['states']['membership']['FragmentPath'] != str(Q.UNITS / 'cyberherd.service')
            or (U.MASKS / 'cyberherd.service').exists() or (U.MASKS / 'cyberherd.service').is_symlink()):
        raise ValueError('original running core/member unit topology required')
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    for index, name in enumerate(r['baseline']['files']):
        TX.copy_durable(Path(name), directory / f'original-{index}', r['baseline']['files'][name])
    for role in configurations():
        selected = candidate / ('candidate-' + role + '.toml')
        TX.copy_durable(selected, directory / selected.name, TX.fingerprint(selected))
    TX.copy_durable(candidate / 'native-checker', directory / 'native-checker', r['native_checker'])
    payload = directory / 'payload'
    payload.mkdir(mode=0o700)
    native = Q.BIN / 'cyberherd-statectl'
    TX.copy_durable(native, payload / native.name, r['baseline']['files'][str(native)])
    checked = native_checks(directory, r)
    rows = TX.prepare(directory / 'configuration', [
        (target, directory / ('candidate-' + role + '.toml'), r['baseline']['files'][str(target)])
        for role, target in configurations().items()
    ], lambda: fresh(r))
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    fresh(r)
    C.write_json(directory / 'PREPARED.json', {'version': 1, 'tool': verified_tool_identity(),
        'first_financial_transaction': True, 'candidate': str(candidate), 'candidate_sha256': candidate_sha,
        'program_review': str(programs), 'program_review_sha256': program_sha,
        'baseline': r['baseline'], 'limits': r['limits'], 'original_order': list(r['baseline']['files']),
        'configurations': rows, 'configuration_manifest_sha256': U.digest(directory / 'configuration/TRANSACTION.json'),
        'native_checks': checked, 'activation_authorized': False, 'services_changed': False, 'journals_opened': False})
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'planned': True, 'configurations': 4, 'activation_authorized': False,
            'services_changed': False, 'journals_opened': False}


def load(directory, expected):
    C.root()
    a = sealed(directory, expected)
    if (a.get('version') != 1 or a.get('tool') != verified_tool_identity()
            or a.get('first_financial_transaction') is not True
            or any(a.get(k) is not False for k in ('activation_authorized', 'services_changed', 'journals_opened'))):
        raise ValueError('matching first financial transaction required')
    r = candidate_review(Path(a['candidate']), a['candidate_sha256'])
    program_review(Path(a['program_review']), a['program_review_sha256'], r['baseline'])
    if a['baseline'] != r['baseline'] or a['limits'] != r['limits']:
        raise ValueError('reviewed financial identity or limits changed')
    if (set(a['original_order']) != set(a['baseline']['files'])
            or len(a['original_order']) != len(set(a['original_order']))):
        raise ValueError('original financial installation inventory changed')
    for index, name in enumerate(a['original_order']):
        saved = directory / f'original-{index}'
        C.protected(saved)
        if TX.fingerprint(saved) != a['baseline']['files'][name]:
            raise ValueError('original protected financial file changed')
    expected_rows = []
    for role, target in configurations().items():
        selected = directory / ('candidate-' + role + '.toml')
        C.protected(selected)
        if selected.read_bytes() != (Path(a['candidate']) / selected.name).read_bytes():
            raise ValueError('selected financial configuration changed')
        old = a['baseline']['files'][str(target)]
        expected_rows.append({'destination': str(target), 'old': old, 'new': dict(old, sha256=U.digest(selected))})
    if (U.digest(directory / 'configuration/TRANSACTION.json') != a['configuration_manifest_sha256']
            or a['configurations'] != expected_rows
            or C.read_json(directory / 'configuration/TRANSACTION.json') != {'version': 1, 'files': expected_rows}):
        raise ValueError('only the four exact financial configurations may be replaced')
    for index, row in enumerate(expected_rows):
        for version in ('old', 'new'):
            path = directory / 'configuration' / f'{index}.{version}'
            C.protected(path)
            if TX.fingerprint(path) != row[version]:
                raise ValueError('financial replacement evidence changed')
    for path, fp in [(directory / 'native-checker', r['native_checker']),
                     (directory / 'payload/cyberherd-statectl', r['baseline']['files'][str(Q.BIN / 'cyberherd-statectl')])]:
        C.protected(path)
        if TX.fingerprint(path) != fp:
            raise ValueError('qualified native financial checker changed')
    return a, {'programs': expected_rows, 'review': str(directory)}, r


def reward_review(directory, m, r, observed):
    result = json.loads(C.command(str(directory / 'payload/cyberherd-statectl'), '--config', str(S.MAINTENANCE),
        'review-rewards', '--bundle', str(directory / 'backup'), '--destination', str(directory / 'reward-review'), timeout=300))
    if (set(result) != {'operation', 'id', 'accepted', 'activation_authorized'}
            or result['operation'] != 'review_rewards' or result['accepted'] is not True
            or result['activation_authorized'] is not False or str(uuid.UUID(result['id'])) != result['id']):
        raise ValueError('native financial backlog review refused')
    C.protected(directory / 'reward-review', directory=True, private=True)
    report = C.read_json(directory / 'reward-review/REVIEW.json')
    manifest = C.read_json(directory / 'backup/MANIFEST.json')
    if ((directory / 'reward-review/INCOMPLETE').exists() or type(report.get('version')) is not int
            or report['version'] != 1 or report['id'] != result['id']
            or report['bundle_id'] != manifest['id']
            or report['bundle_manifest_sha256'] != U.digest(directory / 'backup/MANIFEST.json')
            or report['owner'] != r['baseline']['policy']['owner'] or report['epoch'] != r['baseline']['policy']['epoch']
            or report['all_barriers_examined'] is not True or report['activation_authorized'] is not False
            or report['external_reconciliation_required'] is not True):
        raise ValueError('native financial backlog scope or backup binding differs')
    U.sources_match(m, r, observed)
    return {'native': result, 'report_sha256': U.digest(directory / 'reward-review/REVIEW.json')}


def checkpoint(directory, expected, approved):
    if not approved:
        raise ValueError('explicit exclusive maintenance approval required')
    a, m, r = load(directory, expected)
    with M.exclusive(directory):
        if any((directory / n).exists() for n in ('MAINTENANCE-STARTED.json', 'RESUME-REQUESTED.json', 'ROLLBACK-REQUESTED.json')):
            raise ValueError('financial maintenance already began; inspect rather than restarting')
        fresh(r)
        if native_checks(directory, r) != a['native_checks']:
            raise ValueError('financial native acceptance changed')
        M.note(directory, 'MAINTENANCE-STARTED.json', {'exclusive_maintenance': True, 'database_restore_authorized': False})
        U.quiesce(m, r)
        observed = U.state_files(r)
        U.native(m, 'snapshot', '--destination', directory / 'backup', '--writers-stopped')
        accepted = U.native(m, 'verify', '--bundle', directory / 'backup')
        U.sources_match(m, r, observed)
        review = reward_review(directory, m, r, observed)
        M.note(directory, 'CHECKPOINT.json', {'native_backup': accepted, 'live_images': observed,
            'backup_manifest_sha256': U.digest(directory / 'backup/MANIFEST.json'), 'reward_review': review,
            'configurations_changed': False, 'activation_authorized': False})
    return {'checkpointed': True, 'current_native_backup_verified': True, 'private_reward_review_ready': True,
            'configurations_changed': False, 'services_started': False, 'activation_authorized': False}


def checked_checkpoint(directory, checkpoint_sha, review_sha, m, r, installed):
    if (re.fullmatch('[0-9a-f]{64}', checkpoint_sha) is None
            or re.fullmatch('[0-9a-f]{64}', review_sha) is None
            or U.digest(directory / 'CHECKPOINT.json') != checkpoint_sha):
        raise ValueError('independently reviewed fresh checkpoint and reward report digests required')
    value = C.read_json(directory / 'CHECKPOINT.json')
    if (value.get('configurations_changed') is not False or value.get('activation_authorized') is not False
            or value['reward_review']['report_sha256'] != review_sha
            or U.digest(directory / 'reward-review/REVIEW.json') != review_sha
            or U.digest(directory / 'backup/MANIFEST.json') != value['backup_manifest_sha256']
            or U.native(m, 'verify', '--bundle', directory / 'backup') != value['native_backup']):
        raise ValueError('accepted financial checkpoint or backup changed')
    U.sources_match(m, r, value['live_images'], installed=installed)
    return value, C.read_json(directory / 'reward-review/REVIEW.json')


def first_activation_backlog(report, limits):
    """No retrospective allocation, old reservation adoption or skipped intake."""
    names = ('unreceived_payment_events', 'unreceived_feeding_events', 'pending_feeding_ingress',
             'unfinalized_core_reservations', 'unresolved_provider_payments', 'payout_held_sats')
    if (report.get('all_barriers_examined') is not True or report.get('activation_authorized') is not False
            or report.get('external_reconciliation_required') is not True
            or type(report.get('stored_member_percent')) is not int
            or report['stored_member_percent'] != limits['member_percent']
            or type(report.get('stored_max_members')) is not int or report['stored_max_members'] != 3
            or any(type(report.get(name)) is not int or report[name] != 0 for name in names)):
        raise ValueError('first activation requires the selected stored allocation and classified financial intake without existing holds')
    allowed = {'unplanned_zero_allocation', 'unplanned_no_strike_funding', 'unplanned_funded_allocation',
               'no_payout', 'awaiting_reservation', 'reserved'}
    stages = report.get('stages')
    if (not isinstance(stages, dict) or not set(stages) <= allowed
            or any(type(v) is not int or v < 0 for v in stages.values())
            or any(stages.get(name, 0) for name in ('unplanned_funded_allocation', 'awaiting_reservation', 'reserved'))):
        raise ValueError('existing funded reward work requires a separate explicit accounting decision')


def apply(directory, expected, checkpoint_sha, review_sha, approved):
    if not approved:
        raise ValueError('explicit continuing exclusive maintenance approval required')
    a, m, r = load(directory, expected)
    with M.exclusive(directory):
        if any((directory / n).exists() for n in ('APPLICATION-STARTED.json', 'RESUME-REQUESTED.json', 'ROLLBACK-REQUESTED.json')):
            raise ValueError('financial application already began; inspect rather than reapply')
        checked, report = checked_checkpoint(directory, checkpoint_sha, review_sha, m, r, False)
        first_activation_backlog(report, r['limits'])
        if native_checks(directory, r) != a['native_checks']:
            raise ValueError('financial native acceptance changed before configuration application')
        M.note(directory, 'APPLICATION-STARTED.json', {'checkpoint_sha256': checkpoint_sha,
            'reward_review_sha256': review_sha, 'database_restore_authorized': False})
        TX.execute(directory / 'configuration', lambda: U.sources_match(m, r, checked['live_images'], installed=None))
        checked_checkpoint(directory, checkpoint_sha, review_sha, m, r, True)
        M.note(directory, 'APPLIED.json', {'checkpoint_sha256': checkpoint_sha, 'reward_review_sha256': review_sha,
            'limits': r['limits'], 'financial_images_unchanged': True, 'configurations': 4, 'services_started': False})
    return {'configured': True, 'configurations': 4, 'financial_images_unchanged': True, 'services_started': False}


def rollback(directory, expected, checkpoint_sha, review_sha, approved):
    if not approved:
        raise ValueError('explicit pre-resumption configuration rollback approval required')
    a, m, r = load(directory, expected)
    with M.exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists():
            raise ValueError('financial resumption already began; automatic rollback is refused')
        checked, _ = checked_checkpoint(directory, checkpoint_sha, review_sha, m, r, None)
        M.note(directory, 'ROLLBACK-REQUESTED.json', {'configurations_only': True, 'database_restore_authorized': False})
        TX.execute(directory / 'configuration', lambda: U.sources_match(m, r, checked['live_images'], installed=None), rollback=True)
        checked_checkpoint(directory, checkpoint_sha, review_sha, m, r, False)
        M.note(directory, 'ROLLED-BACK.json', {'configurations_restored': True, 'database_images_restored': False})
    return {'configurations_restored': True, 'database_images_restored': False, 'services_started': False}


def restore_units(directory, a, r, original):
    M.restore_unit(U.original(directory, a, C.UNIT), r['baseline']['files'][str(C.UNIT)])
    for role in ('membership', *(() if original else ('signer', 'payout'))):
        mask = U.MASKS / (U.P.ROLE_NAMES[role] + '.service')
        if not M.masked(mask):
            raise ValueError('selected role mask changed before financial resumption')
        mask.unlink()
        TX.sync_directory(mask.parent)
    C.command('/usr/bin/systemctl', 'daemon-reload')
    U.stable({'programs': a['configurations']}, r, installed=not original)
    for role, name in U.P.ROLE_NAMES.items():
        if original and role in ('payout', 'signer'):
            continue
        value = C.status(name + '.service')
        expected_unit = C.UNIT if role == 'core' else Q.UNITS / (name + '.service')
        if value['FragmentPath'] != str(expected_unit) or value['NeedDaemonReload'] != 'no':
            raise ValueError('effective financial service unit changed')
        if role in ('core', 'membership') and value['DropInPaths'] != r['baseline']['states'][role]['DropInPaths']:
            raise ValueError('effective core/member override changed')
        if any(path not in r['baseline']['files'] for path in value['DropInPaths'].split()):
            raise ValueError('unreviewed effective service override appeared')
        M.start_identity(value, 'lightning-goats' if role == 'core' else name)


def enabled_role(role):
    name = U.P.ROLE_NAMES[role] + '.service'
    link = U.MASKS / 'multi-user.target.wants' / name
    C.protected(link.parent, directory=True)
    if (not link.is_symlink() or link.lstat().st_uid != 0 or link.lstat().st_gid != 0
            or link.resolve() != Q.UNITS / name or C.status(name)['UnitFileState'] != 'enabled'):
        raise ValueError('selected financial role lacks its exact persistent unit enablement')


def resume(directory, expected, checkpoint_sha, review_sha, approved, reviewed, original=False):
    if not approved or (not original and not reviewed):
        raise ValueError('explicit financial activation and provider/recipient review acknowledgment required')
    a, m, r = load(directory, expected)
    with M.exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists():
            raise ValueError('resumption already began; inspect without automatic restart or rollback')
        if original:
            if C.read_json(directory / 'ROLLED-BACK.json') != {'configurations_restored': True, 'database_images_restored': False}:
                raise ValueError('original zero-budget configuration rollback must be complete')
        elif (directory / 'ROLLBACK-REQUESTED.json').exists():
            raise ValueError('rollback was selected; financial activation refused')
        checked, report = checked_checkpoint(directory, checkpoint_sha, review_sha, m, r, not original)
        if not original:
            applied = C.read_json(directory / 'APPLIED.json')
            if (applied.get('checkpoint_sha256') != checkpoint_sha or applied.get('reward_review_sha256') != review_sha
                    or applied.get('limits') != r['limits'] or applied.get('financial_images_unchanged') is not True
                    or applied.get('services_started') is not False or applied.get('configurations') != 4):
                raise ValueError('financial application receipt changed')
            first_activation_backlog(report, r['limits'])
            if native_checks(directory, r) != a['native_checks']:
                raise ValueError('financial native acceptance changed before activation')
        # An acknowledgment is operator attestation, NOT provider/payment proof.
        # No automatic code path may turn configuration checks into this approval.
        M.note(directory, 'RESUME-REQUESTED.json', {'approved': True, 'original_configuration': original,
            'provider_recipient_review_acknowledged': reviewed if not original else False,
            'checkpoint_sha256': checkpoint_sha, 'reward_review_sha256': review_sha})
        restore_units(directory, a, r, original)
        if not original:
            C.command('/usr/bin/systemctl', 'enable', '--', 'cyberherd-signer.service', 'cyberherd-payout.service')
            for role in ('signer', 'payout'):
                enabled_role(role)
        order = [('core', 'lightning-goatsd', 'http://127.0.0.1:8787/healthz')]
        if not original:
            order.append(('signer', 'cyberherd-signerd', None))
        order.append(('membership', 'cyberherdd', 'http://127.0.0.1:3340/api/v1/cyberherd'))
        if not original:
            order.append(('payout', 'cyberherd-payoutd', None))
        for role, program, health in order:
            name = U.P.ROLE_NAMES[role] + '.service'
            user = 'lightning-goats' if role == 'core' else U.P.ROLE_NAMES[role]
            C.command('/usr/bin/systemctl', 'start', '--', name, timeout=120)
            M.running(name, user, Q.BIN / program)
            if health:
                M.health(health)
        U.stable(m, r, installed=not original)
        for role in ('core', 'membership', 'signer', 'payout'):
            if original and role in ('signer', 'payout'):
                value = C.status(U.P.ROLE_NAMES[role] + '.service')
                if value['LoadState'] != 'masked' or value['UnitFileState'] != 'masked' or value['MainPID'] != '0':
                    raise ValueError('outgoing role changed during original resumption')
            else:
                name = U.P.ROLE_NAMES[role]
                program = {'core': 'lightning-goatsd', 'membership': 'cyberherdd',
                           'signer': 'cyberherd-signerd', 'payout': 'cyberherd-payoutd'}[role]
                M.running(name + '.service', 'lightning-goats' if role == 'core' else name, Q.BIN / program)
                if role in ('signer', 'payout'):
                    enabled_role(role)
        M.note(directory, 'RESUMED.json', {'core_running': True, 'membership_running': True,
            'outgoing_roles_started': not original, 'outgoing_roles_enabled': not original, 'original_configuration': original,
            'provider_execution_qualified': False, 'external_recovery_adopted': False})
    return {'core_running': True, 'membership_running': True, 'outgoing_roles_started': not original, 'outgoing_roles_enabled': not original,
            'original_configuration': original, 'provider_execution_qualified': False, 'external_recovery_adopted': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    planning = commands.add_parser('plan')
    for name in ('candidate', 'program_review', 'directory'):
        planning.add_argument(name, type=Path)
    planning.add_argument('--candidate-sha256', required=True)
    planning.add_argument('--program-review-sha256', required=True)
    for name in ('checkpoint', 'apply', 'rollback', 'resume', 'resume-original'):
        command = commands.add_parser(name)
        command.add_argument('directory', type=Path)
        command.add_argument('--review-sha256', required=True)
        if name != 'checkpoint':
            command.add_argument('--checkpoint-sha256', required=True)
            command.add_argument('--reward-review-sha256', required=True)
        if name in ('checkpoint', 'apply'):
            command.add_argument('--exclusive-maintenance', required=True, action='store_true')
        elif name == 'rollback':
            command.add_argument('--approve-configuration-rollback', required=True, action='store_true')
        elif name == 'resume-original':
            command.add_argument('--approve-original-resumption', required=True, action='store_true')
        else:
            command.add_argument('--approve-financial-activation', required=True, action='store_true')
            command.add_argument('--acknowledge-provider-recipient-review', required=True, action='store_true')
    args = parser.parse_args()
    if args.operation == 'plan':
        result = plan(args.candidate, args.candidate_sha256, args.program_review, args.program_review_sha256, args.directory)
    elif args.operation == 'checkpoint':
        result = checkpoint(args.directory, args.review_sha256, args.exclusive_maintenance)
    else:
        common = (args.directory, args.review_sha256, args.checkpoint_sha256, args.reward_review_sha256)
        if args.operation == 'apply':
            result = apply(*common, args.exclusive_maintenance)
        elif args.operation == 'rollback':
            result = rollback(*common, args.approve_configuration_rollback)
        elif args.operation == 'resume-original':
            result = resume(*common, args.approve_original_resumption, False, original=True)
        else:
            result = resume(*common, args.approve_financial_activation, args.acknowledge_provider_recipient_review)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
