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
    schema_review = r.get('program_upgrade_only') is False and r.get('inclusive_fee_upgrade') is True
    program_only = r.get('program_upgrade_only') is True and 'inclusive_fee_upgrade' not in r
    if (r.get('version') != 1 or not (program_only or schema_review)
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
    if schema_review:
        completed_inclusive_review(path)
    return r


def completed_inclusive_review(path):
    # The distinct schema transaction is accepted only after native verification
    # of its retained before/after images. Never interpret older review tool code
    # or open the live journals to qualify a financial preparation.
    applied = C.read_json(path / 'APPLIED.json')
    if (applied.get('inclusive_fee_upgrade') is not True
            or applied.get('retained_rows_unchanged') is not True
            or any(applied.get(k) is not False for k in
                   ('activation_authorized', 'services_started', 'configurations_changed'))):
        raise ValueError('completed inclusive-fee migration receipt required')
    for folder, field in (('backup', 'backup_manifest_sha256'), ('migrated', 'migrated_manifest_sha256')):
        manifest = path / folder / 'MANIFEST.json'
        C.protected(manifest)
        if U.digest(manifest) != applied.get(field):
            raise ValueError('completed inclusive-fee migration images changed')
    program = str(Q.BIN / 'cyberherd-statectl')
    common = (program, '--config', str(S.MAINTENANCE))
    source = json.loads(C.command(*common, 'verify-inclusive-fee-upgrade-source',
                                  '--bundle', str(path / 'backup'), timeout=300))
    verified = json.loads(C.command(*common, 'verify', '--bundle', str(path / 'migrated'), timeout=300))
    compared = json.loads(C.command(*common, 'compare-inclusive-fee-upgrade',
        '--bundle', str(path / 'backup'), '--candidate-copy', str(path / 'migrated'), timeout=300))
    backup = C.read_json(path / 'BACKUP-ACCEPTED.json')
    if (backup.get('inclusive_fee_upgrade') is not True or source != backup.get('selected_source')
            or verified != applied.get('verified') or compared != applied.get('compared')
            or backup.get('manifest_sha256') != applied['backup_manifest_sha256']):
        raise ValueError('completed inclusive-fee native comparison changed')


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
            # Execute the already reviewed root-owned installation. /run holds
            # only candidate data and need not permit executing copied programs.
            program, config = installed, carrier / (role + '.toml')
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


def cutoff_summary(value):
    fields = {'decision_id', 'epoch', 'through', 'backup_manifest_sha256', 'historical_feedings', 'decision_sha256'}
    if (not isinstance(value, dict) or set(value) != fields
            or any(type(value.get(k)) is not int or value[k] < 0 for k in ('through', 'historical_feedings'))
            or value['through'] > 2**63 - 1 or value['historical_feedings'] > min(16384, value['through'])
            or any(not isinstance(value.get(k), str) or re.fullmatch('[0-9a-f]{64}', value[k]) is None
                   for k in ('backup_manifest_sha256', 'decision_sha256'))):
        raise ValueError('exact bounded native cutoff summary required')
    for key in ('decision_id', 'epoch'):
        if not isinstance(value.get(key), str) or str(uuid.UUID(value[key])) != value[key] or uuid.UUID(value[key]).int == 0:
            raise ValueError('canonical nonnil cutoff scope required')
    return value


def completed_cutover_review(path, prepared_sha, applied_sha, baseline):
    """Older reviewed receipts are data; execute only the current installed verifier."""
    if path.name != 'reward-cutover':
        raise ValueError('completed cutoff transaction directory required')
    U.private_directory(path.parent)
    C.protected(path, directory=True, private=True)
    for name, expected in [('PREPARED.json', prepared_sha), ('APPLIED.json', applied_sha)]:
        C.protected(path / name, private=True)
        if not isinstance(expected, str) or re.fullmatch('[0-9a-f]{64}', expected) is None or U.digest(path / name) != expected:
            raise ValueError('independently reviewed completed cutoff digests required')
    if (path / 'INCOMPLETE').exists():
        raise ValueError('cutoff preparation is incomplete')
    p, a = (C.read_json(path / name) for name in ('PREPARED.json', 'APPLIED.json'))
    if (p.get('version') != 1 or a.get('version') != 1 or p.get('activation_authorized') is not False
            or a.get('cutoff_review_sha256') != prepared_sha or a.get('retained_rows_unchanged') is not True
            or any(a.get(k) is not False for k in ('configurations_changed', 'services_started', 'activation_authorized'))):
        raise ValueError('completed reward-only cutoff evidence required')
    original = sealed(path.parent, p['financial_review_sha256'])
    if (original.get('first_financial_transaction') is not True
            or any(original.get(k) is not False for k in ('activation_authorized', 'services_changed', 'journals_opened'))
            or original['baseline']['identities'] != baseline['identities']
            or any(original['baseline']['policy'][k] != baseline['policy'][k] for k in ('owner', 'epoch', 'provider'))
            or any((path.parent / n).exists() for n in ('APPLICATION-STARTED.json', 'RESUME-REQUESTED.json', 'ROLLBACK-REQUESTED.json'))
            or U.digest(path.parent / 'CHECKPOINT.json') != p['checkpoint_sha256']
            or U.digest(path.parent / 'reward-review/REVIEW.json') != p['reward_review_sha256']):
        raise ValueError('original cutoff checkpoint or financial scope changed')
    started = C.read_json(path / 'APPLICATION-STARTED.json')
    if (started.get('cutoff_review_sha256') != prepared_sha or started.get('activation_authorized') is not False
            or started.get('database_restore_authorized') is not False):
        raise ValueError('original reward-only application record changed')
    resumed = C.read_json(path / 'RESUMED.json')
    if (any(resumed.get(k) is not True for k in ('core_running', 'membership_running'))
            or any(resumed.get(k) is not False for k in ('configurations_changed', 'outgoing_roles_enabled', 'activation_authorized'))):
        raise ValueError('completed original-only cutoff resumption required')
    summary = cutoff_summary(a.get('initial_reward_cutover'))
    if summary['epoch'] != baseline['policy']['epoch']:
        raise ValueError('cutoff belongs to another core epoch')
    expected_receipt = {k: summary[k] for k in ('decision_id', 'epoch', 'through', 'historical_feedings')}
    expected_receipt['activation_authorized'] = False
    if a.get('receipt') != expected_receipt or p.get('expected_receipt') != expected_receipt:
        raise ValueError('immutable cutoff receipt differs')
    before, after = path.parent / 'backup', path / 'post/backup'
    if (U.digest(before / 'MANIFEST.json') != summary['backup_manifest_sha256']
            or U.digest(after / 'MANIFEST.json') != a['backup_manifest_sha256']
            or U.digest(path / 'post/reward-review/REVIEW.json') != a['reward_report_sha256']):
        raise ValueError('retained cutoff checkpoint bytes changed')
    program = Q.BIN / 'cyberherd-statectl'
    C.protected(program)
    expected_program = baseline['files'][str(program)]
    if TX.fingerprint(program) != expected_program:
        raise ValueError('current qualified cutoff verifier changed')
    common = (str(program), '--config', str(S.MAINTENANCE))
    verified = json.loads(C.command(*common, 'verify', '--bundle', str(after), timeout=300))
    compared = json.loads(C.command(*common, 'compare-initial-reward-cutover', '--bundle', str(before),
        '--preview', str(path / 'preview'), '--preview-sha256', p['preview_sha256'],
        '--candidate-copy', str(after), timeout=300))
    for value, operation in [(verified, 'verify'), (compared, 'initial_reward_cutover_compare')]:
        fields = {'operation', 'id', 'accepted', 'resume_authorized'} | ({'decision_sha256'} if operation == 'initial_reward_cutover_compare' else set())
        if (set(value) != fields or value.get('operation') != operation
                or value.get('accepted') is not True or value.get('resume_authorized') is not False
                or not isinstance(value.get('id'), str) or str(uuid.UUID(value['id'])) != value['id'] or uuid.UUID(value['id']).int == 0):
            raise ValueError('native completed-cutoff proof refused')
    if (verified != a['native_backup'] or compared != a['comparison']
            or compared.get('decision_sha256') != summary['decision_sha256'] or TX.fingerprint(program) != expected_program):
        raise ValueError('retained native cutoff comparison changed')
    # Recompute the summary from the accepted journals; a saved summary alone
    # cannot bind a different decision, even in otherwise valid receipts.
    C.protected(C.BASE, directory=True, private=True)
    with tempfile.TemporaryDirectory(prefix='lg-cutoff-proof-', dir=C.BASE) as folder:
        target = Path(folder) / 'review'
        ack = json.loads(C.command(*common, 'review-rewards', '--bundle', str(after),
                                  '--destination', str(target), timeout=300))
        if (set(ack) != {'operation', 'id', 'accepted', 'activation_authorized'}
                or ack.get('operation') != 'review_rewards' or ack.get('accepted') is not True
                or ack.get('activation_authorized') is not False or not isinstance(ack.get('id'), str)
                or str(uuid.UUID(ack['id'])) != ack['id'] or uuid.UUID(ack['id']).int == 0):
            raise ValueError('fresh native cutoff review refused')
        fresh_report = C.read_json(target / 'REVIEW.json')
        if (fresh_report.get('id') != ack.get('id') or fresh_report.get('initial_reward_cutover') != summary
                or fresh_report.get('bundle_id') != verified['id']
                or fresh_report.get('bundle_manifest_sha256') != a['backup_manifest_sha256']
                or fresh_report.get('owner') != baseline['policy']['owner'] or fresh_report.get('epoch') != summary['epoch']
                or fresh_report.get('all_barriers_examined') is not True or fresh_report.get('activation_authorized') is not False
                or fresh_report.get('external_reconciliation_required') is not True):
            raise ValueError('fresh native cutoff identity differs from reviewed receipt')
    report = C.read_json(path / 'post/reward-review/REVIEW.json')
    if (report.get('initial_reward_cutover') != summary or report.get('owner') != baseline['policy']['owner']
            or report.get('epoch') != summary['epoch'] or report.get('bundle_id') != verified['id']
            or report.get('bundle_manifest_sha256') != a['backup_manifest_sha256']
            or report.get('all_barriers_examined') is not True or report.get('activation_authorized') is not False
            or report.get('external_reconciliation_required') is not True):
        raise ValueError('retained native cutoff reward report changed')
    return {'directory': str(path), 'preparation_sha256': prepared_sha, 'applied_sha256': applied_sha, 'summary': summary}


def plan(candidate, candidate_sha, programs, program_sha, directory,
         cutoff=None, cutoff_prepared_sha=None, cutoff_applied_sha=None):
    if sum(v is not None for v in (cutoff, cutoff_prepared_sha, cutoff_applied_sha)) not in (0, 3):
        raise ValueError('cutoff directory and both reviewed digests must be supplied together')
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    r = candidate_review(candidate, candidate_sha)
    program_review(programs, program_sha, r['baseline'])
    cutoff_binding = (completed_cutover_review(cutoff, cutoff_prepared_sha, cutoff_applied_sha, r['baseline'])
                      if cutoff is not None else None)
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
    C.write_json(directory / 'PREPARED.json', {**({'initial_reward_cutover_review': cutoff_binding} if cutoff_binding else {}),
        'version': 1, 'tool': verified_tool_identity(),
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
    if 'initial_reward_cutover_review' in a:
        bound = a['initial_reward_cutover_review']
        if (not isinstance(bound, dict) or set(bound) != {'directory', 'preparation_sha256', 'applied_sha256', 'summary'}
                or completed_cutover_review(Path(bound['directory']), bound['preparation_sha256'], bound['applied_sha256'], r['baseline']) != bound):
            raise ValueError('reviewed initial reward cutoff binding changed')
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


def first_activation_backlog(report, limits, cutoff=None):
    """A completed native cutoff excludes history, never prior financial authority."""
    names = ('unreceived_payment_events', 'unreceived_feeding_events',
             'unfinalized_core_reservations', 'unresolved_provider_payments', 'payout_held_sats')
    if (report.get('all_barriers_examined') is not True or report.get('activation_authorized') is not False
            or report.get('external_reconciliation_required') is not True
            or type(report.get('stored_member_percent')) is not int
            or report['stored_member_percent'] != limits['member_percent']
            or type(report.get('stored_max_members')) is not int or report['stored_max_members'] != 3
            or any(type(report.get(name)) is not int or report[name] != 0 for name in names)
            or type(report.get('pending_feeding_ingress')) is not int or report['pending_feeding_ingress'] < 0):
        raise ValueError('first activation requires the selected stored allocation and classified financial intake without existing holds')
    allowed = {'unplanned_zero_allocation', 'unplanned_no_strike_funding', 'unplanned_funded_allocation',
               'no_payout', 'awaiting_reservation', 'reserved'}
    forbidden = ('awaiting_reservation', 'reserved')
    if cutoff is None:
        if report.get('initial_reward_cutover') is not None or report['pending_feeding_ingress'] != 0:
            raise ValueError('an initial reward cutoff requires its separately verified completed disposition')
        forbidden += ('unplanned_funded_allocation',)
    else:
        cutoff_summary(cutoff)
        if (report.get('initial_reward_cutover') != cutoff or report.get('epoch') != cutoff['epoch']
                or type(report.get('core_event_head')) is not int or report['core_event_head'] < cutoff['through']
                or type(report.get('pending_reward_eligible_feeding_ingress')) is not int
                or report['pending_reward_eligible_feeding_ingress'] != 0):
            raise ValueError('current native reward report must retain the verified cutoff and classify future financial work')
        allowed.add('excluded_before_activation')
    stages = report.get('stages')
    if (not isinstance(stages, dict) or not set(stages) <= allowed
            or any(type(v) is not int or v < 0 for v in stages.values())
            or any(stages.get(name, 0) for name in forbidden)
            or (cutoff is not None and stages.get('excluded_before_activation', 0) > cutoff['historical_feedings'])):
        raise ValueError('existing funded reward work requires a separate explicit accounting decision')


def apply(directory, expected, checkpoint_sha, review_sha, approved):
    if not approved:
        raise ValueError('explicit continuing exclusive maintenance approval required')
    a, m, r = load(directory, expected)
    with M.exclusive(directory):
        if any((directory / n).exists() for n in ('APPLICATION-STARTED.json', 'RESUME-REQUESTED.json', 'ROLLBACK-REQUESTED.json')):
            raise ValueError('financial application already began; inspect rather than reapply')
        checked, report = checked_checkpoint(directory, checkpoint_sha, review_sha, m, r, False)
        first_activation_backlog(report, r['limits'], a.get('initial_reward_cutover_review', {}).get('summary'))
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
            first_activation_backlog(report, r['limits'], a.get('initial_reward_cutover_review', {}).get('summary'))
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
    planning.add_argument('--initial-reward-cutover', type=Path)
    planning.add_argument('--cutoff-preparation-sha256')
    planning.add_argument('--cutoff-applied-sha256')
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
        result = plan(args.candidate, args.candidate_sha256, args.program_review, args.program_review_sha256, args.directory,
                      args.initial_reward_cutover, args.cutoff_preparation_sha256, args.cutoff_applied_sha256)
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
