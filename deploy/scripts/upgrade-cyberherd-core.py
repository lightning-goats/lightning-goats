#!/usr/bin/env python3
"""Explicit paid-core upgrade, retaining state and leaving the service stopped.

Run through the authenticated launcher with --cyberherd-core. Prepare is read-only
on the installation. Apply stops/masks only the selected existing core, accepts a
native legacy backup, replaces two programs and migrates as the original UID.
Finish restores the original unit only after comparison. No command starts a
service, enables CyberHerd, restores an old database or rotates its epoch.
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
import pwd
import grp
import stat
import subprocess
import tempfile
import tomllib

import inactive_file_transaction as TX

SPEC = importlib.util.spec_from_file_location('paid_core_preflight', Path(__file__).with_name('preflight-release.py'))
PREFLIGHT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREFLIGHT)
BASE = Path('/var/lib/lightning-goats-upgrades')
CONFIG = Path('/etc/lightning-goats/config.receive.toml')
DATABASE = Path('/var/lib/lightning-goats/lightning-goats.db')
UNIT_NAME = 'lightning-goats-receive.service'
UNIT = Path('/etc/systemd/system') / UNIT_NAME
TARGETS = {name: Path('/usr/local/bin') / name for name in ('lightning-goatsd', 'lightning-goatsctl')}
NEW_UNITS = ('cyberherd.service', 'cyberherd-payout.service', 'cyberherd-signer.service')
PROPERTIES = 'Id,LoadState,ActiveState,SubState,MainPID,UnitFileState,FragmentPath,DropInPaths,NeedDaemonReload,ControlGroup,User,Group'


def command(*args, timeout=30):
    # No inherited credentials, proxy, linker or Python configuration. Do not
    # reproduce captured command output (including private status) in errors.
    result = subprocess.run(args, env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'},
                            capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise ValueError('upgrade subprocess failed; preserve the private transaction and stopped state')
    return result.stdout


def protected(path, directory=False, private=False):
    path = Path(path)
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise ValueError('canonical protected path required')
    for parent in reversed(path.parents):
        info = parent.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError('unprotected administrative parent')
        if any(n.startswith('system.posix_acl') for n in os.listxattr(parent)):
            raise ValueError('unreviewed administrative parent ACL')
    info = path.lstat()
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    if (not kind(info.st_mode) or info.st_uid != 0 or info.st_gid != 0 or info.st_mode & 0o022
            or (private and info.st_mode & 0o077) or (not directory and info.st_nlink != 1)):
        raise ValueError('unprotected administrative object')
    if any(n.startswith('system.posix_acl') or n == 'security.capability' for n in os.listxattr(path)):
        raise ValueError('unreviewed administrative ACL/capability')


def root():
    if os.geteuid() != 0 or not sys.flags.isolated or not sys.flags.no_site:
        raise ValueError('root and isolated authenticated launcher required')
    verified_tool_identity()
    os.umask(0o077)


def write_json(path, value):
    with path.open('x') as stream:
        os.fchmod(stream.fileno(), 0o600)
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    TX.sync_directory(path.parent)


def read_json(path):
    protected(path, private=True)
    if path.stat().st_size > 2 * 1024 * 1024:
        raise ValueError('upgrade record exceeds bound')
    return json.loads(path.read_text())


def status(name):
    raw = command('/usr/bin/systemctl', 'show', '--no-pager', '--property=' + PROPERTIES, '--', name)
    values = dict(line.split('=', 1) for line in raw.splitlines())
    if set(values) != set(PROPERTIES.split(',')) or values['Id'] != name:
        raise ValueError('incomplete unit identity')
    return values


def new_roles_absent():
    for name in NEW_UNITS:
        value = status(name)
        if value['LoadState'] != 'not-found' or value['ActiveState'] != 'inactive' or value['MainPID'] != '0':
            raise ValueError('new CyberHerd roles must be absent for a legacy-core upgrade')


def unit_files(value):
    if value['FragmentPath'] != str(UNIT) or value['NeedDaemonReload'] != 'no':
        raise ValueError('requires the reviewed custom core unit with no pending manager reload')
    paths = [UNIT, *(Path(p) for p in value['DropInPaths'].split())]
    for path in paths:
        protected(path)
    return {str(p): TX.fingerprint(p) for p in paths}


def baseline():
    protected(CONFIG)
    config = tomllib.loads(CONFIG.read_text())
    if config['database']['url'] != 'sqlite://' + str(DATABASE) or config.get('core_bridge'):
        raise ValueError('requires the original legacy receive configuration without a CyberHerd bridge')
    user = pwd.getpwnam('lightning-goats')
    if user.pw_uid == 0 or user.pw_gid == 0 or user.pw_shell != '/usr/sbin/nologin':
        raise ValueError('original non-admin core runtime identity required')
    groups = {grp.getgrgid(gid).gr_name for gid in os.getgrouplist(user.pw_name, user.pw_gid)}
    if groups & {'root', 'sudo', 'wheel', 'admin', 'docker', 'lxd', 'disk'}:
        raise ValueError('core runtime retains an administrative group')
    new_roles_absent()
    value = status(UNIT_NAME)
    if value['User'] != user.pw_name or value['Group'] not in ('', grp.getgrgid(user.pw_gid).gr_name):
        raise ValueError('effective core unit uses a different runtime identity')
    if value['LoadState'] != 'loaded' or value['ActiveState'] != 'active' or value['SubState'] != 'running' or not value['MainPID'].isdigit() or value['MainPID'] == '0':
        raise ValueError('requires the original running core')
    files = unit_files(value)
    for path in [CONFIG, *TARGETS.values()]:
        protected(path)
        files[str(path)] = TX.fingerprint(path)
        if path in TARGETS.values() and files[str(path)]['mode'] != 0o755:
            raise ValueError('installed core binaries require reviewed executable permissions')
    return {'unit': value, 'files': files, 'uid': user.pw_uid, 'gid': user.pw_gid}


def prepare(archive, source, digest, destination):
    root()
    protected(BASE, directory=True, private=True)
    if destination.parent != BASE:
        raise ValueError('new transaction must be directly under the protected upgrade base')
    original = baseline()
    destination.mkdir(mode=0o700)  # Never reuse a partial or previous upgrade.
    TX.sync_directory(BASE)
    staged = destination / 'release.tar.gz'
    with PREFLIGHT.regular_archive(archive) as incoming, staged.open('xb') as outgoing:
        os.fchmod(outgoing.fileno(), 0o600)
        total = 0
        while chunk := incoming.read(1024 * 1024):
            total += len(chunk)
            if total > PREFLIGHT.MAX_ARCHIVE_BYTES:
                raise ValueError('archive exceeds bound')
            outgoing.write(chunk)
        outgoing.flush()
        os.fsync(outgoing.fileno())
    release = PREFLIGHT.preflight(staged, source, digest)
    payload = destination / 'payload'
    payload.mkdir(mode=0o700)
    PREFLIGHT.RELEASE.verify_archive(staged, source, payload)
    # The frozen tool verifier, never a module from the archive, authenticates
    # bytes before any selected binary executes. Build provenance is external.
    ctl = payload / 'lightning-goatsctl'
    command(str(ctl), '--help')
    policy = {'version': 1, 'max_component_bytes': 4 * 1024 ** 3,
              'core': {'database': str(DATABASE), 'uid': original['uid'], 'gid': original['gid'], 'unit': UNIT_NAME}}
    policy_text = f'version = 1\nmax_component_bytes = {policy["max_component_bytes"]}\n[core]\ndatabase = "{DATABASE}"\nuid = {original["uid"]}\ngid = {original["gid"]}\nunit = "{UNIT_NAME}"\n'
    with (destination / 'policy.toml').open('x') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(policy_text)
        stream.flush()
        os.fsync(stream.fileno())
    command(str(ctl), 'check-core-upgrade', '--policy', str(destination / 'policy.toml'))
    for index, path in enumerate(original['files']):
        TX.copy_durable(path, destination / f'original-{index}', original['files'][path])
    TX.prepare(destination / 'files', [(target, payload / name, original['files'][str(target)])
                                     for name, target in TARGETS.items()], lambda: require_baseline(original))
    write_json(destination / 'UPGRADE.json', {'version': 1, 'tool': verified_tool_identity(), 'baseline': original,
                                           'original_order': list(original['files']),
                                           'release': release, 'policy': policy,
                                           'policy_sha256': hashlib.sha256(policy_text.encode()).hexdigest()})
    return {'prepared': True, 'service_changed': False, 'state_opened': False}


def require_baseline(expected):
    if baseline() != expected:
        raise ValueError('reviewed installation changed before upgrade')


def load(directory):
    root()
    protected(BASE, directory=True, private=True)
    if directory.parent != BASE:
        raise ValueError('transaction outside protected upgrade base')
    protected(directory, directory=True, private=True)
    metadata = read_json(directory / 'UPGRADE.json')
    if metadata['version'] != 1 or metadata['tool'] != verified_tool_identity():
        raise ValueError('upgrade/tool identity changed')
    protected(directory / 'policy.toml', private=True)
    if hashlib.sha256((directory / 'policy.toml').read_bytes()).hexdigest() != metadata['policy_sha256']:
        raise ValueError('upgrade policy changed')
    records = read_json(directory / 'files/TRANSACTION.json')
    expected = {str(path): name for name, path in TARGETS.items()}
    if records['version'] != 1 or len(records['files']) != len(expected):
        raise ValueError('invalid replacement transaction')
    seen = set()
    for row in records['files']:
        path = row['destination']
        if path not in expected or path in seen:
            raise ValueError('unexpected replacement destination')
        seen.add(path)
        old = metadata['baseline']['files'][path]
        new = dict(old, sha256=metadata['release']['payload_sha256'][expected[path]])
        if row['old'] != old or row['new'] != new:
            raise ValueError('replacement differs from pinned source')
    order = metadata['original_order']
    if len(order) != len(set(order)) or set(order) != set(metadata['baseline']['files']):
        raise ValueError('invalid original installation backup order')
    for index, path in enumerate(order):
        expected_file = metadata['baseline']['files'][path]
        protected(directory / f'original-{index}')
        if TX.fingerprint(directory / f'original-{index}') != expected_file:
            raise ValueError('original installation backup changed')
    return metadata


def stopped(metadata):
    verified_tool_identity()
    new_roles_absent()
    value = status(UNIT_NAME)
    if (value['LoadState'] != 'masked' or value['ActiveState'] != 'inactive' or value['SubState'] != 'dead'
            or value['MainPID'] != '0' or value['UnitFileState'] != 'masked' or value['NeedDaemonReload'] != 'no'
            or not UNIT.is_symlink() or os.readlink(UNIT) != '/dev/null'):
        raise ValueError('core must remain stopped and persistently masked')
    group = value['ControlGroup']
    if group:
        if not group.startswith('/') or any(p in ('.', '..') for p in group.split('/')):
            raise ValueError('unexpected core control group')
        events = Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.events'
        if events.exists() and 'populated 0' not in events.read_text().splitlines():
            raise ValueError('core control group remains populated')
    # Check the actual old runtime UID, not only systemd's MainPID.
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            lines = (process / 'status').read_text().splitlines()
        except FileNotFoundError:
            continue
        if any(line.startswith('Uid:') and metadata['baseline']['uid'] in map(int, line.split()[1:]) for line in lines):
            raise ValueError('another original-runtime writer exists')
    for path, original in metadata['baseline']['files'].items():
        if path in map(str, TARGETS.values()) or path == str(UNIT):
            continue
        protected(Path(path))
        if TX.fingerprint(path) != original:
            raise ValueError('original configuration or drop-in changed')


def mask_unit(metadata):
    command('/usr/bin/systemctl', 'stop', '--', UNIT_NAME, timeout=120)
    value = status(UNIT_NAME)
    if value['ActiveState'] != 'inactive' or value['SubState'] != 'dead' or value['MainPID'] != '0':
        raise ValueError('core did not stop')
    if unit_files(value) != {p: f for p, f in metadata['baseline']['files'].items()
                             if p == str(UNIT) or p in metadata['baseline']['unit']['DropInPaths'].split()}:
        raise ValueError('unit changed after stop')
    # /etc contains the original custom unit. A runtime mask would lose to it.
    # Its synced original is retained in this transaction before replacement.
    with tempfile.TemporaryDirectory(prefix='.lg-mask-', dir=UNIT.parent) as temporary:
        staged = Path(temporary) / UNIT.name
        staged.symlink_to('/dev/null')
        os.replace(staged, UNIT)
        TX.sync_directory(UNIT.parent)
    command('/usr/bin/systemctl', 'daemon-reload')
    stopped(metadata)


def native(directory, *args):
    # After apply, use the installed protected copy, not a re-opened archive.
    protected(TARGETS['lightning-goatsctl'])
    return json.loads(command(str(TARGETS['lightning-goatsctl']), *args, timeout=300))


def capture(directory, metadata, output):
    stopped(metadata)
    protected(DATABASE.parent.parent, directory=True)
    parent = DATABASE.parent.lstat()
    if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != metadata['baseline']['uid'] or parent.st_mode & 0o077:
        raise ValueError('invalid original state directory')
    output.mkdir(mode=0o700)
    TX.sync_directory(output.parent)
    # Never accept a hot rollback journal as an apparently complete main/WAL
    # snapshot. Failed copies retain a marker and are never parsed as complete.
    if Path(str(DATABASE) + '-journal').exists() or Path(str(DATABASE) + '-journal').is_symlink():
        raise ValueError('unexpected original rollback journal')
    with (output / 'INCOMPLETE').open('x') as marker:
        os.fchmod(marker.fileno(), 0o600)
        marker.write('Unaccepted migrated core copy.\n')
        marker.flush()
        os.fsync(marker.fileno())
    observed = {}
    for suffix in ('', '-wal', '-shm'):
        path = Path(str(DATABASE) + suffix)
        if not path.exists() and not path.is_symlink():
            observed[suffix] = None
            continue
        info = path.lstat()
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != metadata['baseline']['uid']
                or info.st_gid != metadata['baseline']['gid'] or info.st_mode & 0o077 or info.st_nlink != 1
                or info.st_size > metadata['policy']['max_component_bytes']):
            raise ValueError('invalid original journal identity')
        observed[suffix] = TX.fingerprint(path)
        if suffix != '-shm':
            copy_raw_image(path, output / ('core.db' + suffix), observed[suffix])
    if observed[''] is None:
        raise ValueError('missing original core')
    stopped(metadata)
    for suffix, before in observed.items():
        path = Path(str(DATABASE) + suffix)
        current = TX.fingerprint(path) if path.exists() or path.is_symlink() else None
        if before != current:
            raise ValueError('original journal changed while copying')
    (output / 'INCOMPLETE').unlink()
    TX.sync_directory(output)
    return observed


def copy_raw_image(source, destination, expected):
    # A raw journal image keeps its destination's ordinary filesystem label;
    # never remove security.selinux or copy a runtime-domain label onto it.
    # O_PATH pins a regular source inode before reopening it for bounded reads.
    descriptor = os.open(source, os.O_PATH | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise ValueError('raw image source is not a regular file')
        digest = hashlib.sha256()
        total = 0
        with open(f'/proc/self/fd/{descriptor}', 'rb') as incoming, destination.open('xb') as outgoing:
            os.fchmod(outgoing.fileno(), 0o600)
            while chunk := incoming.read(1024 * 1024):
                total += len(chunk)
                if total > 4 * 1024 ** 3:
                    raise ValueError('raw source grew beyond budget')
                digest.update(chunk)
                outgoing.write(chunk)
            outgoing.flush()
            os.fsync(outgoing.fileno())
        if digest.hexdigest() != expected['sha256'] or TX.fingerprint(source) != expected:
            raise ValueError('raw image source changed during copying')
    finally:
        os.close(descriptor)


def apply(directory, writers_stopped):
    if not writers_stopped:
        raise ValueError('explicit exclusion of all other administrative/database writers required')
    metadata = load(directory)
    with TX.locked(directory / 'files'):
        require_baseline(metadata['baseline'])
        # Exclusive marker makes failed/interrupted application a manual review,
        # never an implicit rerun or older-database restore.
        write_json(directory / 'APPLY-STARTED.json', {'started': True})
        mask_unit(metadata)
        ctl = directory / 'payload/lightning-goatsctl'
        protected(ctl)
        if TX.fingerprint(ctl)['sha256'] != metadata['release']['payload_sha256']['lightning-goatsctl']:
            raise ValueError('selected native verifier changed')
        command(str(ctl), 'backup-legacy-core', '--policy', str(directory / 'policy.toml'),
                '--destination', str(directory / 'legacy'), '--writers-stopped', timeout=300)
        command(str(ctl), 'verify-core-upgrade', '--policy', str(directory / 'policy.toml'), '--bundle', str(directory / 'legacy'), timeout=300)
    # execute owns this same lock itself; the exclusive APPLY-STARTED marker
    # prevents another application from reaching this point after release.
    TX.execute(directory / 'files', lambda: stopped(metadata))
    stopped(metadata)
    write_json(directory / 'MIGRATION-STARTED.json', {'started': True, 'database_restore_authorized': False})
    command('/usr/bin/setpriv', '--reuid=' + str(metadata['baseline']['uid']), '--regid=' + str(metadata['baseline']['gid']),
            '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
            str(TARGETS['lightning-goatsctl']), '--config', str(CONFIG), 'status', timeout=300)
    observed = capture(directory, metadata, directory / 'migrated')
    receipt = native(directory, 'verify-core-upgrade', '--policy', str(directory / 'policy.toml'),
                     '--bundle', str(directory / 'legacy'), '--candidate-copy', str(directory / 'migrated'))
    if receipt.get('accepted') is not True or receipt.get('candidate_compared') is not True or receipt.get('resume_authorized') is not False:
        raise ValueError('native comparison did not positively accept the upgrade')
    write_json(directory / 'COMPARISON.json', {'native': receipt, 'source': observed})
    return {'upgraded': True, 'core_masked': True, 'core_started': False, 'cyberherd_enabled': False}


def finish(directory, approved):
    if not approved:
        raise ValueError('explicit unit-restoration approval required; this does not start the core')
    metadata = load(directory)
    with TX.locked(directory / 'files'):
        stopped(metadata)
        accepted = read_json(directory / 'COMPARISON.json')
        records = read_json(directory / 'files/TRANSACTION.json')
        for row in records['files']:
            if TX.fingerprint(row['destination']) != row['new']:
                raise ValueError('installed program changed after comparison')
        for suffix, expected in accepted['source'].items():
            path = Path(str(DATABASE) + suffix)
            if (TX.fingerprint(path) if path.exists() or path.is_symlink() else None) != expected:
                raise ValueError('paid state changed after accepted migration')
        receipt = native(directory, 'verify-core-upgrade', '--policy', str(directory / 'policy.toml'),
                         '--bundle', str(directory / 'legacy'), '--candidate-copy', str(directory / 'migrated'))
        if receipt != accepted['native']:
            raise ValueError('native accepted upgrade receipt changed')
        index = metadata['original_order'].index(str(UNIT))
        with tempfile.TemporaryDirectory(prefix='.lg-unit-', dir=UNIT.parent) as temporary:
            staged = Path(temporary) / UNIT.name
            TX.copy_durable(directory / f'original-{index}', staged, metadata['baseline']['files'][str(UNIT)])
            stopped(metadata)
            os.replace(staged, UNIT)
            TX.sync_directory(UNIT.parent)
        command('/usr/bin/systemctl', 'daemon-reload')
        value = status(UNIT_NAME)
        if value['ActiveState'] != 'inactive' or value['SubState'] != 'dead' or value['MainPID'] != '0' or unit_files(value) != {
                p: f for p, f in metadata['baseline']['files'].items() if p == str(UNIT) or p in metadata['baseline']['unit']['DropInPaths'].split()}:
            raise ValueError('original unit restoration requires inspection; no start was requested')
        write_json(directory / 'UNIT-RESTORED.json', {'core_started': False})
    return {'original_unit_restored': True, 'core_started': False, 'explicit_core_restart_required': True, 'cyberherd_enabled': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    plan = commands.add_parser('prepare')
    plan.add_argument('archive', type=Path)
    plan.add_argument('source')
    plan.add_argument('sha256')
    plan.add_argument('directory', type=Path)
    update = commands.add_parser('apply')
    update.add_argument('directory', type=Path)
    update.add_argument('--writers-stopped', action='store_true', required=True)
    restore = commands.add_parser('finish')
    restore.add_argument('directory', type=Path)
    restore.add_argument('--restore-original-unit', action='store_true', required=True)
    args = parser.parse_args()
    if args.operation == 'prepare':
        result = prepare(args.archive, args.source, args.sha256, args.directory)
    elif args.operation == 'apply':
        result = apply(args.directory, args.writers_stopped)
    else:
        result = finish(args.directory, args.restore_original_unit)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
