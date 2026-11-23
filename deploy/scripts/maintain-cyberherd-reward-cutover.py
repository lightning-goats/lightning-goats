#!/usr/bin/env python3
"""Reviewed future-only reward disposition; no financial activation or DB restore.

Authenticated root launcher only. Consumes an existing stopped financial checkpoint.
Preparation rehearses on copies. Apply changes only membership audit rows through
the native member-UID child and proves an exact four-journal comparison. Resume
restores only original core/member services; outgoing roles remain masked.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')
    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import tempfile
import uuid

SPEC = importlib.util.spec_from_file_location('cutoff_financial_support', Path(__file__).with_name('activate-cyberherd-financial.py'))
H = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(H)
H.verified_tool_identity = lambda: verified_tool_identity()
U, C, Q, S, TX, M = H.U, H.C, H.Q, H.S, H.TX, H.M


def native(directory, operation, **arguments):
    program = directory / 'payload/cyberherd-statectl'
    C.protected(program)
    args = [str(program), '--config', str(S.MAINTENANCE), operation]
    for key, value in arguments.items():
        args.extend(['--' + key.replace('_', '-'), str(value)])
    result = json.loads(C.command(*args, timeout=300))
    expected = {'preview-initial-reward-cutover': 'preview_initial_reward_cutover',
                'compare-initial-reward-cutover': 'initial_reward_cutover_compare',
                'review-rewards': 'review_rewards'}[operation]
    fields = ({'cutover_applied', 'activation_authorized'} if operation.startswith('preview-')
              else {'activation_authorized'} if operation == 'review-rewards' else {'resume_authorized', 'decision_sha256'})
    if (set(result) != {'operation', 'id', 'accepted'} | fields or result.get('operation') != expected
            or result.get('accepted') is not True or any(result.get(k) is not False for k in fields - {'decision_sha256'})
            or ('decision_sha256' in fields and (not isinstance(result.get('decision_sha256'), str)
                or re.fullmatch('[0-9a-f]{64}', result['decision_sha256']) is None))
            or str(uuid.UUID(result['id'])) != result['id']):
        raise ValueError('native reward disposition operation refused')
    return result


def preview_value(path, expected):
    C.protected(path, private=True)
    if (re.fullmatch('[0-9a-f]{64}', expected) is None or path.stat().st_size > 4 * 1024 * 1024 + 65536
            or U.digest(path) != expected):
        raise ValueError('independently pinned bounded cutoff preview required')
    return json.loads(path.read_text())


def decision_hash(decision):
    """Same struct field order/UTF-8 integer encoding as the native Decision DTO."""
    fields = ('id', 'epoch', 'through', 'backup_manifest_sha256', 'decided_at', 'historical')
    item_fields = ('request_id', 'core_seq', 'proof_hash')
    if set(decision) != set(fields) or any(set(item) != set(item_fields) for item in decision['historical']):
        raise ValueError('unknown native cutoff decision fields')
    value = {key: decision[key] for key in fields}
    value['historical'] = [{key: item[key] for key in item_fields} for item in decision['historical']]
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()).hexdigest()


def receipt(value):
    decision = value['decision']
    return {'decision_id': decision['id'], 'epoch': decision['epoch'], 'through': decision['through'],
            'historical_feedings': len(decision['historical']), 'activation_authorized': False}


def copy_images(source, destination):
    destination.mkdir(mode=0o700)
    for role in U.P.ROLE_NAMES:
        for suffix in ('', '-wal'):
            incoming = source / (role + '.db' + suffix)
            if suffix and not incoming.exists():
                continue
            C.protected(incoming, private=True)
            TX.copy_durable(incoming, destination / incoming.name, TX.fingerprint(incoming))
    TX.sync_directory(destination)


def child(r, preview, pin, config, state=None, source=None):
    """No inherited credentials/capabilities/network; executable stays disk-backed."""
    identity = r['baseline']['identities']['membership']
    program = Q.BIN / 'cyberherdd'
    expected = r['baseline']['files'][str(program)]
    C.protected(program)
    if TX.fingerprint(program) != expected:
        raise ValueError('reviewed member program changed')
    C.protected(Path('/run'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-reward-cutoff-', dir='/run'))
    carrier.chmod(0o711)
    try:
        selected = carrier / 'member.toml'
        raw = config.read_bytes()
        if state is not None:
            # Change only the clone state path, using TOML's JSON-compatible string.
            parsed = H.tomllib.loads(raw.decode())
            old = parsed['state_dir']
            text, count = re.subn(r'^state_dir\s*=\s*"[^"\n]+"\s*$',
                                 'state_dir = ' + json.dumps(str(state)), raw.decode(), flags=re.M)
            if count != 1 or dict(H.tomllib.loads(text), state_dir=old) != parsed:
                raise ValueError('clone must change only the member state directory')
            raw = text.encode()
            state.mkdir(mode=0o700)
            os.chown(state, identity['uid'], identity['gid'])
            for suffix in ('', '-wal'):
                incoming = source / ('membership.db' + suffix)
                if suffix and not incoming.exists():
                    continue
                fp = TX.fingerprint(incoming)
                TX.copy_durable(incoming, state / ('cyberherd.db' + suffix),
                                dict(fp, uid=identity['uid'], gid=identity['gid'], mode=0o600))
        Q.I.publish(selected, raw, 0o440)
        os.chown(selected, 0, identity['gid'])
        approved = carrier / 'PREVIEW.json'
        Q.I.publish(approved, preview.read_bytes(), 0o440)
        os.chown(approved, 0, identity['gid'])
        result = json.loads(C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
            '--reuid=' + str(identity['uid']), '--regid=' + str(identity['gid']), '--clear-groups',
            '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
            str(program), '--config', str(selected), '--record-initial-reward-cutover', str(approved),
            '--reward-cutover-sha256', pin, '--writers-stopped', timeout=300))
        if (TX.fingerprint(program) != expected or selected.read_bytes() != raw
                or U.digest(approved) != pin):
            raise ValueError('member child inputs changed')
        return result
    finally:
        carrier.chmod(0o700)
        TX.sync_directory(carrier)


def context(directory, expected, checkpoint_sha, report_sha):
    a, m, r = H.load(directory, expected)
    for name in ('APPLICATION-STARTED.json', 'ROLLBACK-REQUESTED.json', 'RESUME-REQUESTED.json'):
        if (directory / name).exists():
            raise ValueError('financial configuration or resumption already began')
    checked, _ = H.checked_checkpoint(directory, checkpoint_sha, report_sha, m, r, False)
    return a, m, r, checked


def prepare(directory, expected, checkpoint_sha, report_sha, decision_id):
    if str(uuid.UUID(decision_id)) != decision_id or uuid.UUID(decision_id).int == 0:
        raise ValueError('canonical nonnil cutoff ID required')
    H.load(directory, expected)
    with M.exclusive(directory):
        _, m, r, checked = context(directory, expected, checkpoint_sha, report_sha)
        root = directory / 'reward-cutover'
        root.mkdir(mode=0o700)
        M.note(root, 'INCOMPLETE', {'activation_authorized': False})
        preview = root / 'preview'
        ack = native(directory, 'preview-initial-reward-cutover', bundle=directory / 'backup',
                     destination=preview, decision_id=decision_id)
        pin = U.digest(preview / 'PREVIEW.json')
        value = preview_value(preview / 'PREVIEW.json', pin)
        if (ack['id'] != decision_id or value['id'] != decision_id
                or value['bundle_id'] != checked['native_backup']['id']
                or value['owner'] != r['baseline']['policy']['owner']
                or value['decision']['epoch'] != r['baseline']['policy']['epoch']
                or value['decision']['backup_manifest_sha256'] != checked['backup_manifest_sha256']):
            raise ValueError('native cutoff preview scope changed')
        # Carrier is root-traversable so the member can reach its private clone.
        carrier = Path(tempfile.mkdtemp(prefix='lg-cutoff-clone-', dir='/run'))
        carrier.chmod(0o711)
        try:
            config = H.configurations()['membership']
            result = child(r, preview / 'PREVIEW.json', pin, config,
                           state=carrier / 'state', source=directory / 'backup')
            if result != receipt(value):
                raise ValueError('member clone did not record the exact cutoff')
            rehearsed = root / 'rehearsed'
            copy_images(directory / 'backup', rehearsed)
            for suffix in ('', '-wal'):
                target = rehearsed / ('membership.db' + suffix)
                incoming = carrier / 'state' / ('cyberherd.db' + suffix)
                if target.exists():
                    target.unlink()
                if incoming.exists():
                    fp = TX.fingerprint(incoming)
                    TX.copy_durable(incoming, target, dict(fp, uid=0, gid=0, mode=0o600))
            TX.sync_directory(rehearsed)
        finally:
            carrier.chmod(0o700)
            TX.sync_directory(carrier)
        compared = native(directory, 'compare-initial-reward-cutover', bundle=directory / 'backup',
                          preview=preview, preview_sha256=pin, candidate_copy=rehearsed)
        if compared['decision_sha256'] != decision_hash(value['decision']):
            raise ValueError('native rehearsal did not pin the entire reviewed decision')
        U.sources_match(m, r, checked['live_images'])
        M.note(root, 'PREPARED.json', {'version': 1, 'tool': verified_tool_identity(),
            'financial_review_sha256': expected, 'checkpoint_sha256': checkpoint_sha,
            'reward_review_sha256': report_sha, 'preview_sha256': pin,
            'expected_receipt': result, 'rehearsal': compared, 'activation_authorized': False})
        (root / 'INCOMPLETE').unlink()
        TX.sync_directory(root)
    return {'prepared': True, 'rehearsed': True, 'live_journals_changed': False, 'activation_authorized': False}


def prepared(directory, expected, checkpoint_sha, report_sha, cutoff_sha):
    root = directory / 'reward-cutover'
    C.protected(root, directory=True, private=True)
    if (re.fullmatch('[0-9a-f]{64}', cutoff_sha) is None
            or U.digest(root / 'PREPARED.json') != cutoff_sha or (root / 'INCOMPLETE').exists()):
        raise ValueError('independently reviewed complete cutoff preparation required')
    value = C.read_json(root / 'PREPARED.json')
    if (value.get('version') != 1 or value.get('tool') != verified_tool_identity()
            or value.get('financial_review_sha256') != expected
            or value.get('checkpoint_sha256') != checkpoint_sha or value.get('reward_review_sha256') != report_sha
            or value.get('activation_authorized') is not False):
        raise ValueError('matching independently reviewed cutoff preparation required')
    if (U.digest(directory / 'CHECKPOINT.json') != checkpoint_sha
            or U.digest(directory / 'reward-review/REVIEW.json') != report_sha):
        raise ValueError('original financial checkpoint or reward report changed')
    preview = root / 'preview'
    data = preview_value(preview / 'PREVIEW.json', value['preview_sha256'])
    if value['expected_receipt'] != receipt(data):
        raise ValueError('cutoff preparation receipt changed')
    compared = native(directory, 'compare-initial-reward-cutover', bundle=directory / 'backup',
                      preview=preview, preview_sha256=value['preview_sha256'], candidate_copy=root / 'rehearsed')
    if compared != value['rehearsal'] or compared['decision_sha256'] != decision_hash(data['decision']):
        raise ValueError('retained native cutoff rehearsal changed')
    return root, value, data


def apply(directory, expected, checkpoint_sha, report_sha, cutoff_sha, approved):
    if not approved:
        raise ValueError('explicit initial reward cutoff maintenance approval required')
    H.load(directory, expected)
    with M.exclusive(directory):
        _, m, r, checked = context(directory, expected, checkpoint_sha, report_sha)
        root, selected, data = prepared(directory, expected, checkpoint_sha, report_sha, cutoff_sha)
        if (root / 'APPLICATION-STARTED.json').exists():
            raise ValueError('cutoff application already began; inspect without automatic retry')
        U.sources_match(m, r, checked['live_images'])
        M.note(root, 'APPLICATION-STARTED.json', {'cutoff_review_sha256': cutoff_sha,
            'activation_authorized': False, 'database_restore_authorized': False})
        result = child(r, root / 'preview/PREVIEW.json', selected['preview_sha256'], H.configurations()['membership'])
        if result != selected['expected_receipt']:
            raise ValueError('live member cutoff receipt differs; retain stopped state')
        U.stopped(m, r)
        observed = U.state_files(r)
        post = root / 'post'
        post.mkdir(mode=0o700)
        U.native(m, 'snapshot', '--destination', post / 'backup', '--writers-stopped')
        accepted = U.native(m, 'verify', '--bundle', post / 'backup')
        compared = native(directory, 'compare-initial-reward-cutover', bundle=directory / 'backup',
            preview=root / 'preview', preview_sha256=selected['preview_sha256'], candidate_copy=post / 'backup')
        review_ack = native(directory, 'review-rewards', bundle=post / 'backup', destination=post / 'reward-review')
        report = C.read_json(post / 'reward-review/REVIEW.json')
        summary = dict(decision_id=data['decision']['id'], epoch=data['decision']['epoch'],
            through=data['decision']['through'], backup_manifest_sha256=data['decision']['backup_manifest_sha256'],
            historical_feedings=len(data['decision']['historical']), decision_sha256=decision_hash(data['decision']))
        manifest = C.read_json(post / 'backup/MANIFEST.json')
        if (report.get('initial_reward_cutover') != summary or report.get('id') != review_ack['id']
                or report.get('bundle_id') != accepted['id'] or manifest.get('id') != accepted['id']
                or report.get('bundle_manifest_sha256') != U.digest(post / 'backup/MANIFEST.json')
                or report.get('owner') != data['owner'] or report.get('epoch') != data['decision']['epoch']
                or report.get('all_barriers_examined') is not True or report.get('activation_authorized') is not False
                or report.get('external_reconciliation_required') is not True):
            raise ValueError('post-cutoff native reward report differs')
        U.sources_match(m, r, observed)
        M.note(root, 'APPLIED.json', {'version': 1, 'cutoff_review_sha256': cutoff_sha,
            'receipt': result, 'initial_reward_cutover': summary, 'native_backup': accepted, 'comparison': compared,
            'backup_manifest_sha256': U.digest(post / 'backup/MANIFEST.json'), 'reward_review': review_ack,
            'reward_report_sha256': U.digest(post / 'reward-review/REVIEW.json'), 'live_images': observed,
            'retained_rows_unchanged': True, 'configurations_changed': False, 'services_started': False,
            'activation_authorized': False})
    return {'cutoff_applied': True, 'retained_rows_unchanged': True, 'services_started': False, 'activation_authorized': False}


def resume(directory, expected, checkpoint_sha, report_sha, cutoff_sha, applied_sha, approved):
    if not approved:
        raise ValueError('explicit original payment/membership resumption approval required')
    a, m, r = H.load(directory, expected)
    with M.exclusive(directory):
        root, selected, _ = prepared(directory, expected, checkpoint_sha, report_sha, cutoff_sha)
        path = root / 'APPLIED.json'
        C.protected(path, private=True)
        if re.fullmatch('[0-9a-f]{64}', applied_sha) is None or U.digest(path) != applied_sha:
            raise ValueError('independently reviewed completed cutoff receipt required')
        value = C.read_json(path)
        if (value.get('version') != 1 or value.get('cutoff_review_sha256') != cutoff_sha
                or value.get('receipt') != selected['expected_receipt'] or value.get('retained_rows_unchanged') is not True
                or any(value.get(k) is not False for k in ('configurations_changed', 'services_started', 'activation_authorized'))
                or any((directory / n).exists() for n in ('APPLICATION-STARTED.json', 'ROLLBACK-REQUESTED.json', 'RESUME-REQUESTED.json'))
                or (root / 'RESUME-REQUESTED.json').exists()):
            raise ValueError('original-only cutoff resumption evidence differs or already began')
        post = root / 'post'
        if (U.native(m, 'verify', '--bundle', post / 'backup') != value['native_backup']
                or U.digest(post / 'backup/MANIFEST.json') != value['backup_manifest_sha256']
                or U.digest(post / 'reward-review/REVIEW.json') != value['reward_report_sha256']
                or native(directory, 'compare-initial-reward-cutover', bundle=directory / 'backup',
                    preview=root / 'preview', preview_sha256=selected['preview_sha256'], candidate_copy=post / 'backup') != value['comparison']):
            raise ValueError('accepted post-cutoff checkpoint changed')
        U.sources_match(m, r, value['live_images'])
        M.note(root, 'RESUME-REQUESTED.json', {'applied_sha256': applied_sha, 'outgoing_roles_enabled': False})
        H.restore_units(directory, a, r, original=True)
        for role, program, health in [('core', 'lightning-goatsd', 'http://127.0.0.1:8787/healthz'),
                                     ('membership', 'cyberherdd', 'http://127.0.0.1:3340/api/v1/cyberherd')]:
            name = U.P.ROLE_NAMES[role]
            C.command('/usr/bin/systemctl', 'start', '--', name + '.service', timeout=120)
            M.running(name + '.service', 'lightning-goats' if role == 'core' else name, Q.BIN / program)
            M.health(health)
        U.stable(m, r)
        for role in ('payout', 'signer'):
            status = C.status(U.P.ROLE_NAMES[role] + '.service')
            if (status['LoadState'] != 'masked' or status['UnitFileState'] != 'masked' or status['MainPID'] != '0'):
                raise ValueError('outgoing role changed during original resumption')
        M.note(root, 'RESUMED.json', {'core_running': True, 'membership_running': True,
            'configurations_changed': False, 'outgoing_roles_enabled': False, 'activation_authorized': False})
    return {'core_running': True, 'membership_running': True, 'outgoing_roles_enabled': False, 'activation_authorized': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    for name in ('prepare', 'apply', 'resume-original'):
        command = commands.add_parser(name)
        command.add_argument('directory', type=Path)
        for field in ('review', 'checkpoint', 'reward-review'):
            command.add_argument('--' + field + '-sha256', required=True)
        if name == 'prepare':
            command.add_argument('--decision-id', required=True)
        else:
            command.add_argument('--cutoff-review-sha256', required=True)
            if name == 'apply':
                command.add_argument('--approve-initial-reward-cutover', action='store_true', required=True)
            else:
                command.add_argument('--applied-sha256', required=True)
                command.add_argument('--approve-original-resumption', action='store_true', required=True)
    args = parser.parse_args()
    common = (args.directory, args.review_sha256, args.checkpoint_sha256, args.reward_review_sha256)
    if args.operation == 'prepare':
        result = prepare(*common, args.decision_id)
    elif args.operation == 'apply':
        result = apply(*common, args.cutoff_review_sha256, args.approve_initial_reward_cutover)
    else:
        result = resume(*common, args.cutoff_review_sha256, args.applied_sha256, args.approve_original_resumption)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
