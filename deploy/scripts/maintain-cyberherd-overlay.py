#!/usr/bin/env python3
"""Explicit maintenance for a sealed overlay stage; never restore financial state.

Plan is read-only outside a fresh private transaction. Apply requires exclusive
maintenance, masks all writers and verifies a native four-journal backup before
configuration replacement. Resume is a separate explicit operator action.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
from contextlib import contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import time
import tomllib
import uuid


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


S = helper('overlay_review_support', 'stage-cyberherd-overlay.py')
C, Q, TX = S.C, S.Q, S.C.TX
C.verified_tool_identity = lambda: verified_tool_identity()
MASKS = Path('/etc/systemd/system')


def digest(path):
    return TX.fingerprint(path)['sha256']


def private_directory(path):
    C.protected(path, directory=True, private=True)
    if path.parent != C.BASE or re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', path.name) is None:
        raise ValueError('transaction must be a direct protected upgrade-base child')


def read_review(directory, expected):
    private_directory(directory)
    if re.fullmatch('[0-9a-f]{64}', expected) is None or digest(directory / 'PREPARED.json') != expected:
        raise ValueError('independently reviewed preparation digest required')
    r = C.read_json(directory / 'PREPARED.json')
    if (r['version'] != 1 or r['activation_supported'] is not False
            or r['services_changed'] is not False or r['live_core_journal_opened'] is not False
            or (directory / 'INCOMPLETE').exists()):
        raise ValueError('accepted preparation-only review required')
    S.check_native_report(r['native_validation'])
    names = {'core.toml', 'membership.toml', C.UNIT.name, 'bridge.tmpfiles'}
    if set(r['candidate_sha256']) != names:
        raise ValueError('unexpected stage candidate set')
    for name in names:
        C.protected(directory / name, private=True)
        if digest(directory / name) != r['candidate_sha256'][name]:
            raise ValueError('review candidate changed')
    expected_files = {**r['baseline']['installation']['core']['files'],
                      str(Q.ETC / 'cyberherd/config.toml'): r['original_files'][str(Q.ETC / 'cyberherd/config.toml')],
                      str(S.MAINTENANCE): r['baseline']['maintenance']}
    if (r['original_files'] != expected_files or len(r['original_order']) != len(expected_files)
            or set(r['original_order']) != set(expected_files)):
        raise ValueError('original review inventory differs')
    for i, path in enumerate(r['original_order']):
        C.protected(directory / f'original-{i}')
        if TX.fingerprint(directory / f'original-{i}') != expected_files[path]:
            raise ValueError('original review backup changed')
    return r


def original_path(directory, r, path):
    return directory / ('original-' + str(r['original_order'].index(str(path))))


def stable(r):
    """Fixed identities/programs/unused capabilities; no database opens."""
    base = r['baseline']
    if S.role_dropins() != base.get('role_dropins', {}):
        raise ValueError('role-specific unit overrides changed')
    S.global_dropins(base['installation']['core'])
    C.protected(MASKS, directory=True)
    if Q.I.check_identities() != base['installation']['runtime']:
        raise ValueError('runtime identity changed')
    core = C.pwd.getpwnam('lightning-goats')
    policy = tomllib.loads(S.MAINTENANCE.read_text())
    groups = {C.grp.getgrgid(gid).gr_name for gid in os.getgrouplist(core.pw_name, core.pw_gid)}
    if (core.pw_uid != policy['components']['core']['uid'] or core.pw_gid != policy['components']['core']['gid']
            or core.pw_uid == 0 or core.pw_gid == 0 or core.pw_shell != '/usr/sbin/nologin'
            or groups & {'root', 'sudo', 'wheel', 'admin', 'docker', 'lxd', 'disk'}):
        raise ValueError('original non-admin core identity changed')
    Q.I.require_no_sudo('lightning-goats')
    for name, fp in base['core_programs'].items():
        C.protected(Q.BIN / name)
        if TX.fingerprint(Q.BIN / name) != fp:
            raise ValueError('protected core program changed')
    for role, fp in base['installation']['program'].items():
        program = Q.BIN / Q.ROLES[role][0]
        C.protected(program)
        if TX.fingerprint(program) != fp:
            raise ValueError('protected role program changed')
        unit = Q.UNITS / (role + '.service')
        C.protected(unit)
        if unit.read_text() != base['installation']['units'][role]:
            raise ValueError('protected role unit changed')
        if role != 'cyberherd':
            config = Q.ETC / role / 'config.toml'
            C.protected(config)
            if digest(config) != base['installation']['configuration'][role]:
                raise ValueError('outgoing role configuration changed')
    for path in [S.MAINTENANCE, Path(r['validator'])]:
        C.protected(path)
    if TX.fingerprint(S.MAINTENANCE) != base['maintenance'] or TX.fingerprint(r['validator']) != base['validator']:
        raise ValueError('protected policy/validator changed')
    for path, fp in base['installation']['core']['files'].items():
        if path not in (str(C.UNIT), str(C.CONFIG)):
            C.protected(Path(path))
            if TX.fingerprint(path) != fp:
                raise ValueError('original core drop-in changed')


def validate_review(directory, r):
    stable(r)
    core = tomllib.loads(original_path(directory, r, C.CONFIG).read_text())
    member = tomllib.loads(original_path(directory, r, Q.ETC / 'cyberherd/config.toml').read_text())
    new_core, new_member = S.render(core, member, r['baseline']['installation']['runtime']['cyberherd']['uid'],
                                    C.grp.getgrnam('cyberherd-bridge').gr_gid, r['message_start_after'])
    if (tomllib.loads((directory / 'core.toml').read_text()) != new_core
            or tomllib.loads((directory / 'membership.toml').read_text()) != new_member):
        raise ValueError('candidate capabilities differ from the reviewed rendering')
    expected_unit = original_path(directory, r, C.UNIT).read_text() + '\n[Service]\nSupplementaryGroups=cyberherd-bridge\nReadWritePaths=' + str(S.SOCKET.parent) + '\n'
    expected_tmp = f'd {S.SOCKET.parent} 0750 lightning-goats cyberherd-bridge -\n'
    if (directory / C.UNIT.name).read_text() != expected_unit or (directory / 'bridge.tmpfiles').read_text() != expected_tmp:
        raise ValueError('unit/runtime candidate differs')
    S.check_native_report(json.loads(C.command(r['validator'], str(directory / 'core.toml'), str(directory / 'membership.toml'))))


def plan(review, review_sha, directory):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    if directory.parent != C.BASE:
        raise ValueError('new private execution directory required')
    r = read_review(review, review_sha)
    validate_review(review, r)
    fresh = lambda: require_fresh(r)
    fresh()
    if any(p.exists() or p.is_symlink() for p in (S.SOCKET.parent, S.TMPFILE)):
        raise ValueError('runtime/tmpfiles destination is no longer unused')
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    rows = TX.prepare(directory / 'configs', [
        (C.CONFIG, review / 'core.toml', r['original_files'][str(C.CONFIG)]),
        (Q.ETC / 'cyberherd/config.toml', review / 'membership.toml', r['original_files'][str(Q.ETC / 'cyberherd/config.toml')]),
    ], fresh)
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    fresh()
    C.write_json(directory / 'EXECUTION.json', {'version': 1, 'tool': verified_tool_identity(),
        'review': str(review), 'review_sha256': review_sha, 'configs': rows,
        'config_manifest_sha256': digest(directory / 'configs/TRANSACTION.json')})
    (directory / 'INCOMPLETE').unlink(); TX.sync_directory(directory)
    return {'planned': True, 'services_changed': False, 'maintenance_approved': False}


def require_fresh(r):
    current, _, _ = S.baseline(Path(r['installation']), Path(r['validator']), r['validator_sha256'])
    # An older sealed review has no override inventory. Accept it unchanged only
    # when there are no role overrides at all; never manufacture its identity.
    if 'role_dropins' not in r['baseline'] and not current.get('role_dropins'):
        current.pop('role_dropins', None)
    if current != r['baseline']:
        raise ValueError('original running installation changed since preparation')


def load(directory):
    C.root()
    private_directory(directory)
    m = C.read_json(directory / 'EXECUTION.json')
    if m['version'] != 1 or m['tool'] != verified_tool_identity() or (directory / 'INCOMPLETE').exists():
        raise ValueError('different or incomplete executor generation')
    r = read_review(Path(m['review']), m['review_sha256'])
    validate_review(Path(m['review']), r)
    if digest(directory / 'configs/TRANSACTION.json') != m['config_manifest_sha256']:
        raise ValueError('configuration transaction changed')
    records = C.read_json(directory / 'configs/TRANSACTION.json')
    if records != {'version': 1, 'files': m['configs']} or [v['destination'] for v in m['configs']] != [str(C.CONFIG), str(Q.ETC / 'cyberherd/config.toml')]:
        raise ValueError('configuration destination set differs')
    for i, row in enumerate(m['configs']):
        name = ('core.toml', 'membership.toml')[i]
        if (row['old'] != r['original_files'][row['destination']]
                or row['new'] != dict(row['old'], sha256=r['candidate_sha256'][name])):
            raise ValueError('replacement no longer matches the sealed review')
        for version in ('old', 'new'):
            if TX.fingerprint(directory / 'configs' / f'{i}.{version}') != row[version]:
                raise ValueError('configuration replacement backup changed')
        if TX.fingerprint(row['destination']) not in (row['old'], row['new']):
            raise ValueError('installed configuration drift requires reconciliation')
    return m, r


@contextmanager
def exclusive(directory):
    # One host-wide maintenance lock, as well as this transaction's lock. This
    # excludes our executors, not an administrator; explicit exclusivity remains
    # an operator attestation for the complete maintenance interval.
    path = C.BASE / 'CYBERHERD-OVERLAY-MAINTENANCE.lock'
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    os.close(fd)
    C.protected(path, private=True)
    import fcntl
    fd = os.open(path, os.O_RDWR | os.O_NOFOLLOW)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with TX.locked(directory):
            yield
    finally:
        os.close(fd)


def note(directory, name, value):
    path = directory / name
    if path.exists():
        if C.read_json(path) != value:
            raise ValueError('conflicting maintenance receipt')
    else:
        C.write_json(path, value)


def masked(path):
    info = path.lstat()
    return stat.S_ISLNK(info.st_mode) and info.st_uid == info.st_gid == 0 and os.readlink(path) == '/dev/null'


def mask(path, allowed):
    C.protected(path.parent, directory=True)
    if path.is_symlink():
        if not masked(path):
            raise ValueError('unexpected unit alias')
        return
    C.protected(path)
    if TX.fingerprint(path) not in allowed:
        raise ValueError('custom unit changed before masking')
    with tempfile.TemporaryDirectory(prefix='.lg-overlay-mask-', dir=path.parent) as temporary:
        candidate = Path(temporary) / path.name
        candidate.symlink_to('/dev/null')
        os.replace(candidate, path); TX.sync_directory(path.parent)


def unit_versions(r):
    old = r['original_files'][str(C.UNIT)]
    return old, dict(old, sha256=r['candidate_sha256'][C.UNIT.name])


def stopped(r):
    stable(r)
    for name in (C.UNIT_NAME, *(role + '.service' for role in Q.ROLES)):
        value = C.status(name)
        pins = {**S.global_dropins(r['baseline']['installation']['core']),
                **{p: fp for p, fp in r['baseline'].get('role_dropins', {}).items() if Path(p).parent.name == name + '.d'}}
        if name == C.UNIT_NAME:
            pins.update({p: fp for p, fp in r['original_files'].items() if Path(p).parent.name == name + '.d'})
        if (value['Id'] != name or value['LoadState'] != 'masked' or value['ActiveState'] != 'inactive'
                or value['SubState'] != 'dead' or value['MainPID'] != '0'
                or value['UnitFileState'] != 'masked' or not Q.I.masked_reload_current(value, pins)
                or not masked(MASKS / name)):
            raise ValueError('all four writers must remain stopped and persistently masked')
        group = value['ControlGroup']
        if group:
            if not group.startswith('/') or any(p in ('.', '..') for p in group.split('/')):
                raise ValueError('unexpected writer cgroup')
            events = Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.events'
            if events.exists() and 'populated 0' not in events.read_text().splitlines():
                raise ValueError('writer cgroup remains populated')
    policy = tomllib.loads(S.MAINTENANCE.read_text())
    Q.no_role_processes(policy['components'])


def quiesce(r, rollback=False):
    # Intake is masked before normal apply. Rollback may stop a running intake;
    # outgoing roles are never unmasked, stopped-to-enable or started here.
    C.protected(MASKS, directory=True)
    for role in ('cyberherd-payout', 'cyberherd-signer'):
        if not masked(MASKS / (role + '.service')) or C.status(role + '.service')['MainPID'] != '0':
            raise ValueError('outgoing roles must remain masked without a process')
    member_mask = MASKS / 'cyberherd.service'
    if rollback:
        C.command('/usr/bin/systemctl', 'stop', '--', 'cyberherd.service', timeout=120)
        if not member_mask.exists() and not member_mask.is_symlink():
            member_mask.symlink_to('/dev/null'); TX.sync_directory(member_mask.parent)
    if not masked(member_mask):
        raise ValueError('intake must remain masked before configuration replacement')
    C.command('/usr/bin/systemctl', 'stop', '--', C.UNIT_NAME, timeout=120)
    mask(C.UNIT, unit_versions(r))
    C.command('/usr/bin/systemctl', 'daemon-reload')
    stopped(r)


def native_backup(directory, r):
    stopped(r)
    bundle = directory / 'backup'
    program = str(Q.BIN / 'cyberherd-statectl')
    if not bundle.exists():
        C.command(program, '--config', str(S.MAINTENANCE), 'snapshot', '--destination', str(bundle), '--writers-stopped', timeout=300)
    if (bundle / 'INCOMPLETE').exists():
        raise ValueError('incomplete backup retained; no automatic replacement')
    result = json.loads(C.command(program, '--config', str(S.MAINTENANCE), 'verify', '--bundle', str(bundle), timeout=300))
    if (result.get('accepted') is not True or result.get('resume_authorized') is not False
            or result.get('operation') != 'verify' or str(uuid.UUID(result['id'])) != result['id']):
        raise ValueError('native backup not positively accepted')
    manifest = C.read_json(bundle / 'MANIFEST.json')
    if manifest['id'] != result['id'] or manifest['source_policy_sha256'] != digest(S.MAINTENANCE):
        raise ValueError('native backup scope changed')
    policy = tomllib.loads(S.MAINTENANCE.read_text())
    images = {v['name']: v for v in manifest['images']}
    originals = {}
    for role, component in policy['components'].items():
        for suffix in ('', '-wal', '-shm'):
            path = Path(component['database'] + suffix)
            if not path.exists() and not path.is_symlink():
                if suffix != '-shm' and role + '.db' + suffix in images:
                    raise ValueError('original sidecar disappeared after backup')
                originals[str(path)] = None
                continue
            info = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600
                    or info.st_uid != component['uid'] or info.st_gid != component['gid']
                    or info.st_size > policy['max_component_bytes']):
                raise ValueError('original journal identity changed')
            fp = TX.fingerprint(path)
            if suffix != '-shm':
                image = images[role + '.db' + suffix]
                if fp['sha256'] != image['sha256'] or info.st_size != image['bytes']:
                    raise ValueError('original journal no longer matches the accepted backup')
            originals[str(path)] = fp
    stopped(r)
    note(directory, 'BACKUP-ACCEPTED.json', {'native': result, 'originals': originals,
                                           'manifest_sha256': digest(bundle / 'MANIFEST.json')})


def originals_unchanged(directory, r):
    stopped(r)
    record = C.read_json(directory / 'BACKUP-ACCEPTED.json')
    if digest(directory / 'backup/MANIFEST.json') != record['manifest_sha256']:
        raise ValueError('accepted backup manifest changed')
    for name, expected in record['originals'].items():
        path = Path(name)
        actual = TX.fingerprint(path) if path.exists() or path.is_symlink() else None
        if actual != expected:
            raise ValueError('journal changed during configuration-only maintenance')


def install_runtime(review, r):
    data = (review / 'bridge.tmpfiles').read_bytes()
    C.protected(S.TMPFILE.parent, directory=True)
    if S.TMPFILE.exists() or S.TMPFILE.is_symlink():
        C.protected(S.TMPFILE)
        if S.TMPFILE.read_bytes() != data or stat.S_IMODE(S.TMPFILE.stat().st_mode) != 0o644:
            raise ValueError('unexpected existing tmpfiles entry')
    else:
        with S.TMPFILE.open('xb') as out:
            os.fchmod(out.fileno(), 0o644); out.write(data); out.flush(); os.fsync(out.fileno())
        TX.sync_directory(S.TMPFILE.parent)
    # Created as a protected root directory first; a recorded partial create is
    # completed only while empty. Never repair an arbitrary existing directory.
    parent = S.SOCKET.parent
    uid = tomllib.loads(S.MAINTENANCE.read_text())['components']['core']['uid']
    gid = C.grp.getgrnam('cyberherd-bridge').gr_gid
    if not parent.exists() and not parent.is_symlink():
        parent.mkdir(mode=0o700); TX.sync_directory(parent.parent)
    info = parent.lstat()
    if (stat.S_ISDIR(info.st_mode) and (info.st_uid, info.st_gid) in ((0, 0), (uid, gid))
            and stat.S_IMODE(info.st_mode) == 0o700 and not list(parent.iterdir())):
        os.chown(parent, uid, gid); os.chmod(parent, 0o750); TX.sync_directory(parent)
    info = parent.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != uid or info.st_gid != gid or stat.S_IMODE(info.st_mode) != 0o750:
        raise ValueError('bridge runtime directory differs')
    if any(n.startswith('system.posix_acl') or n == 'security.capability' for n in os.listxattr(parent)):
        raise ValueError('unexpected bridge runtime access metadata')


def apply(directory, approved):
    if not approved:
        raise ValueError('explicit exclusive maintenance approval required')
    m, r = load(directory)
    with exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists() or (directory / 'ROLLED-BACK.json').exists():
            raise ValueError('resume/rollback already began; do not reapply')
        if not (directory / 'MAINTENANCE-STARTED.json').exists():
            require_fresh(r)
        note(directory, 'MAINTENANCE-STARTED.json', {'exclusive_maintenance': True, 'database_restore_authorized': False})
        quiesce(r)
        native_backup(directory, r)
        TX.execute(directory / 'configs', lambda: originals_unchanged(directory, r))
        install_runtime(Path(m['review']), r)
        originals_unchanged(directory, r)
        note(directory, 'APPLIED.json', {'configured': True, 'writers_masked': True, 'services_started': False})
    return {'configured': True, 'four_journal_backup_verified': True, 'services_started': False}


def restore_unit(source, expected):
    if not C.UNIT.is_symlink() and TX.fingerprint(C.UNIT) == expected:
        return
    if not masked(C.UNIT):
        raise ValueError('unit must be the retained mask before restoration')
    with tempfile.TemporaryDirectory(prefix='.lg-overlay-unit-', dir=C.UNIT.parent) as temporary:
        candidate = Path(temporary) / C.UNIT.name
        TX.copy_durable(source, candidate, expected)
        os.replace(candidate, C.UNIT); TX.sync_directory(C.UNIT.parent)


def running(name, user, program):
    # Type=simple's start job may finish before execve. Wait for this existing
    # process to become the pinned program; never issue another start/restart.
    expected = digest(program)
    deadline = time.monotonic() + 10
    while True:
        value = C.status(name)
        pid = value['MainPID']
        if value['User'] != user or value['NeedDaemonReload'] != 'no' or value['ActiveState'] == 'failed':
            raise ValueError('selected service identity/state failed during startup')
        if (value['ActiveState'] == 'active' and value['SubState'] == 'running'
                and pid.isdecimal() and int(pid) >= 2):
            try:
                if hashlib.sha256(Path('/proc', pid, 'exe').read_bytes()).hexdigest() == expected:
                    return value
            except (FileNotFoundError, ProcessLookupError):
                pass
        if time.monotonic() >= deadline:
            raise ValueError('selected service is not running the protected program')
        time.sleep(0.1)


def health(url):
    for _ in range(20):
        try:
            C.command('/usr/bin/curl', '--noproxy', '*', '--fail', '--silent', '--show-error', '--max-time', '2',
                      '--output', '/dev/null', url, timeout=3)
            return
        except ValueError:
            time.sleep(0.5)
    raise ValueError('local health check failed; preserve maintenance evidence')


def start_identity(value, user):
    if value['User'] != user or value['Group'] not in ('', user):
        raise ValueError('effective service user/group differs before start')


def resume(directory, approved, core_only=False):
    if not approved:
        raise ValueError('explicit service resumption approval required')
    m, r = load(directory)
    with exclusive(directory):
        terminal = 'CORE-RESUMED.json' if core_only else 'RESUMED.json'
        if (directory / terminal).exists():
            raise ValueError('resumption already completed; inspect instead of restarting')
        required = 'ROLLED-BACK.json' if core_only else 'APPLIED.json'
        if not core_only and (directory / 'ROLLBACK-REQUESTED.json').exists():
            raise ValueError('rollback already selected; overlay resumption is refused')
        C.read_json(directory / required)
        version = 'old' if core_only else 'new'
        for row in m['configs']:
            if TX.fingerprint(row['destination']) != row[version]:
                raise ValueError('resumption configuration differs')
        marker = 'CORE-RESUME-REQUESTED.json' if core_only else 'RESUME-REQUESTED.json'
        if not (directory / marker).exists():
            if not core_only:
                originals_unchanged(directory, r)
            else:
                stopped(r)
        if not core_only:
            accepted = C.read_json(directory / 'BACKUP-ACCEPTED.json')
            verified = json.loads(C.command(str(Q.BIN / 'cyberherd-statectl'), '--config', str(S.MAINTENANCE),
                                           'verify', '--bundle', str(directory / 'backup'), timeout=300))
            if verified != accepted['native']:
                raise ValueError('accepted backup failed independent verification before resumption')
        note(directory, marker, {'approved': True, 'core_only': core_only, 'payouts_enabled': False})
        old, new = unit_versions(r)
        review = Path(m['review'])
        restore_unit(original_path(review, r, C.UNIT) if core_only else review / C.UNIT.name, old if core_only else new)
        member_mask = MASKS / 'cyberherd.service'
        if not core_only and member_mask.is_symlink():
            if not masked(member_mask):
                raise ValueError('unexpected intake unit alias')
            member_mask.unlink(); TX.sync_directory(member_mask.parent)
        C.command('/usr/bin/systemctl', 'daemon-reload')
        stable(r)
        core_unit = C.status(C.UNIT_NAME)
        if (core_unit['FragmentPath'] != str(C.UNIT)
                or core_unit['DropInPaths'] != r['baseline']['installation']['core']['properties']['DropInPaths']):
            raise ValueError('effective core unit changed before start')
        start_identity(core_unit, 'lightning-goats')
        if core_only:
            if not masked(member_mask) or C.status('cyberherd.service')['MainPID'] != '0':
                raise ValueError('intake must stay masked for original-core resumption')
        else:
            member_unit = C.status('cyberherd.service')
            expected_dropins = {p for p in r['baseline'].get('role_dropins', {}) if Path(p).parent.name == 'cyberherd.service.d'}
            expected_dropins.update(S.global_dropins(r['baseline']['installation']['core']))
            if (member_unit['FragmentPath'] != str(Q.UNITS / 'cyberherd.service')
                    or set(member_unit['DropInPaths'].split()) != expected_dropins):
                raise ValueError('effective membership unit changed before start')
            start_identity(member_unit, 'cyberherd')
        C.command('/usr/bin/systemctl', 'start', '--', C.UNIT_NAME, timeout=120)
        running(C.UNIT_NAME, 'lightning-goats', Q.BIN / 'lightning-goatsd')
        health('http://127.0.0.1:8787/healthz'); health('http://127.0.0.1:8787/api/v1/status')
        if not core_only:
            C.command('/usr/bin/systemctl', 'start', '--', 'cyberherd.service', timeout=120)
            running('cyberherd.service', 'cyberherd', Q.BIN / 'cyberherdd')
            health('http://127.0.0.1:3340/api/v1/cyberherd')
        for role in ('cyberherd-payout', 'cyberherd-signer'):
            if not masked(MASKS / (role + '.service')) or C.status(role + '.service')['MainPID'] != '0':
                raise ValueError('outgoing role changed during resumption')
        note(directory, terminal, {'core_running': True, 'membership_running': not core_only, 'payouts_enabled': False})
    return {'core_running': True, 'membership_running': not core_only, 'payouts_enabled': False}


def rollback(directory, approved):
    if not approved:
        raise ValueError('explicit exclusive maintenance approval required')
    m, r = load(directory)
    with exclusive(directory):
        if (directory / 'CORE-RESUMED.json').exists():
            raise ValueError('original-core resumption already completed; inspect instead of stopping again')
        if not (directory / 'MAINTENANCE-STARTED.json').exists():
            raise ValueError('no interrupted/applied maintenance to roll back')
        note(directory, 'ROLLBACK-REQUESTED.json', {'configuration_only': True, 'database_restore_authorized': False})
        quiesce(r, rollback=True)
        TX.execute(directory / 'configs', lambda: stopped(r), rollback=True)
        if S.TMPFILE.exists() or S.TMPFILE.is_symlink():
            C.protected(S.TMPFILE)
            if digest(S.TMPFILE) != r['candidate_sha256']['bridge.tmpfiles']:
                raise ValueError('tmpfiles drift; retain for inspection')
            S.TMPFILE.unlink(); TX.sync_directory(S.TMPFILE.parent)
        # Journals, socket directory, signed jobs and replay history stay intact.
        stopped(r)
        note(directory, 'ROLLED-BACK.json', {'configuration_only': True, 'writers_masked': True, 'database_restored': False})
    return {'configuration_rolled_back': True, 'writers_masked': True, 'database_restored': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    p = commands.add_parser('plan')
    p.add_argument('review', type=Path); p.add_argument('directory', type=Path)
    p.add_argument('--review-sha256', required=True)
    for name in ('apply', 'rollback', 'resume', 'resume-core'):
        p = commands.add_parser(name); p.add_argument('directory', type=Path)
        p.add_argument('--exclusive-maintenance' if name in ('apply', 'rollback') else '--approve-resumption', action='store_true', required=True)
    args = parser.parse_args()
    if args.operation == 'plan':
        result = plan(args.review, args.review_sha256, args.directory)
    elif args.operation in ('apply', 'rollback'):
        result = (apply if args.operation == 'apply' else rollback)(args.directory, args.exclusive_maintenance)
    else:
        result = resume(args.directory, args.approve_resumption, core_only=args.operation == 'resume-core')
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
