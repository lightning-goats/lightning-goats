#!/usr/bin/env python3
"""Freeze an overlay-only CyberHerd stage for review; never apply or activate it.

Uses the authenticated root closure and a separately pinned native configuration
validator. Existing services, configuration, journals, identities and secrets are
unchanged. A later reviewed maintenance transaction is still required.
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
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import socket
import tomllib


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


Q = helper('overlay_inactive_baseline', 'qualify-cyberherd-installed.py')
C = Q.C
C.verified_tool_identity = lambda: verified_tool_identity()
W = helper('overlay_configuration_rendering', 'qualify-cyberherd-workers.py')
SOCKET = Path('/run/lightning-goats-bridge/core.sock')
TMPFILE = Path('/etc/tmpfiles.d/lightning-goats-cyberherd-bridge.conf')
MAINTENANCE = Path('/etc/cyberherd/state-maintenance.toml')
USERS = {'herd', 'dexter', 'rowan', 'cosmo', 'newton', 'nova'}


def role_dropins():
    """Pin existing role-specific overrides, including those hidden by masks."""
    result = {}
    for role in Q.ROLES:
        for parent in (Path('/etc/systemd/system'), Path('/run/systemd/system'), Q.UNITS, Path('/usr/lib/systemd/system')):
            directory = parent / (role + '.service.d')
            if not directory.exists() and not directory.is_symlink():
                continue
            C.protected(directory, directory=True)
            for path in directory.iterdir():
                if path.name.endswith('.conf'):
                    C.protected(path)
                    result[str(path)] = C.TX.fingerprint(path)
    return result


def render(core, member, peer_uid, group_gid, message_start_after):
    """Only the native receive/zero-budget bridge and presentation may change."""
    if (type(message_start_after) is not int or message_start_after < 0
            or type(peer_uid) is not int or peer_uid <= 0
            or type(group_gid) is not int or group_gid <= 0
            or core.get('core_bridge') is not None or core['lnurl'].get('zap') is not None
            or member['enabled'] is not False or member['core_socket'] != str(SOCKET)
            or member['core']['core_uid'] == peer_uid):
        raise ValueError('requires the original receive core and separate inactive intake')
    payees = member['core']['payees']
    registry = {entry['user'] for entry in core['lightning_address']}
    if set(payees) != USERS or registry != USERS:
        raise ValueError('requires the exact six existing native receive payees')
    owner = member['project_pubkey']
    if core['nostr']['bunker_pubkey'] != owner:
        raise ValueError('receipt signer and membership project differ')
    for key in (owner, *payees.values()):
        if not isinstance(key, str) or re.fullmatch('[0-9a-f]{64}', key) is None:
            raise ValueError('invalid existing public identity')
    candidate_core = copy.deepcopy(core)
    candidate_core['lnurl']['zap'] = {'receipt_pubkey': owner, 'payee_pubkeys': copy.deepcopy(payees),
                                     'alias_origins': ['https://lnb.bolverker.com/']}
    candidate_core['core_bridge'] = {
        'socket': str(SOCKET), 'peer_uid': peer_uid, 'group_gid': group_gid,
        'payout_execution_enabled': False, 'delegated_feeder_messages': False,
        'reward_policy': {'owner': owner, 'member_percent_ceiling': 0,
                          'max_sats_per_feed': 0, 'max_sats_per_24h': 0},
    }
    candidate_member = copy.deepcopy(member)
    candidate_member.update(enabled=True, messaging={
        'enabled': True, 'start_after': message_start_after, 'poll_millis': 1000,
        'relays': copy.deepcopy(member['relays']), 'lifecycle': False},
        view={'enabled': True, 'listen': '127.0.0.1:3340', 'overlay': True})
    # These optional capabilities are not part of this stage. Original policy
    # remains in the retained configuration; no journal/cutoff is reset.
    candidate_member.pop('rewards', None)
    candidate_member.pop('payout_bridge', None)
    return candidate_core, candidate_member


def check_native_report(value):
    expected = {'accepted': True, 'configuration_only': True, 'ledger_opened': False,
                'credentials_loaded': False, 'provider_operations': 0,
                'services_changed': False, 'payout_capabilities_enabled': False}
    if (not isinstance(value, dict) or set(value) != set(expected)
            or any(type(value[k]) is not type(v) or value[k] != v for k, v in expected.items())):
        raise ValueError('native validator did not positively accept the zero-budget stage')
    return value


def baseline(installation, validator, validator_sha):
    dropins = role_dropins()
    original = Q.baseline(installation, masked_dropins=dropins)
    core_user = C.pwd.getpwnam('lightning-goats')
    core_groups = {C.grp.getgrgid(gid).gr_name for gid in os.getgrouplist(core_user.pw_name, core_user.pw_gid)}
    if (original['core']['properties']['User'] != core_user.pw_name
            or original['core']['properties']['ActiveState'] != 'active'
            or original['core']['properties']['SubState'] != 'running'
            or core_user.pw_uid == 0 or core_user.pw_gid == 0 or core_user.pw_shell != '/usr/sbin/nologin'
            or core_groups & {'root', 'sudo', 'wheel', 'admin', 'docker', 'lxd', 'disk'}):
        raise ValueError('original non-admin receive core must be running')
    core = tomllib.loads(C.CONFIG.read_text())
    member = tomllib.loads((Q.ETC / 'cyberherd/config.toml').read_text())
    if (core['database']['url'] != 'sqlite://' + str(C.DATABASE)
            or member['core']['core_uid'] != core_user.pw_uid):
        raise ValueError('original core store or peer identity differs')
    if not re.fullmatch('[0-9a-f]{64}', validator_sha):
        raise ValueError('independent native validator SHA256 required')
    C.protected(validator)
    validator_info = C.TX.fingerprint(validator)
    if (validator_info['mode'] != 0o755 or validator_info['sha256'] != validator_sha
            or validator.stat().st_size > 64 * 1024 * 1024):
        raise ValueError('native validator identity differs')
    C.protected(MAINTENANCE, private=True)
    policy = tomllib.loads(MAINTENANCE.read_text())
    components = policy['components']
    if (policy['owner'] != member['project_pubkey'] or policy['epoch'] != member['core']['epoch']
            or components['core'] != {'database': str(C.DATABASE), 'uid': core_user.pw_uid,
                                      'gid': core_user.pw_gid, 'unit': C.UNIT_NAME}):
        raise ValueError('maintenance policy does not preserve the existing core binding')
    payout = tomllib.loads((Q.ETC / 'cyberherd-payout/config.toml').read_text())
    if policy['provider'] != payout['policy']['scope']['provider']:
        raise ValueError('maintenance provider scope differs from the inactive payout journal')
    for role, component in [('cyberherd', 'membership'), ('cyberherd-payout', 'payout'), ('cyberherd-signer', 'signer')]:
        database = Q.ROLES[role][1]
        identity = original['runtime'][role]
        if components[component] != {'database': str(Q.STATE / role / database), **identity, 'unit': role + '.service'}:
            raise ValueError('maintenance journal/identity scope differs')
    # Read protected program identities, never parse the live financial store.
    programs = {}
    for name in ('lightning-goatsd', 'lightning-goatsctl', 'cyberherd-statectl'):
        program = Q.BIN / name
        C.protected(program)
        programs[name] = C.TX.fingerprint(program)
    pid = original['core']['properties']['MainPID']
    if not pid.isdecimal() or int(pid) < 2 or hashlib.sha256(Path('/proc', pid, 'exe').read_bytes()).hexdigest() != programs['lightning-goatsd']['sha256']:
        raise ValueError('running core is not the protected installed program')
    C.command(str(Q.BIN / 'cyberherd-statectl'), '--config', str(MAINTENANCE), 'check')
    if role_dropins() != dropins:
        raise ValueError('role unit overrides changed during baseline inspection')
    return {'installation': original, 'core_programs': programs,
            'validator': validator_info, 'maintenance': C.TX.fingerprint(MAINTENANCE),
            'role_dropins': dropins}, core, member


def prepare(installation, validator, validator_sha, destination, message_start_after):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    if destination.parent != C.BASE or type(message_start_after) is not int or message_start_after < 0:
        raise ValueError('new protected review destination and explicit nonnegative message cutoff required')
    original, core, member = baseline(installation, validator, validator_sha)
    group = C.grp.getgrnam('cyberherd-bridge').gr_gid
    if group == C.pwd.getpwnam('lightning-goats').pw_gid:
        raise ValueError('core primary credential group cannot be the bridge group')
    new_core, new_member = render(core, member, original['installation']['runtime']['cyberherd']['uid'], group, message_start_after)
    for path in (SOCKET.parent, TMPFILE):
        if path.exists() or path.is_symlink():
            raise ValueError('new bridge runtime/tmpfiles destination already exists')
    C.protected(TMPFILE.parent, directory=True)
    # Fail before staging if the optional loopback view is already occupied.
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(('127.0.0.1', 3340))
    unit = C.UNIT.read_text()
    candidate_unit = unit + '\n[Service]\nSupplementaryGroups=cyberherd-bridge\nReadWritePaths=' + str(SOCKET.parent) + '\n'
    tmpfiles = f'd {SOCKET.parent} 0750 lightning-goats cyberherd-bridge -\n'
    destination.mkdir(mode=0o700)
    C.TX.sync_directory(destination.parent)
    C.write_json(destination / 'INCOMPLETE', {'prepared': False, 'services_changed': False})
    candidates = {'core.toml': W.toml_text(new_core), 'membership.toml': W.toml_text(new_member),
                  C.UNIT.name: candidate_unit, 'bridge.tmpfiles': tmpfiles}
    for name, text in candidates.items():
        path = destination / name
        with path.open('x') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(text); stream.flush(); os.fsync(stream.fileno())
    accepted = check_native_report(json.loads(C.command(str(validator), str(destination / 'core.toml'),
                                                       str(destination / 'membership.toml'))))
    C.command('/usr/bin/systemd-analyze', 'verify', str(destination / C.UNIT.name))
    files = {**original['installation']['core']['files'],
             str(Q.ETC / 'cyberherd/config.toml'): C.TX.fingerprint(Q.ETC / 'cyberherd/config.toml'),
             str(MAINTENANCE): original['maintenance']}
    for index, (name, fingerprint) in enumerate(files.items()):
        C.TX.copy_durable(Path(name), destination / ('original-' + str(index)), fingerprint)
    C.TX.prepare(destination / 'config-files', [
        (C.CONFIG, destination / 'core.toml', files[str(C.CONFIG)]),
        (Q.ETC / 'cyberherd/config.toml', destination / 'membership.toml', files[str(Q.ETC / 'cyberherd/config.toml')]),
    ], lambda: require_baseline(installation, validator, validator_sha, original))
    require_baseline(installation, validator, validator_sha, original)
    C.write_json(destination / 'PREPARED.json', {
        'version': 1, 'tool': verified_tool_identity(), 'baseline': original,
        'installation': str(installation), 'validator': str(validator), 'validator_sha256': validator_sha,
        'message_start_after': message_start_after,
        'candidate_sha256': {name: hashlib.sha256(text.encode()).hexdigest() for name, text in candidates.items()},
        'original_order': list(files), 'original_files': files, 'native_validation': accepted,
        'services_changed': False, 'journals_parsed': False, 'live_core_journal_opened': False,
        'role_journals_hashed_only': True, 'activation_supported': False,
    })
    (destination / 'INCOMPLETE').unlink(); C.TX.sync_directory(destination)
    return {'prepared': True, 'native_configuration_validated': True, 'services_changed': False,
            'journals_parsed': False, 'live_core_journal_opened': False,
            'credentials_loaded': False, 'activation_supported': False}


def require_baseline(installation, validator, validator_sha, expected):
    current, _, _ = baseline(installation, validator, validator_sha)
    if current != expected:
        raise ValueError('installation changed during review preparation')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    command = commands.add_parser('prepare')
    for name in ('installation', 'validator', 'destination'):
        command.add_argument(name, type=Path)
    command.add_argument('--validator-sha256', required=True)
    command.add_argument('--message-start-after', type=int, required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(args.installation, args.validator, args.validator_sha256,
                             args.destination, args.message_start_after), sort_keys=True))


if __name__ == '__main__':
    main()
