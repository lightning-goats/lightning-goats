#!/usr/bin/env python3
"""Explicit core maintenance; preserve all four paid journals.

Authenticated launcher only. Plan opens no journal. Apply masks all writers,
verifies a current native backup and a separately migrated copy before replacing
three programs and migrating as the original UID. Resume is separate. Contact,
CyberHerd publication and payouts remain disabled. No database restore exists.
The separately tagged --current-schema path preserves existing contact config,
verifies matching compiled schemas, and replaces programs without live migration.
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
import stat
import tempfile
import tomllib
import uuid


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


P = helper('contact_preparation_support', 'prepare-contact-upgrade.py')
M = helper('contact_maintenance_support', 'maintain-cyberherd-overlay.py')
C, Q, S, TX = P.C, P.Q, P.S, P.TX
C.verified_tool_identity = lambda: verified_tool_identity()
M.C.verified_tool_identity = lambda: verified_tool_identity()
UPGRADE_KIND = 'contact'
MASKS = Path('/etc/systemd/system')
OVERRIDE_ROOTS = (MASKS, Path('/run/systemd/system'), Q.UNITS, Path('/usr/lib/systemd/system'))


def digest(path):
    return TX.fingerprint(path)['sha256']


def private_directory(path):
    C.protected(path, directory=True, private=True)
    if path.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', path.name) is None:
        raise ValueError('direct private upgrade-base child required')


def original(review, r, path):
    return review / ('original-' + str(r['original_order'].index(str(path))))


def patch_mode(record, current_schema):
    if record.get('core_upgrade', 'contact') != UPGRADE_KIND:
        raise ValueError('contact and feeder-cap upgrade transactions cannot be interchanged')
    if (record.get('current_schema_patch', False) is not current_schema
            or (current_schema and record.get('schema_upgrade_authorized') is not False)):
        raise ValueError('current-schema and migration transactions cannot be interchanged')


def review_record(review, expected, *, current_schema=False):
    private_directory(review)
    if re.fullmatch('[0-9a-f]{64}', expected) is None or digest(review / 'PREPARED.json') != expected:
        raise ValueError('independently reviewed preparation digest required')
    r = C.read_json(review / 'PREPARED.json')
    patch_mode(r, current_schema)
    if (r['version'] != 1 or r['tool'] != verified_tool_identity()
            or r.get('native_runtime_help_verified') is not True
            or any(r[key] is not False for key in ('activation_supported', 'services_changed',
                   'journals_opened', 'credentials_loaded'))
            or (not current_schema and r.get('contact_enabled') is not False)
            or (review / 'INCOMPLETE').exists()):
        raise ValueError('matching complete preparation-only review required')
    files, order = r['baseline']['files'], r['original_order']
    if len(order) != len(set(order)) or set(order) != set(files):
        raise ValueError('original review inventory differs')
    for index, path in enumerate(order):
        C.protected(review / f'original-{index}')
        if TX.fingerprint(review / f'original-{index}') != files[path]:
            raise ValueError('retained original file changed')
    if (r['baseline']['states']['core']['FragmentPath'] != str(C.UNIT)
            or r['baseline']['states']['membership']['FragmentPath'] != str(Q.UNITS / 'cyberherd.service')):
        raise ValueError('requires the reviewed custom core and installed membership units')
    for name in P.TARGET_NAMES:
        path = review / 'payload' / name
        C.protected(path)
        if digest(path) != r['selected_program_sha256'][name]:
            raise ValueError('selected native program changed')
    if (digest(review / 'release.tar.gz') != r['release']['archive_sha256']
            or digest(review / 'review.tar.gz') != r['review_archive_sha256']):
        raise ValueError('accepted source archive changed')
    core = tomllib.loads(original(review, r, C.CONFIG).read_text())
    configs = [tomllib.loads(original(review, r, Q.ETC / name / 'config.toml').read_text())
               for name in ('cyberherd', 'cyberherd-payout', 'cyberherd-signer')]
    P.configuration_boundary(core, *configs, r['baseline']['policy'], current_schema=current_schema)
    if r.get('contact_enabled') is not (core.get('contact') is not None):
        raise ValueError('retained contact configuration differs from the review')
    return r


def plan(review, expected, directory, *, current_schema=False):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    r = review_record(review, expected, current_schema=current_schema)
    if directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None:
        raise ValueError('new direct private execution directory required')

    def fresh():
        if P.baseline(current_schema=current_schema) != r['baseline']:
            raise ValueError('installation or process changed after preparation')
        member_mask = MASKS / 'cyberherd.service'
        if member_mask.exists() or member_mask.is_symlink():
            raise ValueError('membership mask destination must remain unused')

    fresh()
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    rows = TX.prepare(directory / 'programs', [
        (Q.BIN / name, review / 'payload' / name, r['baseline']['files'][str(Q.BIN / name)])
        for name in P.TARGET_NAMES
    ], fresh)
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    if current_schema:
        # Keep the original native verifier as protected data for the current
        # backup. The selected new verifier must independently accept that same
        # bundle against its compiled migrations before any candidate DB open.
        payload = directory / 'payload'
        payload.mkdir(mode=0o700)
        path = Q.BIN / 'cyberherd-statectl'
        TX.copy_durable(original(review, r, path), payload / path.name,
                        r['baseline']['files'][str(path)])
    fresh()
    C.write_json(directory / 'EXECUTION.json', {
        'version': 1, 'tool': verified_tool_identity(), 'review': str(review),
        'core_upgrade': UPGRADE_KIND, 'review_sha256': expected,
        'programs': rows, 'program_manifest_sha256': digest(directory / 'programs/TRANSACTION.json'),
        **({'current_schema_patch': True, 'schema_upgrade_authorized': False} if current_schema else {}),
    })
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'planned': True, 'services_changed': False, 'journals_opened': False}


def load(directory, *, current_schema=False):
    C.root()
    private_directory(directory)
    if (directory / 'INCOMPLETE').exists():
        raise ValueError('incomplete execution retained')
    m = C.read_json(directory / 'EXECUTION.json')
    patch_mode(m, current_schema)
    if m['version'] != 1 or m['tool'] != verified_tool_identity():
        raise ValueError('execution/tool identity changed')
    r = review_record(Path(m['review']), m['review_sha256'], current_schema=current_schema)
    if current_schema:
        path = directory / 'payload/cyberherd-statectl'
        C.protected(path)
        if TX.fingerprint(path) != r['baseline']['files'][str(Q.BIN / path.name)]:
            raise ValueError('retained current native verifier changed')
    if digest(directory / 'programs/TRANSACTION.json') != m['program_manifest_sha256']:
        raise ValueError('program transaction changed')
    transaction = C.read_json(directory / 'programs/TRANSACTION.json')
    if transaction['version'] != 1 or transaction['files'] != m['programs'] or len(m['programs']) != 3:
        raise ValueError('program replacement inventory differs')
    for index, (row, name) in enumerate(zip(m['programs'], P.TARGET_NAMES)):
        target = str(Q.BIN / name)
        old = r['baseline']['files'][target]
        if (row['destination'] != target or row['old'] != old
                or row['new'] != dict(old, sha256=r['selected_program_sha256'][name])):
            raise ValueError('program replacement differs from the sealed review')
        for version in ('old', 'new'):
            if TX.fingerprint(directory / 'programs' / f'{index}.{version}') != row[version]:
                raise ValueError('program transaction payload changed')
    return m, r


def stable(m, r, masked=False, installed=False):
    files = r['baseline']['files']
    selected = {row['destination']: (row['old'], row['new']) if installed is None
                else (row['new' if installed else 'old'],) for row in m['programs']}
    for name, fp in files.items():
        path = Path(name)
        if masked and path == C.UNIT:
            if not M.masked(path):
                raise ValueError('original core must retain its persistent mask')
            continue
        C.protected(path)
        if TX.fingerprint(path) not in selected.get(name, (fp,)):
            raise ValueError('protected installation changed during maintenance')
    runtime = Q.I.check_identities()
    if any(runtime[P.ROLE_NAMES[role]] != r['baseline']['identities'][role]
           for role in ('membership', 'payout', 'signer')):
        raise ValueError('runtime identity changed')
    core = C.pwd.getpwnam('lightning-goats')
    groups = {C.grp.getgrgid(gid).gr_name for gid in os.getgrouplist(core.pw_name, core.pw_gid)}
    if ({'uid': core.pw_uid, 'gid': core.pw_gid} != r['baseline']['identities']['core']
            or core.pw_shell != '/usr/sbin/nologin'
            or groups & {'root', 'sudo', 'wheel', 'admin', 'docker', 'lxd', 'disk'}):
        raise ValueError('core runtime identity changed')
    Q.I.require_no_sudo('lightning-goats')
    if S.role_dropins() != {name: fp for name, fp in files.items()
                            if Path(name).parent.name in {role + '.service.d' for role in Q.ROLES}}:
        raise ValueError('hidden role override set changed')
    hidden = {}
    for unit in (C.UNIT_NAME, 'service'):
        suffix = unit + '.d'
        for parent in OVERRIDE_ROOTS:
            path = parent / suffix
            if not path.exists() and not path.is_symlink():
                continue
            C.protected(path, directory=True)
            for candidate in path.glob('*.conf'):
                C.protected(candidate)
                hidden[str(candidate)] = TX.fingerprint(candidate)
    if hidden != {name: fp for name, fp in files.items()
                  if Path(name).parent.name in (C.UNIT_NAME + '.d', 'service.d')}:
        raise ValueError('hidden core or common override set changed')


def stopped(m, r, installed=False):
    stable(m, r, masked=True, installed=installed)
    for role, name in P.ROLE_NAMES.items():
        unit = name + '.service'
        value = C.status(unit)
        pins = {p: fp for p, fp in r['baseline']['files'].items()
                if Path(p).parent.name in ('service.d', unit + '.d')}
        if (value['LoadState'] != 'masked' or value['ActiveState'] != 'inactive'
                or value['SubState'] != 'dead' or value['MainPID'] != '0'
                or value['UnitFileState'] != 'masked' or not M.masked(MASKS / unit)
                or not Q.I.masked_reload_current(value, pins)):
            raise ValueError('all four writers must remain stopped and masked')
        group = value['ControlGroup']
        if group:
            if not group.startswith('/') or any(v in ('.', '..') for v in group.split('/')):
                raise ValueError('unexpected writer cgroup')
            events = Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.events'
            if events.exists() and 'populated 0' not in events.read_text().splitlines():
                raise ValueError('writer cgroup remains populated')
    Q.no_role_processes(r['baseline']['policy']['components'])


def quiesce(m, r):
    stable(m, r)
    for name in ('cyberherd-payout', 'cyberherd-signer'):
        if not M.masked(MASKS / (name + '.service')) or C.status(name + '.service')['MainPID'] != '0':
            raise ValueError('outgoing roles must remain masked')
    C.command('/usr/bin/systemctl', 'stop', '--', 'cyberherd.service', C.UNIT_NAME, timeout=120)
    M.mask(C.UNIT, (r['baseline']['files'][str(C.UNIT)],))
    member = MASKS / 'cyberherd.service'
    if member.exists() or member.is_symlink():
        raise ValueError('unexpected preexisting membership mask')
    member.symlink_to('/dev/null')
    TX.sync_directory(member.parent)
    C.command('/usr/bin/systemctl', 'daemon-reload')
    stopped(m, r)


def native(m, operation, *arguments):
    if UPGRADE_KIND == 'feeder-cap':
        operation = {'snapshot-before-contact':'snapshot-before-feeder-cap',
                     'verify-contact-upgrade-source':'verify-feeder-cap-upgrade-source',
                     'compare-contact-upgrade':'compare-feeder-cap-upgrade'}.get(operation, operation)
    program = Path(m['review']) / 'payload/cyberherd-statectl'
    C.protected(program)
    result = json.loads(C.command(str(program), '--config', str(S.MAINTENANCE), operation, *map(str, arguments), timeout=300))
    names = {'snapshot-before-feeder-cap':'feeder_cap_upgrade_snapshot',
             'verify-feeder-cap-upgrade-source':'feeder_cap_upgrade_source_verify',
             'compare-feeder-cap-upgrade':'feeder_cap_upgrade_compare',
             'snapshot-before-contact': 'contact_upgrade_snapshot',
             'verify-contact-upgrade-source': 'contact_upgrade_source_verify',
             'compare-contact-upgrade': 'contact_upgrade_compare', 'snapshot': 'snapshot', 'verify': 'verify'}
    if (operation not in names or set(result) != {'operation', 'id', 'accepted', 'resume_authorized'}
            or result.get('operation') != names[operation]
            or result.get('accepted') is not True or result.get('resume_authorized') is not False
            or str(uuid.UUID(result['id'])) != result['id']):
        raise ValueError('native state operation was not positively accepted')
    return result


def state_files(r):
    """Fingerprint stopped original/runtime images without a database parser."""
    result = {}
    for role, component in r['baseline']['policy']['components'].items():
        database = Path(component['database'])
        C.protected(database.parent.parent, directory=True)
        parent = database.parent.lstat()
        if (not stat.S_ISDIR(parent.st_mode) or parent.st_uid != component['uid']
                or parent.st_gid != component['gid'] or stat.S_IMODE(parent.st_mode) != 0o700):
            raise ValueError('runtime state directory changed')
        for path in [Path(str(database) + '-journal'), database.parent / 'CYBERHERD_RECOVERY_REQUIRED']:
            if path.exists() or path.is_symlink():
                raise ValueError('journal or recovery marker requires reconciliation')
        for suffix in ('', '-wal', '-shm'):
            path = Path(str(database) + suffix)
            if not path.exists() and not path.is_symlink():
                if not suffix:
                    raise ValueError('missing original journal')
                result[str(path)] = None
                continue
            info = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                    or info.st_uid != component['uid'] or info.st_gid != component['gid']
                    or stat.S_IMODE(info.st_mode) != 0o600
                    or info.st_size > r['baseline']['policy']['max_component_bytes']):
                raise ValueError('runtime journal identity or bounds changed')
            result[str(path)] = TX.fingerprint(path)
    return result


def sources_match(m, r, observed, installed=False):
    stopped(m, r, installed=installed)
    if state_files(r) != observed:
        raise ValueError('original financial images changed during maintenance')


def backup(directory, m, r, *, current_schema=False):
    stopped(m, r)
    before = state_files(r)
    verifier = {'review': str(directory)} if current_schema else m
    native(verifier, 'snapshot' if current_schema else 'snapshot-before-contact',
           '--destination', directory / 'backup', '--writers-stopped')
    accepted = native(verifier, 'verify' if current_schema else 'verify-contact-upgrade-source',
                      '--bundle', directory / 'backup')
    if current_schema:
        # verify checks exact compiled schemas and migration checksums of all
        # four stores without migrating them. A future-schema build fails here.
        selected = native(m, 'verify', '--bundle', directory / 'backup')
        if selected != accepted:
            raise ValueError('selected native schema compatibility differs')
    sources_match(m, r, before)
    M.note(directory, 'BACKUP-ACCEPTED.json', {'native': accepted, 'originals': before,
            'manifest_sha256': digest(directory / 'backup/MANIFEST.json')})
    return before


def migrate_as_runtime(program, config, component):
    return C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
              '--reuid=' + str(component['uid']), '--regid=' + str(component['gid']),
              '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all',
              '--no-new-privs', str(program), '--config', str(config), 'status', timeout=300)


def clone_compare(directory, m, r, observed, *, current_schema=False):
    sources_match(m, r, observed)
    review = Path(m['review'])
    candidate = directory / 'candidate'
    candidate.mkdir(mode=0o700)
    manifest = C.read_json(directory / 'backup/MANIFEST.json')
    for image in manifest['images']:
        source = directory / 'backup' / image['name']
        C.copy_raw_image(source, candidate / image['name'], TX.fingerprint(source))
    C.protected(Path('/var/lib'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-contact-clone-', dir='/var/lib'))
    carrier.chmod(0o711)
    component = r['baseline']['policy']['components']['core']
    child = carrier / 'core'
    child.mkdir(mode=0o700)
    os.chown(child, component['uid'], component['gid'])
    try:
        for suffix in ('', '-wal'):
            source = candidate / ('core.db' + suffix)
            if source.exists():
                target = child / ('core.db' + suffix)
                C.copy_raw_image(source, target, TX.fingerprint(source))
                os.chown(target, component['uid'], component['gid'])
        config = tomllib.loads(original(review, r, C.CONFIG).read_text())
        config['database']['url'] = 'sqlite://' + str(child / 'core.db')
        Q.I.publish(carrier / 'config.toml', S.W.toml_text(config).encode(), 0o440)
        os.chown(carrier / 'config.toml', 0, component['gid'])
        program = review / 'payload/lightning-goatsctl'
        TX.copy_durable(program, carrier / 'lightning-goatsctl', TX.fingerprint(program))
        if current_schema:
            old_program = original(review, r, Q.BIN / 'lightning-goatsctl')
            TX.copy_durable(old_program, carrier / 'original-ctl', TX.fingerprint(old_program))
            previous = migrate_as_runtime(carrier / 'original-ctl', carrier / 'config.toml', component)
            selected = migrate_as_runtime(carrier / 'lightning-goatsctl', carrier / 'config.toml', component)
            if not isinstance(previous, str) or not previous.strip() or selected != previous:
                raise ValueError('selected native config/status differs on the current-schema copy')
        else:
            migrate_as_runtime(carrier / 'lightning-goatsctl', carrier / 'config.toml', component)
        sources_match(m, r, observed)
        Q.no_role_processes({'core': component})
        if (child / 'core.db-journal').exists() or (child / 'core.db-journal').is_symlink():
            raise ValueError('clone rollback journal requires review')
        for suffix in ('', '-wal', '-shm'):
            path = child / ('core.db' + suffix)
            if not path.exists() and not path.is_symlink():
                continue
            info = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                    or info.st_uid != component['uid'] or info.st_gid != component['gid']
                    or stat.S_IMODE(info.st_mode) != 0o600
                    or info.st_size > r['baseline']['policy']['max_component_bytes']):
                raise ValueError('clone file identity changed')
        if current_schema:
            result = native(m, 'verify', '--bundle', directory / 'backup')
            sources_match(m, r, observed)
            M.note(directory, 'CLONE-ACCEPTED.json', {'native': result, 'carrier': str(carrier),
                    'current_schema_patch': True, 'schema_upgrade_authorized': False,
                    'matching_native_status_sha256': hashlib.sha256(selected.encode()).hexdigest(),
                    'backup_manifest_sha256': digest(directory / 'backup/MANIFEST.json')})
            return
        # Retain the original candidate core images; replace only the separate
        # candidate with the native-migrated clone, never the accepted backup.
        for suffix in ('', '-wal'):
            target = candidate / ('core.db' + suffix)
            if target.exists():
                target.rename(candidate / ('before-core.db' + suffix))
            source = child / ('core.db' + suffix)
            if source.exists():
                C.copy_raw_image(source, target, TX.fingerprint(source))
        # Original candidate core bytes are evidence, outside the comparison's
        # fixed four-journal image namespace.
        retained = directory / 'candidate-original-core'
        retained.mkdir(mode=0o700)
        for path in candidate.glob('before-core.db*'):
            path.rename(retained / path.name)
        TX.sync_directory(retained)
        TX.sync_directory(candidate)
        result = native(m, 'compare-contact-upgrade', '--bundle', directory / 'backup', '--candidate-copy', candidate)
        sources_match(m, r, observed)
        M.note(directory, 'CLONE-ACCEPTED.json', {'native': result, 'carrier': str(carrier),
                'backup_manifest_sha256': digest(directory / 'backup/MANIFEST.json')})
    finally:
        carrier.chmod(0o700)
        TX.sync_directory(carrier)


def apply(directory, approved, *, current_schema=False):
    if not approved:
        raise ValueError('explicit exclusive maintenance approval required')
    m, r = load(directory, current_schema=current_schema)
    with M.exclusive(directory):
        if any((directory / name).exists() for name in ('MAINTENANCE-STARTED.json', 'RESUME-REQUESTED.json')):
            raise ValueError('maintenance already began; preserve evidence and inspect rather than reapply')
        if P.baseline(current_schema=current_schema) != r['baseline']:
            raise ValueError('reviewed installation/processes changed')
        M.note(directory, 'MAINTENANCE-STARTED.json', {'exclusive_maintenance': True, 'database_restore_authorized': False})
        quiesce(m, r)
        observed = backup(directory, m, r, current_schema=current_schema)
        clone_compare(directory, m, r, observed, current_schema=current_schema)
        TX.execute(directory / 'programs', lambda: sources_match(m, r, observed, installed=None))
        sources_match(m, r, observed, installed=True)
        if current_schema:
            verified = native(m, 'verify', '--bundle', directory / 'backup')
            sources_match(m, r, observed, installed=True)
            M.note(directory, 'COMPARISON.json', {'verified': verified, 'live_images': observed,
                    'backup_manifest_sha256': digest(directory / 'backup/MANIFEST.json'),
                    'current_schema_patch': True, 'schema_upgrade_authorized': False,
                    'live_migration_performed': False})
            return {'upgraded': True, 'four_journal_backup_verified': True, 'clone_compared': True,
                    'live_migration_performed': False, 'financial_images_unchanged': True,
                    'services_started': False, 'contact_enabled': r['contact_enabled']}
        M.note(directory, 'MIGRATION-STARTED.json', {'database_restore_authorized': False, 'old_binary_rollback_authorized': False})
        migrate_as_runtime(Q.BIN / 'lightning-goatsctl', C.CONFIG, r['baseline']['policy']['components']['core'])
        stopped(m, r, installed=True)
        native(m, 'snapshot', '--destination', directory / 'migrated', '--writers-stopped')
        verified = native(m, 'verify', '--bundle', directory / 'migrated')
        compared = native(m, 'compare-contact-upgrade', '--bundle', directory / 'backup', '--candidate-copy', directory / 'migrated')
        after = state_files(r)
        sources_match(m, r, after, installed=True)
        M.note(directory, 'COMPARISON.json', {'verified': verified, 'compared': compared, 'live_images': after,
                'core_upgrade': UPGRADE_KIND,
                'migrated_manifest_sha256': digest(directory / 'migrated/MANIFEST.json')})
    return {'upgraded': True, 'four_journal_backup_verified': True, 'clone_compared': True,
            'live_migration_compared': True, 'services_started': False, 'contact_enabled': False}


def repair_backlog(directory, repair_id, credit_entry_id, expected_sats):
    """Retire one reviewed original funding lot after verified cap migration.

    Writers remain masked throughout. No service, refund, gateway, provider or
    publication operation is available here. A started repair is never retried
    automatically; preserve the original migration and repair evidence.
    """
    if UPGRADE_KIND != 'feeder-cap':
        raise ValueError('explicit feeder-cap selector required for backlog repair')
    if (not isinstance(repair_id, str) or re.fullmatch('[A-Za-z0-9_:-]{1,256}', repair_id) is None
            or type(credit_entry_id) is not int or credit_entry_id < 1
            or type(expected_sats) is not int or not 1 <= expected_sats <= 2**63-1):
        raise ValueError('stable repair ID and exact positive source credit/amount required')
    m, r = load(directory)
    with M.exclusive(directory):
        if any((directory / name).exists() for name in ('REPAIR-STARTED.json', 'RESUME-REQUESTED.json')):
            raise ValueError('repair/resumption already started; preserve evidence and inspect')
        accepted = C.read_json(directory / 'COMPARISON.json')
        patch_mode(accepted, False)
        sources_match(m, r, accepted['live_images'], installed=True)
        if (native(m, 'verify', '--bundle', directory / 'migrated') != accepted['verified']
                or native(m, 'compare-contact-upgrade', '--bundle', directory / 'backup',
                          '--candidate-copy', directory / 'migrated') != accepted['compared']
                or digest(directory / 'migrated/MANIFEST.json') != accepted['migrated_manifest_sha256']):
            raise ValueError('verified pre-repair migration comparison changed')
        args = (str(Q.BIN / 'lightning-goatsctl'), '--config', str(C.CONFIG), 'retire-feed-credit',
                '--id', repair_id, '--credit-entry-id', str(credit_entry_id),
                '--expected-sats', str(expected_sats), '--writers-stopped')
        preview = json.loads(C.command(*args, timeout=300))
        if (set(preview) != {'applied', 'duplicate', 'retired_sats', 'feed_credit_sats'}
                or preview['applied'] is not False or preview['duplicate'] is not False
                or preview['retired_sats'] != expected_sats
                or type(preview['feed_credit_sats']) is not int or preview['feed_credit_sats'] < 0):
            raise ValueError('exact native retirement preview refused or changed')
        # Rollback-only preview may checkpoint WAL; accept only runtime-owned
        # native state with all writers still masked, never a caller file list.
        stopped(m, r, installed=True)
        M.note(directory, 'REPAIR-STARTED.json', {'id': repair_id, 'credit_entry_id': credit_entry_id,
                'expected_sats': expected_sats, 'preview': preview, 'non_feeding_adjustment': True})
        applied = json.loads(C.command(*args, '--apply', timeout=300))
        if applied != dict(preview, applied=True):
            raise ValueError('exact native repair was not positively confirmed; inspect retained evidence')
        stopped(m, r, installed=True)
        native(m, 'snapshot', '--destination', directory / 'repaired', '--writers-stopped')
        verified = native(m, 'verify', '--bundle', directory / 'repaired')
        after = state_files(r)
        sources_match(m, r, after, installed=True)
        M.note(directory, 'REPAIR-ACCEPTED.json', {'core_upgrade': UPGRADE_KIND,
                'result': applied, 'verified': verified, 'live_images': after,
                'repaired_manifest_sha256': digest(directory / 'repaired/MANIFEST.json')})
    return {'credit_retired': True, 'services_started': False, 'physical_feedings_created': False,
            'retired_sats': expected_sats, 'feed_credit_sats': applied['feed_credit_sats']}


def resume(directory, approved, *, current_schema=False):
    if not approved:
        raise ValueError('explicit service resumption approval required')
    m, r = load(directory, current_schema=current_schema)
    with M.exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists():
            raise ValueError('resumption already began; inspect instead of restarting')
        accepted = C.read_json(directory / 'COMPARISON.json')
        patch_mode(accepted, current_schema)
        repair = None
        if UPGRADE_KIND == 'feeder-cap':
            repair = C.read_json(directory / 'REPAIR-ACCEPTED.json')
            if (repair.get('core_upgrade') != UPGRADE_KIND
                    or native(m, 'verify', '--bundle', directory / 'repaired') != repair['verified']
                    or digest(directory / 'repaired/MANIFEST.json') != repair['repaired_manifest_sha256']):
                raise ValueError('verified exact backlog repair is required before cap resumption')
        sources_match(m, r, (repair or accepted)['live_images'], installed=True)
        if current_schema:
            if (accepted.get('live_migration_performed') is not False
                    or native(m, 'verify', '--bundle', directory / 'backup') != accepted['verified']
                    or native({'review': str(directory)}, 'verify', '--bundle', directory / 'backup') != accepted['verified']
                    or digest(directory / 'backup/MANIFEST.json') != accepted['backup_manifest_sha256']):
                raise ValueError('accepted current-schema comparison changed before resumption')
        elif (native(m, 'verify', '--bundle', directory / 'migrated') != accepted['verified']
                or native(m, 'compare-contact-upgrade', '--bundle', directory / 'backup',
                          '--candidate-copy', directory / 'migrated') != accepted['compared']
                or digest(directory / 'migrated/MANIFEST.json') != accepted['migrated_manifest_sha256']):
            raise ValueError('accepted migration comparison changed before resumption')
        M.note(directory, 'RESUME-REQUESTED.json', {'approved': True, 'contact_enabled': r['contact_enabled'], 'payouts_enabled': False})
        M.restore_unit(original(Path(m['review']), r, C.UNIT), r['baseline']['files'][str(C.UNIT)])
        member_mask = MASKS / 'cyberherd.service'
        if not M.masked(member_mask):
            raise ValueError('membership mask changed before restoration')
        member_mask.unlink()
        TX.sync_directory(member_mask.parent)
        C.command('/usr/bin/systemctl', 'daemon-reload')
        stable(m, r, installed=True)
        for role in ('core', 'membership'):
            name = P.ROLE_NAMES[role] + '.service'
            value = C.status(name)
            before = r['baseline']['states'][role]
            if (value['FragmentPath'] != before['FragmentPath'] or value['DropInPaths'] != before['DropInPaths']
                    or value['NeedDaemonReload'] != 'no'):
                raise ValueError('effective unit changed before resumption')
            M.start_identity(value, 'lightning-goats' if role == 'core' else 'cyberherd')
        C.command('/usr/bin/systemctl', 'start', '--', C.UNIT_NAME, timeout=120)
        M.running(C.UNIT_NAME, 'lightning-goats', Q.BIN / 'lightning-goatsd')
        M.health('http://127.0.0.1:8787/healthz')
        C.command('/usr/bin/systemctl', 'start', '--', 'cyberherd.service', timeout=120)
        M.running('cyberherd.service', 'cyberherd', Q.BIN / 'cyberherdd')
        M.health('http://127.0.0.1:3340/api/v1/cyberherd')
        for role in ('cyberherd-payout', 'cyberherd-signer'):
            if not M.masked(MASKS / (role + '.service')) or C.status(role + '.service')['MainPID'] != '0':
                raise ValueError('outgoing role changed during resumption')
        M.note(directory, 'RESUMED.json', {'core_running': True, 'membership_running': True,
                'contact_enabled': r['contact_enabled'], 'payouts_enabled': False})
    return {'core_running': True, 'membership_running': True, 'contact_enabled': r['contact_enabled'], 'payouts_enabled': False}


def main():
    global UPGRADE_KIND
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--feeder-cap', action='store_true', help='exact contained schema-24 to 25 feeder-cap upgrade')
    parser.add_argument('--current-schema', action='store_true', help='program-only patch; never migrate live state')
    commands = parser.add_subparsers(dest='operation', required=True)
    planning = commands.add_parser('plan')
    planning.add_argument('review', type=Path)
    planning.add_argument('directory', type=Path)
    planning.add_argument('--review-sha256', required=True)
    applying = commands.add_parser('apply')
    applying.add_argument('directory', type=Path)
    applying.add_argument('--exclusive-maintenance', required=True, action='store_true')
    repairing = commands.add_parser('repair-backlog')
    repairing.add_argument('directory', type=Path)
    repairing.add_argument('--id', required=True)
    repairing.add_argument('--credit-entry-id', required=True, type=int)
    repairing.add_argument('--expected-sats', required=True, type=int)
    resuming = commands.add_parser('resume')
    resuming.add_argument('directory', type=Path)
    resuming.add_argument('--approve-resumption', required=True, action='store_true')
    args = parser.parse_args()
    if args.feeder_cap:
        if args.current_schema: raise ValueError('upgrade kinds cannot be combined')
        UPGRADE_KIND = P.UPGRADE_KIND = 'feeder-cap'
    if args.operation == 'plan':
        result = plan(args.review, args.review_sha256, args.directory, current_schema=args.current_schema)
    elif args.operation == 'apply':
        result = apply(args.directory, args.exclusive_maintenance, current_schema=args.current_schema)
    elif args.operation == 'repair-backlog':
        result = repair_backlog(args.directory, args.id, args.credit_entry_id, args.expected_sats)
    else:
        result = resume(args.directory, args.approve_resumption, current_schema=args.current_schema)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
