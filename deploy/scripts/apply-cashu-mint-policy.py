#!/usr/bin/env python3
"""Install one reviewed mint policy while the wallet is masked and stopped."""
import argparse
from contextlib import ExitStack, closing
import fcntl
import json
import os
from pathlib import Path
import re
import runpy
import stat
import subprocess
import tempfile
from uuid import UUID


def apply(args, verify_tools):
    tool = verify_tools()
    here = Path(__file__).resolve().parent
    common = runpy.run_path(str(here / 'collect-cashu-backup.py'))
    tx = runpy.run_path(str(here / 'inactive_file_transaction.py'))
    budget = common['Budget'](1024**3, 120)
    selected = [args.policy_sha256, args.baseline_sha256, args.candidate_sha256]
    if len(set(selected[1:])) != 2 or any(not re.fullmatch('[0-9a-f]{64}', value) for value in selected):
        raise ValueError('distinct reviewed configuration digests required')
    policy, _ = common['load_policy'](args.policy, args.policy_sha256, budget)
    worker = policy['workers']['wallet']
    state = Path(worker['state'])
    current = Path(policy['wallet_config'])
    artifact = policy['runtime_artifacts']['wallet']
    binary = Path(artifact['path'])
    if len({args.policy, current, args.baseline, args.candidate, binary}) != 5:
        raise ValueError('separate protected original and candidate required')

    lock_path = common['GLOBAL_LOCK']
    common['protected_directory'](lock_path.parent, private=True)
    with ExitStack() as stack:
        fd = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, 0o600)
        stack.callback(os.close, fd)
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1 or info.st_mode & 0o077:
            raise ValueError('unsafe global maintenance lock')
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        identity = common['identity']
        lock_identity = identity(os.fstat(fd))

        def frozen(path, size=65536, *, uid=0, gid=None, empty=False):
            return stack.enter_context(closing(common['Frozen'](path, uid, size, budget,
                secret=False, gid=gid, empty=empty)))

        policy_file = frozen(args.policy)
        baseline = frozen(args.baseline)
        candidate = frozen(args.candidate)
        executable = frozen(binary, 512 * 1024**2)
        if [policy_file.digest, baseline.digest, candidate.digest] != selected or executable.digest != artifact['sha256']:
            raise ValueError('protected inputs differ from review')
        # Only the independently inventoried wallet may be used as the review
        # subject, and the original config remains available for exact retry.
        baseline.file.seek(0)
        original = common['document'](baseline.file.read())
        if Path(original['state_directory']) != state or original['runtime_uid'] != worker['uid']:
            raise ValueError('reviewed config differs from installed wallet identity')
        live = frozen(current)
        fingerprint = tx['fingerprint']
        old = fingerprint(args.baseline)
        new = dict(old, sha256=candidate.digest)
        installed = fingerprint(current)
        if old['sha256'] != baseline.digest or installed not in (old, new) or live.digest != installed['sha256']:
            raise ValueError('installed config drift requires a new review')

        def guard():
            common['check_quiescence']({'wallet': worker}, budget)
            if identity(lock_path.lstat()) != lock_identity or os.path.lexists(state / 'CASHU_RECOVERY_REQUIRED'):
                raise ValueError('maintenance lock changed or recovery fence present')

        guard()
        reviewed = subprocess.run(
            ['/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
             f"--reuid={worker['uid']}", f"--regid={worker['gid']}", '--clear-groups',
             '--no-new-privs', '--', str(binary), 'review-mint-policy', str(args.baseline), str(args.candidate)],
            stdin=subprocess.DEVNULL, capture_output=True, timeout=budget.remaining(30),
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
        )
        if reviewed.returncode or not 0 < len(reviewed.stdout) <= 65536:
            raise ValueError('native mint policy review refused')
        witness = common['document'](reviewed.stdout)
        if (witness['version'] != 1 or witness['applied'] is not False
                or witness['current_sha256'] != baseline.digest or witness['candidate_sha256'] != candidate.digest
                or witness['wallet_instance'] != str(UUID(original['wallet_instance']))
                or witness['wallet_epoch'] != str(UUID(original['wallet_epoch']))):
            raise ValueError('native mint policy review differs from approved change')

        # Hold the same lock as native startup through configuration publication.
        # It must already exist and belong to the wallet service identity.
        wallet_lock = frozen(state / 'wallet-writer.lock', 4096, uid=worker['uid'], gid=worker['gid'], empty=True)
        if wallet_lock.expected[4] & 0o077:
            raise ValueError('unsafe wallet writer lock')
        fcntl.flock(wallet_lock.file, fcntl.LOCK_EX | fcntl.LOCK_NB)

        def preinstall():
            for item in (policy_file, baseline, candidate, executable, wallet_lock, live):
                item.check()
            guard()
            if fingerprint(current) != installed:
                raise ValueError('installed config changed during review')

        if installed == old:
            with tempfile.TemporaryDirectory(prefix='.lg-mint-policy-', dir=current.parent) as directory:
                staged = Path(directory) / 'config'
                tx['copy_durable'](args.candidate, staged, new)
                preinstall()
                os.replace(staged, current)
        else:
            preinstall()
        tx['sync_directory'](current.parent)
        guard()
        if fingerprint(current) != new:
            raise ValueError('installed mint policy readback differs')
        return {'result': 'CASHU_MINT_POLICY_CONFIG_INSTALLED', 'review': witness,
                'tool': tool, 'already_installed': installed == new, 'services_activated': False}


def main():
    if 'verified_tool_identity' not in globals() or os.geteuid() != 0:
        raise SystemExit('Use the externally verified root-owned maintenance launcher.')
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('policy', 'baseline', 'candidate'):
        parser.add_argument(name, type=Path)
        parser.add_argument(name + '_sha256')
    args = parser.parse_args()
    try:
        result = apply(args, verified_tool_identity)
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
        reason = error.strerror if isinstance(error, OSError) else str(error)
        raise SystemExit('Mint policy installation failed. Check the installed config digest before retrying. '
                         + (reason or type(error).__name__))
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
