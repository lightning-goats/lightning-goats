#!/usr/bin/env python3
"""Install three disabled, masked CyberHerd roles without loading credentials.

Use the externally authenticated launcher. The optional identities step creates
only the fixed non-admin users/groups. Install requires existing reviewed public
configuration, an independently pinned review archive and native legacy-core
backup evidence. It never changes the core service/configuration, starts a unit,
loads a credential or contacts a provider, relay or physical gateway.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import grp
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import re
import stat
import subprocess
import tarfile
import tomllib


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


C = helper('inactive_core_support', 'upgrade-cyberherd-core.py')
C.verified_tool_identity = lambda: verified_tool_identity()
PACKAGE = helper('inactive_package_verifier', 'verify-cyberherd-package.py')
ROLES = {
    'cyberherd': ('cyberherdd', 'config', 'cyberherd.db'),
    'cyberherd-payout': ('cyberherd-payoutd', 'payout', 'payout.db'),
    'cyberherd-signer': ('cyberherd-signerd', 'signer', 'signer.db'),
}
GROUPS = {'cyberherd-bridge', 'cyberherd-plans', 'cyberherd-payout-signer'}
MEMBERSHIPS = {
    'cyberherd': {'cyberherd', 'cyberherd-bridge', 'cyberherd-plans'},
    'cyberherd-payout': {'cyberherd-payout', 'cyberherd-bridge', 'cyberherd-plans', 'cyberherd-payout-signer'},
    'cyberherd-signer': {'cyberherd-signer', 'cyberherd-payout-signer'},
}
BIN = Path('/usr/local/bin')
ETC = Path('/etc')
STATE = Path('/var/lib')
MASKS = Path('/etc/systemd/system')
UNITS = Path('/usr/local/lib/systemd/system')


def absent(path):
    if path.exists() or path.is_symlink():
        raise ValueError('inactive installation refuses any existing destination or partial state')


def require_uninstalled():
    for role, (binary, _, _) in ROLES.items():
        C.protected(MASKS, directory=True)
        value = C.status(role + '.service')
        if value['LoadState'] != 'not-found' or value['ActiveState'] != 'inactive' or value['MainPID'] != '0':
            raise ValueError('new roles must remain absent before fresh installation')
        for path in [BIN / binary, ETC / role, STATE / role, MASKS / (role + '.service'), UNITS / (role + '.service')]:
            absent(path)
    absent(BIN / 'cyberherd-statectl')


def identities(create):
    C.root()
    if not create:
        raise ValueError('explicit identity-creation flag required')
    require_uninstalled()
    for role in ROLES:
        try:
            pwd.getpwnam(role)
        except KeyError:
            pass
        else:
            raise ValueError('existing or partial runtime identity requires inspection, never recreation')
    for group in GROUPS | set(ROLES):
        try:
            grp.getgrnam(group)
        except KeyError:
            pass
        else:
            raise ValueError('existing or partial runtime group requires inspection')
    definitions = [f'g {name} -' for name in sorted(GROUPS)]
    definitions += [f'u {role} - "CyberHerd service" {STATE / role} /usr/sbin/nologin' for role in ROLES]
    definitions += [f'm {role} {group}' for role, groups in MEMBERSHIPS.items() for group in sorted(groups - {role})]
    C.command('/usr/bin/systemd-sysusers', '--inline', *definitions)
    result = check_identities()
    return {'identities_created': True, 'runtime': result, 'services_installed': False, 'services_started': False}


def check_identities():
    result = {}
    for role in ROLES:
        user = pwd.getpwnam(role)
        primary = grp.getgrnam(role)
        groups = {grp.getgrgid(gid).gr_name for gid in os.getgrouplist(role, user.pw_gid)}
        if (user.pw_uid == 0 or user.pw_gid == 0 or user.pw_gid != primary.gr_gid
                or user.pw_shell != '/usr/sbin/nologin' or user.pw_dir != str(STATE / role)
                or groups != MEMBERSHIPS[role]):
            raise ValueError('unexpected runtime UID/home/shell/primary or supplementary groups')
        locked = C.command('/usr/bin/passwd', '-S', role).split()
        if len(locked) < 2 or locked[1] not in ('L', 'LK'):
            raise ValueError('runtime password is not locked')
        require_no_sudo(role)
        result[role] = {'uid': user.pw_uid, 'gid': user.pw_gid}
    if len({row['uid'] for row in result.values()}) != 3 or len({row['gid'] for row in result.values()}) != 3:
        raise ValueError('runtime identities must be distinct')
    core = pwd.getpwnam('lightning-goats')
    if core.pw_uid == 0 or core.pw_uid in {row['uid'] for row in result.values()}:
        raise ValueError('core must retain its separate non-root identity')
    return result


def require_no_sudo(role):
    result = subprocess.run(['/usr/bin/sudo', '-n', '-l', '-U', role],
                            env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'},
                            capture_output=True, text=True, timeout=15)
    denial = rf'User {re.escape(role)} is not allowed to run sudo on [^\s]+\.\n?'
    if result.returncode not in (0, 1) or result.stderr or re.fullmatch(denial, result.stdout) is None:
        raise ValueError('cannot establish that new runtime identity has no sudo authority')


def configs(source, runtime, evidence):
    C.protected(source, directory=True)
    values, bytes_by_role = {}, {}
    for role in ROLES:
        path = source / (role + '.toml')
        C.protected(path)
        if path.stat().st_size > 65536:
            raise ValueError('configuration exceeds budget')
        raw = path.read_bytes()
        values[role] = tomllib.loads(raw.decode())
        bytes_by_role[role] = raw
        if values[role].get('enabled') is not False or values[role].get('state_dir') != str(STATE / role):
            raise ValueError('only disabled fixed-path configurations may be installed')
    intake, payout, signer = (values[role] for role in ROLES)
    scope = payout['policy']['scope']
    policy = payout['policy']
    if (policy['execution_enabled'] is not False or any(policy[name] != 0 for name in
            ('member_percent_ceiling', 'max_sats_per_feed', 'max_sats_per_24h', 'max_fee_sats'))
            or signer['policy']['max_net_sats'] != 0 or signer['policy']['max_new_per_hour'] != 0):
        raise ValueError('initial execution, signing admission and all monetary ceilings must be zero')
    if intake.get('messaging', {}).get('enabled', False) or intake.get('view', {}).get('enabled', False):
        raise ValueError('initial messaging and public view must remain disabled')
    core_uid = pwd.getpwnam('lightning-goats').pw_uid
    if (intake['core']['core_uid'] != core_uid or scope['core_uid'] != core_uid
            or evidence['policy']['core']['uid'] != core_uid
            or scope['epoch'] != intake['core']['epoch'] or scope['epoch'] != evidence['report']['epoch']
            or scope['owner'] != intake['project_pubkey'] or scope['owner'] != signer['policy']['owner']):
        raise ValueError('configuration must retain the actual core UID/epoch and one owner')
    if (payout['intake_uid'] != runtime['cyberherd']['uid'] or payout['signer_uid'] != runtime['cyberherd-signer']['uid']
            or signer['policy']['peer_uid'] != runtime['cyberherd-payout']['uid']
            or signer['group_gid'] != grp.getgrnam('cyberherd-payout-signer').gr_gid):
        raise ValueError('configured peer identities/groups differ from installed NSS identities')
    if intake.get('payout_bridge'):
        bridge = intake['payout_bridge']
        if bridge['peer_uid'] != runtime['cyberherd-payout']['uid'] or bridge['group_gid'] != grp.getgrnam('cyberherd-plans').gr_gid:
            raise ValueError('intake bridge differs from the dedicated payout peer/group')
    provider = scope['provider']
    if provider['environment'] != 'strike-production' or provider['credential_fingerprint'] == '0' * 64:
        raise ValueError('real selected payout scope is required; do not bind a placeholder credential')
    return bytes_by_role


def publish(path, raw, mode):
    absent(path)
    with path.open('xb') as stream:
        os.fchmod(stream.fileno(), mode)
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    C.TX.sync_directory(path.parent)


def install(archive, source, digest, config_source, policy, bundle, destination, approved):
    C.root()
    if not approved:
        raise ValueError('explicit disabled-installation flag required')
    C.protected(C.BASE, directory=True, private=True)
    if destination.parent != C.BASE:
        raise ValueError('new install transaction must be directly under the protected upgrade base')
    for parent in [BIN, ETC, STATE, MASKS]:
        C.protected(parent, directory=True)
    for parent in reversed([UNITS, *UNITS.parents]):
        if parent.exists():
            C.protected(parent, directory=True)
    require_uninstalled()
    runtime = check_identities()
    # Native verification opens only retained copies. Never discover/migrate the
    # actual core database with ordinary status as an installation side effect.
    C.protected(BIN / 'lightning-goatsctl')
    accepted = json.loads(C.command(str(BIN / 'lightning-goatsctl'), 'verify-core-upgrade', '--policy', str(policy), '--bundle', str(bundle), timeout=300))
    if accepted.get('accepted') is not True or accepted.get('resume_authorized') is not False:
        raise ValueError('native core evidence was not accepted')
    evidence = C.read_json(bundle / 'MANIFEST.json')
    raw_configs = configs(config_source, runtime, evidence)
    destination.mkdir(mode=0o700)
    C.TX.sync_directory(destination.parent)
    C.write_json(destination / 'INCOMPLETE', {'installed': False, 'services_started': False})
    staged_archive = destination / 'review.tar.gz'
    with C.PREFLIGHT.regular_archive(archive) as incoming, staged_archive.open('xb') as outgoing:
        os.fchmod(outgoing.fileno(), 0o600)
        total, actual = 0, hashlib.sha256()
        while chunk := incoming.read(1024 * 1024):
            total += len(chunk)
            if total > 512 * 1024 * 1024:
                raise ValueError('review archive exceeds bound')
            actual.update(chunk)
            outgoing.write(chunk)
        outgoing.flush()
        os.fsync(outgoing.fileno())
    if actual.hexdigest() != digest:
        raise ValueError('independent review archive digest differs')
    reviewed = PACKAGE.verify(staged_archive, source)
    selected = {binary for binary, _, _ in ROLES.values()} | {'cyberherd-statectl'}
    selected |= {f'deploy/systemd/{role}.service.example' for role in ROLES}
    payload = {}
    with tarfile.open(staged_archive, 'r:gz') as archive_reader:
        for member in archive_reader:
            name = str(Path(member.name))
            if name in selected:
                payload[name] = archive_reader.extractfile(member).read()
    if set(payload) != selected:
        raise ValueError('verified review archive lacks selected installation files')
    # Freeze all inputs before the first installed path is created.
    for role, raw in raw_configs.items():
        publish(destination / (role + '.toml'), raw, 0o644)
    for binary in selected - {name for name in selected if '/' in name}:
        publish(destination / binary, payload[binary], 0o755)
        C.command(str(destination / binary), '--help')
    for role, (binary, _, _) in ROLES.items():
        C.command(str(destination / binary), '--config', str(destination / (role + '.toml')), '--check')
    require_uninstalled()
    if runtime != check_identities():
        raise ValueError('runtime NSS identities changed during preparation')
    # Persistent masks are established BEFORE unit/program/config publication.
    # No enable/start command exists in this installer.
    for role in ROLES:
        (MASKS / (role + '.service')).symlink_to('/dev/null')
    C.TX.sync_directory(MASKS)
    C.command('/usr/bin/systemctl', 'daemon-reload')
    check_masked()
    for parent in reversed([UNITS, *UNITS.parents]):
        if not parent.exists():
            C.protected(parent.parent, directory=True)
            parent.mkdir(mode=0o755)
            parent.chmod(0o755)
            C.TX.sync_directory(parent.parent)
    for parent in [BIN, ETC, STATE, UNITS]:
        C.protected(parent, directory=True)
    for binary in selected - {name for name in selected if '/' in name}:
        publish(BIN / binary, payload[binary], 0o755)
    installed = {}
    for role, (binary, _, journal) in ROLES.items():
        config_dir, state = ETC / role, STATE / role
        config_dir.mkdir(mode=0o755)
        config_dir.chmod(0o755)
        publish(config_dir / 'config.toml', raw_configs[role], 0o644)
        state.mkdir(mode=0o700)
        os.chown(state, runtime[role]['uid'], runtime[role]['gid'])
        C.TX.sync_directory(ETC)
        C.TX.sync_directory(STATE)
        publish(UNITS / (role + '.service'), payload[f'deploy/systemd/{role}.service.example'], 0o644)
        C.command('/usr/bin/setpriv', '--reuid=' + str(runtime[role]['uid']), '--regid=' + str(runtime[role]['gid']),
                  '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
                  str(BIN / binary), '--config', str(config_dir / 'config.toml'), '--initialize-state', timeout=300)
        info = (state / journal).lstat()
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_uid != runtime[role]['uid']
                or info.st_gid != runtime[role]['gid'] or stat.S_IMODE(info.st_mode) != 0o600):
            raise ValueError('initialized journal has unexpected private ownership')
        installed[role] = {'runtime': runtime[role], 'journal': C.TX.fingerprint(state / journal),
                           'config_sha256': hashlib.sha256(raw_configs[role]).hexdigest(),
                           'binary_sha256': hashlib.sha256(payload[binary]).hexdigest()}
    C.command('/usr/bin/systemctl', 'daemon-reload')
    check_masked()
    C.write_json(destination / 'INSTALLED.json', {'version': 1, 'tool': verified_tool_identity(), 'review': reviewed,
                                                'archive_sha256': digest, 'core_evidence_id': accepted['id'],
                                                'roles': installed, 'services_started': False, 'credentials_loaded': False})
    (destination / 'INCOMPLETE').unlink()
    C.TX.sync_directory(destination)
    return {'installed': True, 'roles': 3, 'services_masked': True, 'services_started': False, 'credentials_loaded': False}


def check_masked():
    for role in ROLES:
        path = MASKS / (role + '.service')
        value = C.status(role + '.service')
        if (not path.is_symlink() or os.readlink(path) != '/dev/null' or value['LoadState'] != 'masked'
                or value['ActiveState'] != 'inactive' or value['SubState'] != 'dead'
                or value['MainPID'] != '0' or value['UnitFileState'] != 'masked' or value['NeedDaemonReload'] != 'no'):
            raise ValueError('every new role must remain persistently masked/inactive')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    accounts = commands.add_parser('identities')
    accounts.add_argument('--create-identities', action='store_true', required=True)
    install_command = commands.add_parser('install')
    for name in ('archive', 'config_source', 'policy', 'bundle', 'destination'):
        install_command.add_argument(name, type=Path)
    install_command.add_argument('--source', required=True)
    install_command.add_argument('--sha256', required=True)
    install_command.add_argument('--install-disabled', action='store_true', required=True)
    args = parser.parse_args()
    if args.operation == 'identities':
        result = identities(args.create_identities)
    else:
        result = install(args.archive, args.source, args.sha256, args.config_source, args.policy, args.bundle, args.destination, args.install_disabled)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
