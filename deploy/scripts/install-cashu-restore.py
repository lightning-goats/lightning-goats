#!/usr/bin/env python3
"""Install or advance a verified checkpoint with every recovery fence intact."""
import argparse
import base64
from contextlib import ExitStack, closing
import hmac
import json
import os
from pathlib import Path
import re
import runpy
import sqlite3
import stat
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent
REVIEW = runpy.run_path(str(HERE / 'review-cashu-restore.py'))
GIB = 1024**3
SIGNER_CLIENTS = {
    'cashu_signer': ('cashu_client_source', 'cashu-client.hex', 'cashu-nostr-client', 'lg-cashu-nostr-client'),
    'signer': ('payout_client_source', 'payout-client.hex', 'nostr-client-key', 'cyberherd-signer-client'),
}
# CRED_AES256_GCM_BY_HOST in systemd's creds-util.h. Reject null-key
# envelopes even on versions that do not offer systemd-creds --refuse-null.
HOST_CREDENTIAL = bytes.fromhex('5a1c6a86df9d4096b1d5a65e0862f19a')
HOST_KEY = Path('/var/lib/systemd/credential.secret')
HOST_KEY_APP_ID = 'd3acecba0dad4cdfb8c9381528936c58'
CLEAN_ENV = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}


def private_attributes(path):
    if any(name.startswith('system.posix_acl') or name == 'security.capability'
           for name in os.listxattr(path, follow_symlinks=False)):
        raise ValueError('runtime state has unexpected access attributes')


def target_inventory(state, worker, common):
    common['protected_directory'](state, worker['uid'], worker['gid'], private=True)
    private_attributes(state)
    if stat.S_IMODE(state.lstat().st_mode) != 0o700:
        raise ValueError('installed state directory mode differs')
    return {p.name for p in state.iterdir()}


def verify_directory(state, worker, members, common, budget, names=None):
    if names is None:
        names = target_inventory(state, worker, common)
    if names != set(members):
        raise ValueError('existing target differs from the exact fenced installation')
    for name, member in members.items():
        path = state / name
        private_attributes(path)
        with closing(common['Frozen'](path, worker['uid'], GIB, budget, gid=worker['gid'],
                                      empty=name == 'wallet-writer.lock')) as installed:
            if (installed.digest, installed.expected[6], stat.S_IMODE(installed.expected[4])) != (
                    member['sha256'], member['bytes'], 0o600):
                raise ValueError('existing target differs from the exact fenced installation')


def copy_role(source, destination, members, worker, common, budget):
    destination.mkdir(mode=0o700)
    destination.chmod(0o700)
    private_attributes(destination)
    for name, expected in members.items():
        with closing(common['Frozen'](source / name, 0, GIB, budget, empty=name == 'wallet-writer.lock')) as frozen:
            if frozen.copy(destination / name) != expected:
                raise ValueError('staged image changed while preparing installation')
        path = destination / name
        private_attributes(path)
        with path.open('rb') as file:
            os.fchown(file.fileno(), worker['uid'], worker['gid'])
            os.fsync(file.fileno())
    os.chown(destination, worker['uid'], worker['gid'])
    common['sync'](destination)
    common['sync'](destination.parent)


def systemd_credential(operation, name, data, budget, *, allow_refusal=False):
    args = ['/usr/bin/systemd-creds', '--no-pager', '--newline=no', '--name=' + name]
    if operation == 'encrypt':
        args.append('--with-key=host')
    result = subprocess.run([*args, operation, '-', '-'], input=data, capture_output=True,
                            timeout=budget.remaining(15), env=CLEAN_ENV)
    if result.returncode and allow_refusal and not result.stdout:
        return None
    if result.returncode or not 0 < len(result.stdout) <= 4096:
        raise ValueError('systemd signer credential operation refused')
    return result.stdout


def ensure_existing_host_key(key, budget):
    # systemd may generate a replacement when the stored machine identity
    # differs. Check the identity in the current host-key format first.
    result = subprocess.run(['/usr/bin/systemd-id128', '--app-specific=' + HOST_KEY_APP_ID, 'machine-id'],
                            stdin=subprocess.DEVNULL, capture_output=True,
                            timeout=budget.remaining(5), env=CLEAN_ENV)
    key.file.seek(0)
    if (result.returncode or not re.fullmatch(rb'[0-9a-f]{32}\n', result.stdout)
            or key.expected[3] != 0 or stat.S_IMODE(key.expected[4]) != 0o400
            or key.file.read(16) != bytes.fromhex(result.stdout.decode())):
        raise ValueError('current systemd credential host key required')
    key.check()


def check_credential_store(store, common):
    common['protected_directory'](store, gid=0, private=True)
    for path in (store, *store.parents):
        private_attributes(path)


def prepare_signer_clients(store, stage, originals, policy, common, budget, stack):
    check_credential_store(store, common)
    if store.is_relative_to(stage):
        raise ValueError('signer credential store overlaps the recovery evidence')
    directory = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix='.lg-signer-restore-', dir=store)))
    prepared = {}
    for role, (field, source_name, name, filename) in SIGNER_CLIENTS.items():
        archive_name = 'recovery-material/' + source_name
        entry = originals[archive_name]
        path = stage / 'checkpoint-evidence' / archive_name
        private_attributes(path)
        with closing(common['Frozen'](path, 0, 65, budget)) as key:
            if (entry['source'] != policy[field] or entry['source_uid'] != 0
                    or (key.digest, key.expected[6]) != (entry['sha256'], entry['bytes'])):
                raise ValueError('portable signer client differs from the verified collection')
            key.file.seek(0)
            plaintext = key.file.read(65)
            key.check()
        if not re.fullmatch(rb'[0-9a-fA-F]{64}\n?', plaintext):
            raise ValueError('invalid archived signer client')
        target = store / filename
        candidate = None
        if os.path.lexists(target):
            private_attributes(target)
            with closing(common['Frozen'](target, 0, 4096, budget, gid=0)) as existing:
                existing.file.seek(0)
                encrypted = existing.file.read(4096)
                existing.check()
        else:
            encrypted = systemd_credential('encrypt', name, plaintext, budget)
            candidate = directory / filename
        decoded = base64.b64decode(b''.join(encrypted.split()), validate=True)
        if not decoded.startswith(HOST_CREDENTIAL) or not hmac.compare_digest(
                systemd_credential('decrypt', name, encrypted, budget), plaintext):
            raise ValueError('installed signer credential differs from the original client or host encryption')
        # systemd accepts unnamed envelopes even with --name. A different
        # name must fail for the candidate to be bound to this signer unit.
        if systemd_credential('decrypt', 'lg-cashu-restore-name-probe', encrypted,
                              budget, allow_refusal=True) is not None:
            raise ValueError('signer credential is missing its unit credential name')
        if candidate is not None:
            common['write'](candidate, encrypted)
        prepared[role] = (target, candidate, common['sha'](encrypted))
    return prepared


def publish_no_replace(source, target, budget):
    # Source and destination share a protected filesystem parent. GNU mv
    # supplies no-clobber publication even when the other entry is empty.
    result = subprocess.run(['/usr/bin/mv', '-nT', '--', str(source), str(target)],
                            stdin=subprocess.DEVNULL, capture_output=True,
                            timeout=budget.remaining(15), env=CLEAN_ENV)
    if result.returncode or os.path.lexists(source):
        raise ValueError('fenced destination appeared during publication')


def install_stage(stage, stage_sha, policy_sha, common, budget):
    read = common['COMBINED']['read_private']
    receipt, manifest = REVIEW['stage_metadata'](stage, stage_sha, common)
    collection_raw = read(stage / 'checkpoint-evidence' / 'collection.json', 4 * 1024**2)
    collection = common['document'](collection_raw)
    if (receipt.get('collection_sha256') != common['sha'](collection_raw)
            or collection['checkpoint_id'] != receipt['checkpoint_id']
            or collection['policy_sha256'] != policy_sha
            or collection['quiescence_verified'] is not True):
        raise ValueError('restore policy or collection evidence differs from the selected stage')
    checkpoint = receipt['checkpoint_id']
    original = {row['path']: row for row in collection['originals']}
    if len(original) != len(collection['originals']):
        raise ValueError('duplicate collection record')
    REVIEW['open_images'](stage, stage_sha, common, budget)
    members = {}
    for role, rows in manifest['roles'].items():
        marker = (checkpoint + '\n').encode()
        members[role] = {REVIEW['recovery_marker'](role): {'sha256': common['sha'](marker), 'bytes': len(marker)}}
        members[role].update({row['name']: {'sha256': row['sha256'], 'bytes': row['bytes']} for row in rows})
    return receipt, manifest, collection, original, members


def require_reconciled_reference(report):
    if (report['version'] != 3 or any(report['reference_missing_counts'].values())
            or any(report['reference_pending_counts'].values())):
        raise ValueError('advancement requires retained and settled original operations')
    custody = report['reference_custody_all']
    if any(custody[key] for key in ('stale_local_unspent_sats', 'mint_unresolved_sats', 'mint_unspent_held_sats')):
        raise ValueError('reference has stale or held mint custody')
    for row in report['treasury']:
        accounting = row['reference_accounting']
        if (not row['observed_pair_matches'] or not accounting['wallet_cdk_cleanup_retained']
                or accounting['completion_totals'] is None):
            raise ValueError('reference does not retain the completed original treasury operation')
    terminal = {'confirmed': 'confirmed', 'not_dispatched': 'reconciled_not_fed'}
    for row in report['feeder']:
        if (row['checkpoint_conflict'] or row['gateway_status'] not in terminal
                or row['reference_status'] != terminal[row['gateway_status']]):
            raise ValueError('reference does not retain the original terminal feeder outcome')


def parking_record(state, role, old, later, previous_sha, stage_sha, policy_sha, evidence_sha):
    directory = state.parent / f".lg-cashu-advance-{old['checkpoint_id']}-{later['checkpoint_id']}-{role}"
    intent = {'version': 1, 'role': role, 'state_directory': str(state), 'previous_stage_sha256': previous_sha,
              'later_stage_sha256': stage_sha, 'policy_sha256': policy_sha, 'evidence_sha256': evidence_sha}
    return directory, intent


def parked_state(directory, intent, common, budget):
    if not os.path.lexists(directory):
        return False
    common['protected_directory'](directory, gid=0, private=True)
    private_attributes(directory)
    names = {p.name for p in directory.iterdir()}
    if names not in ({'INTENT.json'}, {'INTENT.json', 'fenced-state'}):
        raise ValueError('retained prior checkpoint has an unexpected inventory')
    path = directory / 'INTENT.json'
    private_attributes(path)
    with closing(common['Frozen'](path, 0, 65536, budget, gid=0)) as source:
        if source.digest != common['sha'](common['json_bytes'](intent)):
            raise ValueError('fenced advancement differs from the retained operation')
    return 'fenced-state' in names


def prepare_parking(directory, intent, common, budget):
    if os.path.lexists(directory):
        if parked_state(directory, intent, common, budget):
            raise ValueError('older checkpoint is already retained')
        return
    with tempfile.TemporaryDirectory(prefix='.lg-cashu-intent-', dir=directory.parent) as temporary:
        prepared = Path(temporary) / 'retention'
        prepared.mkdir(mode=0o700)
        os.chown(prepared, 0, 0)
        prepared.chmod(0o700)
        common['write_json'](prepared / 'INTENT.json', intent)
        common['sync'](prepared)
        publish_no_replace(prepared, directory, budget)
    common['sync'](directory.parent)


def install(args, common, tool_identity, *, after_publish=lambda _: None, after_archive=lambda _: None):
    if os.geteuid() != 0:
        raise ValueError('root is required for restored runtime ownership')
    if any(not re.fullmatch('[0-9a-f]{64}', digest) for digest in (args.stage_sha256, args.policy_sha256)):
        raise ValueError('independently reviewed stage and policy digests required')
    policy, _ = common['load_policy'](args.policy, args.policy_sha256, common['Budget'](65536, 30),
                                      allow_missing_state=True)
    budget = common['Budget'](policy['total_bytes'], policy['deadline_seconds'])
    workers = policy['workers']
    states = {role: Path(worker['state']) for role, worker in workers.items()}
    stage = args.stage
    credential_store = args.signer_credential_store
    advance_from, advance_evidence = getattr(args, 'advance_from', None), getattr(args, 'advance_evidence', None)
    if bool(advance_from) != bool(advance_evidence):
        raise ValueError('advancement requires an exact installed stage and paired evidence')
    receipt, manifest, collection, original, members = install_stage(
        stage, args.stage_sha256, args.policy_sha256, common, budget)
    checkpoint = receipt['checkpoint_id']

    with ExitStack() as stack:
        lock_unchanged = stack.enter_context(common['maintenance_lock'](common['GLOBAL_LOCK']))
        tracked = []

        def freeze(path, maximum, secret=False):
            lease = stack.enter_context(closing(common['Frozen'](Path(path), 0, maximum, budget, secret=secret)))
            tracked.append(lease)
            return lease

        if freeze(args.policy, 65536, secret=True).digest != args.policy_sha256:
            raise ValueError('installed policy changed after selection')

        def verify_original(path, archive_name, maximum):
            entry = original[archive_name]
            file = freeze(path, maximum)
            if (entry['source'] != str(file.path) or entry['sha256'] != file.digest
                    or (entry['source_uid'], entry['source_gid'], entry['source_mode']) != (
                        file.expected[2], file.expected[3], stat.S_IMODE(file.expected[4]))):
                raise ValueError('installed config or executable differs from the selected collection')
            return file

        for role, field in common['CONFIG_FIELDS'].items():
            verify_original(policy[field], f'{role}-config', 65536)
        for role, artifact in policy['runtime_artifacts'].items():
            binary = verify_original(artifact['path'], f'programs/runtime-{role}', 512 * 1024**2)
            private_attributes(binary.path)
            mode = stat.S_IMODE(binary.expected[4])
            if binary.digest != artifact['sha256'] or mode & 0o7000 or not mode & 0o100:
                raise ValueError('installed runtime binary differs from the selected collection')

        def guard():
            lock_unchanged()
            common['check_quiescence'](workers, budget)

        guard()
        parking, archive = {}, set()
        if advance_from:
            old_stage, old_sha = Path(advance_from[0]), advance_from[1]
            evidence, evidence_sha = Path(advance_evidence[0]), advance_evidence[1]
            old, _, _, _, old_members = install_stage(old_stage, old_sha, args.policy_sha256, common, budget)
            reconcile = runpy.run_path(str(HERE / 'reconcile-cashu-restore.py'))['compare_checkpoints']
            comparison = reconcile(old_stage, old_sha, evidence, evidence_sha, stage, args.stage_sha256, common, budget)
            require_reconciled_reference(comparison)
            for role, state in states.items():
                parking[role] = parking_record(state, role, old, receipt, old_sha, args.stage_sha256,
                                                args.policy_sha256, evidence_sha)
        credentials = {}
        if credential_store is not None:
            if receipt['keys_verified'] is not True or collection['key_verification']['signer_clients_verified'] != 2:
                raise ValueError('verified stage must bind both original signer clients')
            private_attributes(HOST_KEY)
            host_key = freeze(HOST_KEY, 65536, secret=True)
            ensure_existing_host_key(host_key, budget)
            credentials = prepare_signer_clients(credential_store, stage, original, policy, common, budget, stack)
        prepared = {}
        empty = {}
        complete = []
        for role, state in states.items():
            common['protected_directory'](state.parent)
            if advance_from:
                park, intent = parking[role]
                if parked_state(park, intent, common, budget):
                    verify_directory(park / 'fenced-state', workers[role], old_members[role], common, budget)
                    if os.path.lexists(state):
                        verify_directory(state, workers[role], members[role], common, budget)
                        complete.append(role)
                        continue
                else:
                    verify_directory(state, workers[role], old_members[role], common, budget)
                    if state.stat().st_dev != state.parent.stat().st_dev or os.path.ismount(state):
                        raise ValueError('runtime directory cannot be retained on the same filesystem')
                    archive.add(role)
            elif os.path.lexists(state):
                names = target_inventory(state, workers[role], common)
                if names:
                    verify_directory(state, workers[role], members[role], common, budget, names=names)
                    complete.append(role)
                    continue
                empty[role] = common['identity'](state.lstat())
            parent = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix='.lg-cashu-restore-', dir=state.parent)))
            staged = parent / 'state'
            copy_role(stage / 'fenced-images' / role, staged, members[role], workers[role], common, budget)
            prepared[role] = staged

        for item in tracked:
            item.check()
        if REVIEW['stage_metadata'](stage, args.stage_sha256, common) != (receipt, manifest):
            raise ValueError('staged restore metadata changed')
        for role, staged in prepared.items():
            state = states[role]
            guard()
            common['protected_directory'](state.parent)
            if role in empty:
                if common['identity'](state.lstat()) != empty[role]:
                    raise ValueError('empty state directory changed before publication')
                state.rmdir()
                common['sync'](state.parent)
            if role in archive:
                park, intent = parking[role]
                prepare_parking(park, intent, common, budget)
                verify_directory(state, workers[role], old_members[role], common, budget)
                if park.stat().st_dev != state.stat().st_dev:
                    raise ValueError('fenced retention crossed a filesystem boundary')
                publish_no_replace(state, park / 'fenced-state', budget)
                common['sync'](park)
                common['sync'](state.parent)
                after_archive(role)
            publish_no_replace(staged, state, budget)
            common['sync'](state.parent)
            after_publish(role)

        guard()
        for role, state in states.items():
            verify_directory(state, workers[role], members[role], common, budget)
            if advance_from:
                park, intent = parking[role]
                if not parked_state(park, intent, common, budget):
                    raise ValueError('retained prior checkpoint is missing')
                verify_directory(park / 'fenced-state', workers[role], old_members[role], common, budget)
        for role, (target, candidate, _) in credentials.items():
            if candidate is not None:
                guard()
                check_credential_store(credential_store, common)
                publish_no_replace(candidate, target, budget)
                common['sync'](credential_store)
                after_publish('signer-credential:' + role)
        if credentials:
            guard()
            check_credential_store(credential_store, common)
        for target, _, checksum in credentials.values():
            private_attributes(target)
            with closing(common['Frozen'](target, 0, 4096, budget, gid=0)) as installed:
                if installed.digest != checksum:
                    raise ValueError('installed signer credential changed during publication')
        for item in tracked:
            item.check()
        result = {'result': 'CASHU_RESTORE_INSTALLED_FENCED', 'version': 1,
                'checkpoint_id': checkpoint, 'stage_sha256': args.stage_sha256,
                'policy_sha256': args.policy_sha256, 'tool': tool_identity,
                'roles': sorted(states), 'already_installed': sorted(complete),
                'signer_clients_verified': sorted(credentials),
                'external_reconciliation_verified': False, 'resume_authorized': False,
                'services_changed': False}
        if advance_from:
            result.update(result='CASHU_RESTORE_ADVANCED_FENCED', previous_stage_sha256=old_sha,
                          advance_evidence_sha256=evidence_sha,
                          retained_paths={role: str(park / 'fenced-state') for role, (park, _) in parking.items()})
        return result


def main():
    if 'verified_tool_identity' not in globals() or os.geteuid() != 0:
        raise SystemExit('Use the externally verified root-owned backup launcher.')
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('stage', 'policy'):
        parser.add_argument(name, type=Path)
        parser.add_argument(name + '_sha256')
    parser.add_argument('--signer-credential-store', type=Path,
                        help='reprovision both archived signer clients in this existing root-private systemd store')
    parser.add_argument('--advance-from', nargs=2, metavar=('STAGED_DIRECTORY', 'RECEIPT_SHA256'))
    parser.add_argument('--advance-evidence', nargs=2, metavar=('MANIFEST', 'SHA256'))
    args = parser.parse_args()
    try:
        common = runpy.run_path(str(HERE / 'collect-cashu-backup.py'))
        result = install(args, common, verified_tool_identity())
    except (OSError, ValueError, TypeError, KeyError, AttributeError, IndexError, sqlite3.Error, subprocess.TimeoutExpired):
        raise SystemExit('Cashu fenced installation refused. Preserve the checkpoint and any installed fences.')
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
