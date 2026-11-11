#!/usr/bin/env python3
"""Freeze a current four-journal core upgrade for review; no apply or activation.

Authenticated root-owned launcher only. Preparation reads protected configuration,
program and service identities, stages a source-pinned release and preserves the
three original programs. It never opens a journal, loads a credential, stops a
service, changes configuration, migrates state or enables contact payments.
Explicit --current-schema preparation may retain existing contact configuration;
its records cannot be consumed by the schema-upgrade maintenance path.
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
import os
from pathlib import Path
import re
import tarfile
import tempfile
import tomllib


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


S = helper('contact_upgrade_stage_support', 'stage-cyberherd-overlay.py')
Q, C, TX = S.Q, S.C, S.C.TX
C.verified_tool_identity = lambda: verified_tool_identity()
UPGRADE_KIND = 'contact'
TARGET_NAMES = ('lightning-goatsd', 'lightning-goatsctl', 'cyberherd-statectl')
ROLE_NAMES = {'core': 'lightning-goats-receive', 'membership': 'cyberherd',
              'payout': 'cyberherd-payout', 'signer': 'cyberherd-signer'}


def configuration_boundary(core, member, payout, signer, policy, *, current_schema=False):
    """Preserve the existing overlay stage, zero budgets and public identities."""
    if UPGRADE_KIND == 'feeder-cap' and (current_schema or core.get('service', {}).get('mode') != 'shadow'
            or core.get('contact') is not None):
        raise ValueError('feeder-cap upgrade requires the explicitly contained shadow service without contact')
    bridge = core.get('core_bridge', {})
    reward = bridge.get('reward_policy', {})
    messages = member.get('messaging', {})
    if ((not current_schema and core.get('contact') is not None)
            or core['database']['url'] != 'sqlite://' + str(C.DATABASE)
            or member.get('enabled') is not True or member.get('rewards') is not None
            or member.get('payout_bridge') is not None or messages.get('signer') is not None
            or messages.get('lifecycle') is not False
            or bridge.get('payout_execution_enabled') is not False
            or bridge.get('delegated_feeder_messages') is not False
            or payout.get('enabled') is not False or signer.get('enabled') is not False
            or payout['policy'].get('execution_enabled') is not False):
        raise ValueError('requires the unchanged overlay-only stage with contact and payouts disabled')
    limits = [(reward, name) for name in ('member_percent_ceiling', 'max_sats_per_feed', 'max_sats_per_24h')]
    limits += [(payout['policy'], name) for name in ('member_percent_ceiling', 'max_sats_per_feed', 'max_sats_per_24h', 'max_fee_sats')]
    limits += [({'max_net_sats': payout['policy'].get('max_net_sats', 0)}, 'max_net_sats')]
    limits += [(signer['policy'], name) for name in ('max_net_sats', 'max_new_per_hour')]
    if any(type(section.get(name)) is not int or section[name] != 0 for section, name in limits):
        raise ValueError('all existing financial and signing ceilings must remain zero')
    owner = policy['owner']
    if (not re.fullmatch('[0-9a-f]{64}', owner)
            or core['nostr']['bunker_pubkey'] != owner or member['project_pubkey'] != owner
            or reward.get('owner') != owner or member['core']['epoch'] != policy['epoch']
            or member['core']['core_uid'] != policy['components']['core']['uid']
            or payout['policy']['scope']['epoch'] != policy['epoch']
            or payout['policy']['scope']['owner'] != owner
            or payout['policy']['scope']['provider'] != policy['provider']):
        raise ValueError('original owner, epoch, peer or provider scope differs')
    if current_schema and core.get('contact') is not None:
        contact = core['contact']
        if (not isinstance(contact, dict) or set(contact) != {'recipient', 'inbox_relays'}
                or contact['recipient'] != owner or not isinstance(contact['inbox_relays'], list)
                or not 1 <= len(contact['inbox_relays']) <= 8
                or any(not isinstance(value, str) or not value.startswith('wss://')
                       for value in contact['inbox_relays'])):
            raise ValueError('current contact identity and bounded inbox relays required')


def baseline(*, current_schema=False):
    C.protected(S.MAINTENANCE, private=True)
    runtime = Q.I.check_identities()
    core_user = C.pwd.getpwnam('lightning-goats')
    groups = {C.grp.getgrgid(gid).gr_name for gid in os.getgrouplist(core_user.pw_name, core_user.pw_gid)}
    if (core_user.pw_uid == 0 or core_user.pw_gid == 0 or core_user.pw_shell != '/usr/sbin/nologin'
            or groups & {'root', 'sudo', 'wheel', 'admin', 'docker', 'lxd', 'disk'}):
        raise ValueError('original non-admin receive identity required')
    Q.I.require_no_sudo('lightning-goats')
    identities = {'core': {'uid': core_user.pw_uid, 'gid': core_user.pw_gid},
                  **{role: runtime[name] for role, name in ROLE_NAMES.items() if role != 'core'}}
    configs = {'core': C.CONFIG, **{role: Q.ETC / name / 'config.toml'
                                  for role, name in ROLE_NAMES.items() if role != 'core'}}
    programs = {name: Q.BIN / name for name in (*TARGET_NAMES, *(v[0] for v in Q.ROLES.values()))}
    files = {}
    for path in [S.MAINTENANCE, *configs.values(), *programs.values()]:
        C.protected(path)
        files[str(path)] = TX.fingerprint(path)
        if path in programs.values() and files[str(path)]['mode'] != 0o755:
            raise ValueError('installed programs require protected executable permissions')
    policy = tomllib.loads(S.MAINTENANCE.read_text())
    values = {role: tomllib.loads(path.read_text()) for role, path in configs.items()}
    configuration_boundary(*(values[role] for role in ROLE_NAMES), policy, current_schema=current_schema)
    if set(policy['components']) != set(ROLE_NAMES):
        raise ValueError('complete four-journal policy required')
    states = {}
    for role, name in ROLE_NAMES.items():
        component = policy['components'][role]
        expected_database = C.DATABASE if role == 'core' else Q.STATE / name / Q.ROLES[name][1]
        if component != {'database': str(expected_database), **identities[role], 'unit': name + '.service'}:
            raise ValueError('original four-journal path, unit or owner differs')
        value = C.status(name + '.service')
        states[role] = value
        if role in ('core', 'membership'):
            user = 'lightning-goats' if role == 'core' else name
            if (value['LoadState'] != 'loaded' or value['ActiveState'] != 'active'
                    or value['SubState'] != 'running' or value['NeedDaemonReload'] != 'no'
                    or not value['MainPID'].isdecimal() or int(value['MainPID']) < 2
                    or value['User'] != user or value['Group'] not in ('', C.grp.getgrgid(identities[role]['gid']).gr_name)):
                raise ValueError('existing core and membership must be running under their original identities')
            program = programs['lightning-goatsd' if role == 'core' else 'cyberherdd']
            if hashlib.sha256(Path('/proc', value['MainPID'], 'exe').read_bytes()).hexdigest() != files[str(program)]['sha256']:
                raise ValueError('running process differs from the protected installed program')
            paths = [Path(value['FragmentPath']), *(Path(v) for v in value['DropInPaths'].split())]
        else:
            mask = Path('/etc/systemd/system') / (name + '.service')
            if (value['LoadState'] != 'masked' or value['ActiveState'] != 'inactive'
                    or value['SubState'] != 'dead' or value['MainPID'] != '0'
                    or value['UnitFileState'] != 'masked' or not mask.is_symlink()
                    or os.readlink(mask) != '/dev/null'):
                raise ValueError('outgoing roles must remain persistently masked and stopped')
            paths = [Q.UNITS / (name + '.service')]
        for path in paths:
            C.protected(path)
            files[str(path)] = TX.fingerprint(path)
    # Hidden role-specific overrides are part of the frozen inventory as well.
    dropins = S.role_dropins()
    files.update(dropins)
    Q.no_role_processes({role: identities[role] for role in ('payout', 'signer')})
    return {'identities': identities, 'states': states, 'files': files, 'policy': policy}


def stage_archive(archive, staged, digest):
    if re.fullmatch('[0-9a-f]{64}', digest) is None:
        raise ValueError('independent archive digest required')
    with C.PREFLIGHT.regular_archive(archive) as incoming, staged.open('xb') as outgoing:
        os.fchmod(outgoing.fileno(), 0o600)
        total, actual = 0, hashlib.sha256()
        while chunk := incoming.read(1024 * 1024):
            total += len(chunk)
            if total > C.PREFLIGHT.MAX_ARCHIVE_BYTES:
                raise ValueError('archive exceeds bound')
            actual.update(chunk)
            outgoing.write(chunk)
        outgoing.flush()
        os.fsync(outgoing.fileno())
    if actual.hexdigest() != digest:
        raise ValueError('independent archive digest differs')


def runtime_help(program, component):
    """Prove actual UID/netns execution before any maintenance or journal open."""
    C.protected(Path('/run'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-contact-help-', dir='/run'))
    carrier.chmod(0o711)
    try:
        target = carrier / 'lightning-goatsctl'
        TX.copy_durable(program, target, TX.fingerprint(program))
        C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
                  '--reuid=' + str(component['uid']), '--regid=' + str(component['gid']),
                  '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all',
                  '--no-new-privs', str(target), '--help', timeout=30)
        return str(carrier)
    finally:
        carrier.chmod(0o700)
        TX.sync_directory(carrier)


def prepare(archive, review_archive, source, digest, review_digest, directory, *, current_schema=False):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    if (directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None
            or re.fullmatch('[0-9a-f]{40}', source) is None):
        raise ValueError('new direct protected upgrade-base child and full source required')
    original = baseline(current_schema=current_schema)
    contact_enabled = current_schema and tomllib.loads(C.CONFIG.read_text()).get('contact') is not None
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    staged = directory / 'release.tar.gz'
    stage_archive(archive, staged, digest)
    release = C.PREFLIGHT.preflight(staged, source, digest)
    payload = directory / 'payload'
    payload.mkdir(mode=0o700)
    C.PREFLIGHT.RELEASE.verify_archive(staged, source, payload)
    review = directory / 'review.tar.gz'
    stage_archive(review_archive, review, review_digest)
    Q.I.PACKAGE.verify(review, source)
    selected = None
    with tarfile.open(review, 'r:gz') as reader:
        for member in reader:
            if str(Path(member.name)) == 'cyberherd-statectl':
                selected = reader.extractfile(member).read()
    if selected is None or (payload / 'cyberherd-statectl').exists():
        raise ValueError('native verifier is missing or its archives overlap')
    # The review archive was fully verified before selecting these program bytes.
    Q.I.publish(payload / 'cyberherd-statectl', selected, 0o755)
    selected_hashes = {name: TX.fingerprint(payload / name)['sha256'] for name in TARGET_NAMES}
    for name in TARGET_NAMES:
        C.command(str(payload / name), '--help')
    C.command(str(payload / 'cyberherd-statectl'), '--config', str(S.MAINTENANCE), 'check')
    carrier = runtime_help(payload / 'lightning-goatsctl', original['identities']['core'])

    def unchanged():
        if baseline(current_schema=current_schema) != original:
            raise ValueError('original installation or process identity changed during preparation')

    for index, path in enumerate(original['files']):
        TX.copy_durable(path, directory / f'original-{index}', original['files'][path])
    TX.prepare(directory / 'programs', [
        (Q.BIN / name, payload / name, original['files'][str(Q.BIN / name)])
        for name in TARGET_NAMES
    ], unchanged)
    unchanged()
    C.write_json(directory / 'PREPARED.json', {
        'version': 1, 'tool': verified_tool_identity(), 'release': release,
        'core_upgrade': UPGRADE_KIND,
        'review_archive_sha256': review_digest, 'selected_program_sha256': selected_hashes,
        'baseline': original, 'original_order': list(original['files']),
        'program_manifest_sha256': TX.fingerprint(directory / 'programs/TRANSACTION.json')['sha256'],
        'activation_supported': False, 'services_changed': False, 'journals_opened': False,
        'credentials_loaded': False, 'contact_enabled': contact_enabled,
        **({'current_schema_patch': True, 'schema_upgrade_authorized': False} if current_schema else {}),
        'native_runtime_help_verified': True, 'native_runtime_help_carrier': carrier,
    })
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'activation_supported': False, 'services_changed': False,
            'journals_opened': False, 'credentials_loaded': False, 'contact_enabled': contact_enabled}


def main():
    global UPGRADE_KIND
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--feeder-cap', action='store_true', help='exact contained schema-24 to 25 feeder-cap upgrade')
    parser.add_argument('--current-schema', action='store_true', help='program-only patch; preserve current schema and configuration')
    for name in ('archive', 'review_archive', 'source', 'sha256', 'review_sha256', 'directory'):
        parser.add_argument(name, type=Path if name in ('archive', 'review_archive', 'directory') else str)
    args = parser.parse_args()
    if args.feeder_cap:
        if args.current_schema: raise ValueError('upgrade kinds cannot be combined')
        UPGRADE_KIND = 'feeder-cap'
    result = prepare(args.archive, args.review_archive, args.source, args.sha256, args.review_sha256,
                     args.directory, current_schema=args.current_schema)
    print(__import__('json').dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
