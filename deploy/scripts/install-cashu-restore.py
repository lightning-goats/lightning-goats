#!/usr/bin/env python3
"""Install a verified checkpoint into empty service paths with every fence intact."""
import argparse
from contextlib import ExitStack, closing
import json
import os
from pathlib import Path
import re
import runpy
import stat
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent
REVIEW = runpy.run_path(str(HERE / 'review-cashu-restore.py'))
GIB = 1024**3


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
        with closing(common['Frozen'](path, worker['uid'], GIB, budget, gid=worker['gid'])) as installed:
            if (installed.digest, installed.expected[6], stat.S_IMODE(installed.expected[4])) != (
                    member['sha256'], member['bytes'], 0o600):
                raise ValueError('existing target differs from the exact fenced installation')


def copy_role(source, destination, members, worker, common, budget):
    destination.mkdir(mode=0o700)
    destination.chmod(0o700)
    private_attributes(destination)
    for name, expected in members.items():
        with closing(common['Frozen'](source / name, 0, GIB, budget)) as frozen:
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


def install(args, common, tool_identity, *, after_publish=lambda _: None):
    if os.geteuid() != 0:
        raise ValueError('root is required for restored runtime ownership')
    if any(not re.fullmatch('[0-9a-f]{64}', digest) for digest in (args.stage_sha256, args.policy_sha256)):
        raise ValueError('independently reviewed stage and policy digests required')
    policy, _ = common['load_policy'](args.policy, args.policy_sha256, common['Budget'](65536, 30),
                                      allow_missing_state=True)
    budget = common['Budget'](policy['total_bytes'], policy['deadline_seconds'])
    workers = policy['workers']
    states = {role: Path(worker['state']) for role, worker in workers.items()}
    read = common['COMBINED']['read_private']
    stage = args.stage

    receipt, manifest = REVIEW['stage_metadata'](stage, args.stage_sha256, common)
    collection_raw = read(stage / 'checkpoint-evidence' / 'collection.json', 4 * 1024**2)
    collection = common['document'](collection_raw)
    if (receipt.get('collection_sha256') != common['sha'](collection_raw)
            or collection['checkpoint_id'] != receipt['checkpoint_id']
            or collection['policy_sha256'] != args.policy_sha256
            or collection['quiescence_verified'] is not True):
        raise ValueError('restore policy or collection evidence differs from the selected stage')
    checkpoint = receipt['checkpoint_id']
    original = {row['path']: row for row in collection['originals']}
    if len(original) != len(collection['originals']):
        raise ValueError('duplicate collection record')
    REVIEW['open_images'](stage, args.stage_sha256, common, budget)
    members = {}
    for role, rows in manifest['roles'].items():
        marker = (checkpoint + '\n').encode()
        members[role] = {REVIEW['recovery_marker'](role): {'sha256': common['sha'](marker), 'bytes': len(marker)}}
        members[role].update({row['name']: {'sha256': row['sha256'], 'bytes': row['bytes']} for row in rows})

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
        prepared = {}
        empty = {}
        complete = []
        for role, state in states.items():
            common['protected_directory'](state.parent)
            if os.path.lexists(state):
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
            # Source and destination share a protected filesystem parent. GNU mv
            # supplies no-clobber publication even when the other entry is empty.
            result = subprocess.run(['/usr/bin/mv', '-nT', '--', str(staged), str(state)],
                                    stdin=subprocess.DEVNULL, capture_output=True,
                                    timeout=budget.remaining(15), env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
            if result.returncode or os.path.lexists(staged):
                raise ValueError('fenced destination appeared during publication')
            common['sync'](state.parent)
            after_publish(role)

        guard()
        for role, state in states.items():
            verify_directory(state, workers[role], members[role], common, budget)
        for item in tracked:
            item.check()
        return {'result': 'CASHU_RESTORE_INSTALLED_FENCED', 'version': 1,
                'checkpoint_id': checkpoint, 'stage_sha256': args.stage_sha256,
                'policy_sha256': args.policy_sha256, 'tool': tool_identity,
                'roles': sorted(states), 'already_installed': sorted(complete),
                'external_reconciliation_verified': False, 'resume_authorized': False,
                'services_changed': False}


def main():
    if 'verified_tool_identity' not in globals() or os.geteuid() != 0:
        raise SystemExit('Use the externally verified root-owned backup launcher.')
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('stage', 'policy'):
        parser.add_argument(name, type=Path)
        parser.add_argument(name + '_sha256')
    args = parser.parse_args()
    try:
        common = runpy.run_path(str(HERE / 'collect-cashu-backup.py'))
        result = install(args, common, verified_tool_identity())
    except (OSError, ValueError, TypeError, KeyError, subprocess.TimeoutExpired):
        raise SystemExit('Cashu fenced installation refused. Preserve the checkpoint and any installed fences.')
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
