#!/usr/bin/env python3
"""Upgrade six native programs while retaining the zero-budget overlay stage.

Authenticated root-owned launcher only. Prepare never opens a journal. Explicit
maintenance stops/masks four writers, accepts a current native backup with both
verifiers, compares original/selected core status on a separate copy, and reviews
copied reward history before replacing programs. Configuration, credentials and
live database bytes never change. Resumption and pre-resumption rollback are
separate explicit operations. This is not financial activation or recovery.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import tarfile
import tempfile
import tomllib
import uuid

SPEC = importlib.util.spec_from_file_location('program_maintenance_support', Path(__file__).with_name('maintain-contact-upgrade.py'))
U = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(U)
U.verified_tool_identity = lambda: verified_tool_identity()
C, Q, S, M, TX = U.C, U.Q, U.S, U.M, U.TX
PROGRAMS = ('lightning-goatsd', 'lightning-goatsctl', 'cyberherd-statectl',
            'cyberherdd', 'cyberherd-payoutd', 'cyberherd-signerd')
WORKERS = {'membership': ('cyberherdd', 'CyberHerd configuration is valid; no runtime started'),
           'payout': ('cyberherd-payoutd', 'Payout configuration is valid; no runtime started'),
           'signer': ('cyberherd-signerd', 'Outgoing signer configuration is valid; no runtime started')}


def fresh(baseline):
    if U.P.baseline(current_schema=True) != baseline:
        raise ValueError('original zero-budget installation or running process changed')
    if (baseline['states']['core']['FragmentPath'] != str(C.UNIT)
            or baseline['states']['membership']['FragmentPath'] != str(Q.UNITS / 'cyberherd.service')
            or (U.MASKS / 'cyberherd.service').exists() or (U.MASKS / 'cyberherd.service').is_symlink()):
        raise ValueError('original running core/member unit topology required')


def native_configuration_checks(directory, record):
    """Execute each selected worker's actual parser as its original UID, offline."""
    C.protected(Path('/run'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-program-check-', dir='/run'))
    carrier.chmod(0o711)
    try:
        for role, (name, positive) in WORKERS.items():
            component = record['baseline']['identities'][role]
            program = carrier / name
            selected = directory / 'payload' / name
            if U.digest(selected) != record['selected_program_sha256'][name]:
                raise ValueError('selected worker changed before native configuration check')
            TX.copy_durable(selected, program, TX.fingerprint(selected))
            config = carrier / (role + '.toml')
            source = U.original(directory, record, Q.ETC / U.P.ROLE_NAMES[role] / 'config.toml')
            Q.I.publish(config, source.read_bytes(), 0o440)
            os.chown(config, 0, component['gid'])
            result = C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
                '--reuid=' + str(component['uid']), '--regid=' + str(component['gid']), '--clear-groups',
                '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
                str(program), '--config', str(config), '--check', timeout=30)
            if result.strip() != positive:
                raise ValueError('selected native worker did not positively accept retained configuration')
            if (U.digest(selected) != record['selected_program_sha256'][name]
                    or config.read_bytes() != source.read_bytes()):
                raise ValueError('native worker or checked configuration changed')
    finally:
        # The exact selected programs/configurations remain in the protected
        # review. These UID-readable execution copies are disposable, including
        # after parser or partial-copy failure; retaining them exhausts /run.
        shutil.rmtree(carrier)
    return {'accepted': True, 'original_worker_uids': True, 'isolated_network': True,
            'configurations_checked': 3, 'journals_opened': False, 'credentials_loaded': False}


def prepare(archive, review_archive, source, archive_sha, review_sha, directory, *, inclusive_fee_upgrade=False):
    C.root()
    C.protected(C.BASE, directory=True, private=True)
    if (directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None
            or re.fullmatch('[0-9a-f]{40}', source) is None):
        raise ValueError('new direct protected upgrade-base child and full source required')
    baseline = U.P.baseline(current_schema=True)
    fresh(baseline)
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    release = directory / 'release.tar.gz'
    U.P.stage_archive(archive, release, archive_sha)
    # Preflight makes an archive copy and extracts the complete payload. Keep
    # that bounded, private scratch space on the review filesystem: /tmp may be
    # a small memory-backed mount on the actual VPS. This synchronous launcher
    # has no concurrent worker; restore the process-local setting on all exits.
    previous_tempdir = tempfile.tempdir
    try:
        tempfile.tempdir = str(directory)
        accepted = C.PREFLIGHT.preflight(release, source, archive_sha)
    finally:
        tempfile.tempdir = previous_tempdir
    payload = directory / 'payload'
    payload.mkdir(mode=0o700)
    C.PREFLIGHT.RELEASE.verify_archive(release, source, payload)
    review = directory / 'review.tar.gz'
    U.P.stage_archive(review_archive, review, review_sha)
    Q.I.PACKAGE.verify(review, source)
    remaining = set(PROGRAMS) - {'lightning-goatsd', 'lightning-goatsctl'}
    with tarfile.open(review, 'r:gz') as reader:
        for member in reader:
            # The accepted package permits conventional ./ prefixes. Use the
            # same canonical member spelling as its verifier before selection.
            name = str(Path(member.name))
            if name not in remaining:
                continue
            if not member.isfile() or (payload / name).exists():
                raise ValueError('selected native archives overlap or contain a non-program')
            Q.I.publish(payload / name, reader.extractfile(member).read(), 0o755)
            remaining.remove(name)
    if remaining:
        raise ValueError('complete four-role native program set required')
    hashes = {}
    for name in PROGRAMS:
        path = payload / name
        C.protected(path)
        if TX.fingerprint(path)['mode'] != 0o755:
            raise ValueError('selected native executable metadata differs')
        hashes[name] = U.digest(path)
        C.command(str(path), '--help', timeout=30)
    if C.command(str(payload / 'cyberherd-statectl'), '--config', str(S.MAINTENANCE), 'check', timeout=30).strip() != 'State maintenance policy is valid; no state, credentials or services accessed':
        raise ValueError('selected native maintenance policy check refused')
    for index, path in enumerate(baseline['files']):
        TX.copy_durable(path, directory / f'original-{index}', baseline['files'][path])
    record = {'version': 1, 'tool': verified_tool_identity(), 'program_upgrade_only': not inclusive_fee_upgrade,
        **({'inclusive_fee_upgrade': True} if inclusive_fee_upgrade else {}),
        'source_commit': source, 'release': accepted, 'review_archive_sha256': review_sha,
        'baseline': baseline, 'original_order': list(baseline['files']), 'selected_program_sha256': hashes,
        'activation_authorized': False, 'schema_upgrade_authorized': False,
        'services_changed': False, 'journals_opened': False, 'credentials_loaded': False}
    record['native_configuration_checks'] = native_configuration_checks(directory, record)
    # Keep the old verifier independently executable after the installed name is
    # replaced. Original file metadata (including labels) is preserved.
    retained = directory / 'original-verifier'
    retained.mkdir(mode=0o700)
    verifier = Q.BIN / 'cyberherd-statectl'
    TX.copy_durable(verifier, retained / verifier.name, baseline['files'][str(verifier)])
    record['programs'] = TX.prepare(directory / 'programs', [
        (Q.BIN / name, payload / name, baseline['files'][str(Q.BIN / name)]) for name in PROGRAMS
    ], lambda: fresh(baseline))
    record['program_manifest_sha256'] = U.digest(directory / 'programs/TRANSACTION.json')
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    fresh(baseline)
    C.write_json(directory / 'PREPARED.json', record)
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'programs': 6, 'configurations_changed': False,
            'activation_authorized': False, 'services_changed': False, 'journals_opened': False}


def load(directory, expected, *, inclusive_fee_upgrade=False):
    C.root()
    U.private_directory(directory)
    if (re.fullmatch('[0-9a-f]{64}', expected) is None
            or U.digest(directory / 'PREPARED.json') != expected or (directory / 'INCOMPLETE').exists()):
        raise ValueError('independently reviewed complete preparation digest required')
    r = C.read_json(directory / 'PREPARED.json')
    if (r.get('version') != 1 or r.get('tool') != verified_tool_identity()
            or r.get('program_upgrade_only') is not (not inclusive_fee_upgrade)
            or r.get('inclusive_fee_upgrade', False) is not inclusive_fee_upgrade
            or any(r.get(k) is not False for k in ('activation_authorized', 'schema_upgrade_authorized',
                                                 'services_changed', 'journals_opened', 'credentials_loaded'))
            or set(r['selected_program_sha256']) != set(PROGRAMS)):
        raise ValueError('matching program-only preparation required')
    if (r['release']['source_commit'] != r['source_commit']
            or U.digest(directory / 'release.tar.gz') != r['release']['archive_sha256']
            or U.digest(directory / 'review.tar.gz') != r['review_archive_sha256']):
        raise ValueError('accepted native source archive changed')
    files, order = r['baseline']['files'], r['original_order']
    if len(order) != len(set(order)) or set(order) != set(files):
        raise ValueError('original protected file inventory changed')
    for path in order:
        saved = U.original(directory, r, Path(path))
        C.protected(saved)
        if TX.fingerprint(saved) != files[path]:
            raise ValueError('retained original changed')
    configs = [tomllib.loads(U.original(directory, r, C.CONFIG if role == 'core' else
        Q.ETC / name / 'config.toml').read_text()) for role, name in U.P.ROLE_NAMES.items()]
    U.P.configuration_boundary(*configs, r['baseline']['policy'], current_schema=True)
    retained = directory / 'original-verifier/cyberherd-statectl'
    C.protected(retained)
    if TX.fingerprint(retained) != files[str(Q.BIN / retained.name)]:
        raise ValueError('retained original native verifier changed')
    if U.digest(directory / 'programs/TRANSACTION.json') != r['program_manifest_sha256']:
        raise ValueError('program transaction manifest changed')
    expected_rows = []
    for index, name in enumerate(PROGRAMS):
        selected = directory / 'payload' / name
        C.protected(selected)
        if U.digest(selected) != r['selected_program_sha256'][name]:
            raise ValueError('selected native payload changed')
        old = files[str(Q.BIN / name)]
        row = {'destination': str(Q.BIN / name), 'old': old,
               'new': dict(old, sha256=r['selected_program_sha256'][name])}
        expected_rows.append(row)
        for version in ('old', 'new'):
            retained = directory / 'programs' / f'{index}.{version}'
            C.protected(retained)
            if TX.fingerprint(retained) != row[version]:
                raise ValueError('retained native replacement changed')
    if (r['programs'] != expected_rows or C.read_json(directory / 'programs/TRANSACTION.json') !=
            {'version': 1, 'files': expected_rows}):
        raise ValueError('only the six reviewed native programs may be replaced')
    return {'programs': expected_rows, 'review': str(directory)}, r


def offline_fee_migration_child(program, config, component):
    """Child primitive for the future stopped-writer coordinator, not a CLI mode.

    Caller must seal the selected hashes, hold all four writers, retain a native
    backup and record migration start before calling. Never use worker startup
    to migrate. Failure may follow a partial migration and must remain held.
    """
    C.root()
    if any(type(component.get(name)) is not int or not 0 < component[name] < 2**32
           for name in ('uid', 'gid')):
        raise ValueError('separate non-root payout UID/GID required')
    C.protected(program)
    C.protected(config)
    output = C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
        '--reuid=' + str(component['uid']), '--regid=' + str(component['gid']),
        '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all',
        '--no-new-privs', str(program), '--config', str(config),
        '--migrate-inclusive-fees', '--writers-stopped', timeout=300)
    if output.strip() != 'Existing payout journal migrated; no runtime, credential or transport started':
        raise ValueError('offline fee migration did not positively confirm; preserve stopped state')
    return {'migrated': True, 'worker_started': False, 'credential_loaded': False}


def original_verify(directory, operation, *arguments):
    # U.native resolves review/payload/cyberherd-statectl. Give it an independent
    # retained namespace, never the installed program or selected new verifier.
    program = directory / 'original-verifier/cyberherd-statectl'
    C.protected(program)
    value = json.loads(C.command(str(program), '--config', str(S.MAINTENANCE), operation,
                                *map(str, arguments), timeout=300))
    if (set(value) != {'operation', 'id', 'accepted', 'resume_authorized'}
            or value['operation'] != operation or value['accepted'] is not True
            or value['resume_authorized'] is not False or str(uuid.UUID(value['id'])) != value['id']):
        raise ValueError('original native snapshot or verify did not positively accept')
    return value


def inclusive_native(directory, operation, *arguments):
    """Bounded native receipt interface for the separate schema coordinator."""
    names = {
        'snapshot-before-inclusive-fees': 'inclusive_fee_upgrade_snapshot',
        'verify-inclusive-fee-upgrade-source': 'inclusive_fee_upgrade_source_verify',
        'compare-inclusive-fee-upgrade': 'inclusive_fee_upgrade_compare',
        'snapshot': 'snapshot', 'verify': 'verify',
    }
    if operation not in names:
        raise ValueError('unsupported inclusive-fee native operation')
    program = directory / 'payload/cyberherd-statectl'
    C.protected(program)
    value = json.loads(C.command(str(program), '--config', str(S.MAINTENANCE), operation,
                                *map(str, arguments), timeout=300))
    if (set(value) != {'operation', 'id', 'accepted', 'resume_authorized'}
            or value['operation'] != names[operation] or value['accepted'] is not True
            or value['resume_authorized'] is not False or type(value['id']) is not str):
        raise ValueError('inclusive-fee native receipt refused')
    identity = uuid.UUID(value['id'])
    if identity.int == 0 or str(identity) != value['id']:
        raise ValueError('inclusive-fee native identity refused')
    return value


def inclusive_backup(directory, m, r):
    """Call only inside exclusive maintenance after all writer masks are held."""
    if r.get('inclusive_fee_upgrade') is not True or r.get('program_upgrade_only') is not False:
        raise ValueError('distinct inclusive-fee upgrade review required')
    U.stopped(m, r)
    observed = U.state_files(r)
    original_verify(directory, 'snapshot', '--destination', directory / 'backup', '--writers-stopped')
    original = original_verify(directory, 'verify', '--bundle', directory / 'backup')
    selected = inclusive_native(directory, 'verify-inclusive-fee-upgrade-source', '--bundle', directory / 'backup')
    if original['id'] != selected['id']:
        raise ValueError('original and selected source verifiers accepted different backups')
    U.sources_match(m, r, observed)
    M.note(directory, 'BACKUP-ACCEPTED.json', {'native': original, 'selected_source': selected,
            'originals': observed, 'manifest_sha256': U.digest(directory / 'backup/MANIFEST.json'),
            'inclusive_fee_upgrade': True, 'migration_started': False})
    return observed


def inclusive_clone_compare(directory, m, r, observed):
    """Migrate only a separate payout copy; originals stay byte-fenced."""
    U.sources_match(m, r, observed)
    accepted = C.read_json(directory / 'BACKUP-ACCEPTED.json')
    if (accepted.get('inclusive_fee_upgrade') is not True
            or accepted['manifest_sha256'] != U.digest(directory / 'backup/MANIFEST.json')
            or inclusive_native(directory, 'verify-inclusive-fee-upgrade-source',
                                '--bundle', directory / 'backup') != accepted['selected_source']):
        raise ValueError('accepted inclusive-fee backup changed before rehearsal')
    manifest = C.read_json(directory / 'backup/MANIFEST.json')
    candidate = directory / 'candidate'
    candidate.mkdir(mode=0o700)
    for image in manifest['images']:
        source = directory / 'backup' / image['name']
        C.copy_raw_image(source, candidate / image['name'], TX.fingerprint(source))
    # Protected disk-backed parent, with only a random scratch carrier granting
    # traversal to its private runtime-owned child. Never relax the audit base.
    C.protected(Path('/var/lib'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-fee-clone-', dir='/var/lib'))
    carrier.chmod(0o711)
    component = r['baseline']['policy']['components']['payout']
    child = carrier / 'payout'
    child.mkdir(mode=0o700)
    os.chown(child, component['uid'], component['gid'])
    complete = False
    try:
        for suffix in ('', '-wal'):
            source = candidate / ('payout.db' + suffix)
            if source.exists():
                target = child / source.name
                C.copy_raw_image(source, target, TX.fingerprint(source))
                os.chown(target, component['uid'], component['gid'])
        config = tomllib.loads(U.original(directory, r, Q.ETC / 'cyberherd-payout/config.toml').read_text())
        config['state_dir'] = str(child)
        Q.I.publish(carrier / 'config.toml', S.W.toml_text(config).encode(), 0o440)
        os.chown(carrier / 'config.toml', 0, component['gid'])
        source = directory / 'payload/cyberherd-payoutd'
        TX.copy_durable(source, carrier / source.name, TX.fingerprint(source))
        offline_fee_migration_child(carrier / source.name, carrier / 'config.toml', component)
        Q.no_role_processes({'payout': component})
        U.sources_match(m, r, observed)
        if (child / 'payout.db-journal').exists() or (child / 'payout.db-journal').is_symlink():
            raise ValueError('clone rollback journal requires inspection')
        for suffix in ('', '-wal', '-shm'):
            path = child / ('payout.db' + suffix)
            if not path.exists() and not path.is_symlink():
                if not suffix:
                    raise ValueError('missing migrated payout copy')
                continue
            info = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                    or info.st_uid != component['uid'] or info.st_gid != component['gid']
                    or stat.S_IMODE(info.st_mode) != 0o600
                    or info.st_size > r['baseline']['policy']['max_component_bytes']):
                raise ValueError('migrated payout copy identity changed')
        retained = directory / 'candidate-original-payout'
        retained.mkdir(mode=0o700)
        for suffix in ('', '-wal'):
            original = candidate / ('payout.db' + suffix)
            if original.exists():
                original.rename(retained / original.name)
            migrated = child / ('payout.db' + suffix)
            if migrated.exists():
                C.copy_raw_image(migrated, original, TX.fingerprint(migrated))
        TX.sync_directory(retained)
        TX.sync_directory(candidate)
        compared = inclusive_native(directory, 'compare-inclusive-fee-upgrade',
                                    '--bundle', directory / 'backup', '--candidate-copy', candidate)
        U.sources_match(m, r, observed)
        M.note(directory, 'CLONE-ACCEPTED.json', {'native': compared,
                'inclusive_fee_upgrade': True, 'backup_manifest_sha256': accepted['manifest_sha256'],
                'originals_unchanged': True, 'live_migration_performed': False})
        complete = True
    finally:
        carrier.chmod(0o700)
        if complete:
            shutil.rmtree(carrier)
        else:
            # Preserve a possibly partial migration privately for diagnosis.
            # All real writers remain held by the outer coordinator.
            M.note(directory, 'CLONE-HELD.json', {'carrier': str(carrier),
                    'inclusive_fee_upgrade': True, 'services_started': False})


def review_backup(directory, m, r, observed):
    result = json.loads(C.command(str(directory / 'payload/cyberherd-statectl'), '--config', str(S.MAINTENANCE),
        'review-rewards', '--bundle', str(directory / 'backup'), '--destination', str(directory / 'reward-review'), timeout=300))
    if (set(result) != {'operation', 'id', 'accepted', 'activation_authorized'}
            or result['operation'] != 'review_rewards' or result['accepted'] is not True
            or result['activation_authorized'] is not False or str(uuid.UUID(result['id'])) != result['id']):
        raise ValueError('native current-backup reward review refused')
    C.protected(directory / 'reward-review', directory=True, private=True)
    C.protected(directory / 'reward-review/REVIEW.json', private=True)
    value = C.read_json(directory / 'reward-review/REVIEW.json')
    manifest = C.read_json(directory / 'backup/MANIFEST.json')
    if ((directory / 'reward-review/INCOMPLETE').exists() or value['id'] != result['id']
            or value['bundle_id'] != manifest['id']
            or value['bundle_manifest_sha256'] != U.digest(directory / 'backup/MANIFEST.json')
            or value['owner'] != r['baseline']['policy']['owner'] or value['epoch'] != r['baseline']['policy']['epoch']
            or value['all_barriers_examined'] is not True or value['activation_authorized'] is not False
            or value['external_reconciliation_required'] is not True):
        raise ValueError('native current-backup reward review binding changed')
    U.sources_match(m, r, observed)
    return {'native': result, 'report_sha256': U.digest(directory / 'reward-review/REVIEW.json')}


def accepted_backup(directory, m, r, observed, installed):
    U.sources_match(m, r, observed, installed=installed)
    accepted = C.read_json(directory / 'BACKUP-ACCEPTED.json')
    if (accepted['originals'] != observed
            or U.digest(directory / 'backup/MANIFEST.json') != accepted['manifest_sha256']
            or original_verify(directory, 'verify', '--bundle', directory / 'backup') != accepted['native']
            or U.native(m, 'verify', '--bundle', directory / 'backup') != accepted['native']):
        raise ValueError('accepted fresh native backup changed')
    return accepted


def apply(directory, expected, approved):
    if not approved:
        raise ValueError('explicit exclusive maintenance approval required')
    m, r = load(directory, expected)
    with M.exclusive(directory):
        if any((directory / n).exists() for n in ('MAINTENANCE-STARTED.json', 'RESUME-REQUESTED.json', 'ROLLBACK-REQUESTED.json')):
            raise ValueError('maintenance already began; inspect rather than reapply')
        fresh(r['baseline'])
        if native_configuration_checks(directory, r) != r['native_configuration_checks']:
            raise ValueError('selected configuration acceptance changed')
        M.note(directory, 'MAINTENANCE-STARTED.json', {'exclusive_maintenance': True, 'activation_authorized': False,
                                                    'database_restore_authorized': False})
        U.quiesce(m, r)
        observed = U.state_files(r)
        original_verify(directory, 'snapshot', '--destination', directory / 'backup', '--writers-stopped')
        verified = original_verify(directory, 'verify', '--bundle', directory / 'backup')
        if U.native(m, 'verify', '--bundle', directory / 'backup') != verified:
            raise ValueError('selected native schema differs from the original accepted backup')
        U.sources_match(m, r, observed)
        M.note(directory, 'BACKUP-ACCEPTED.json', {'native': verified, 'originals': observed,
                                                'manifest_sha256': U.digest(directory / 'backup/MANIFEST.json')})
        U.clone_compare(directory, m, r, observed, current_schema=True)
        backlog = review_backup(directory, m, r, observed)
        accepted_backup(directory, m, r, observed, installed=False)
        TX.execute(directory / 'programs', lambda: U.sources_match(m, r, observed, installed=None))
        accepted_backup(directory, m, r, observed, installed=True)
        if native_configuration_checks(directory, r) != r['native_configuration_checks']:
            raise ValueError('selected worker configuration acceptance changed after replacement')
        M.note(directory, 'APPLIED.json', {'live_images': observed, 'reward_review': backlog,
            'current_schema_patch': True, 'schema_upgrade_authorized': False,
            'financial_images_unchanged': True, 'programs': 6, 'configurations_changed': False,
            'activation_authorized': False, 'services_started': False})
    return {'upgraded': True, 'programs': 6, 'current_four_journal_backup_verified': True,
            'reward_backlog_reviewed': True, 'financial_images_unchanged': True,
            'configurations_changed': False, 'services_started': False, 'activation_authorized': False}


def apply_inclusive_fee_upgrade(directory, expected, approved):
    """Separate schema transaction; no ordinary rollback/resume is applicable.

    Requires dedicated resumption and installed qualification before production
    use; ordinary program-only rollback/resume must never handle this review.
    """
    if not approved:
        raise ValueError('explicit exclusive schema maintenance approval required')
    m, r = load(directory, expected, inclusive_fee_upgrade=True)
    with M.exclusive(directory):
        if any((directory / name).exists() for name in
               ('MAINTENANCE-STARTED.json', 'RESUME-REQUESTED.json', 'ROLLBACK-REQUESTED.json')):
            raise ValueError('schema maintenance already began; preserve evidence and inspect')
        fresh(r['baseline'])
        if native_configuration_checks(directory, r) != r['native_configuration_checks']:
            raise ValueError('selected disabled configuration acceptance changed')
        M.note(directory, 'MAINTENANCE-STARTED.json', {'exclusive_maintenance': True,
                'inclusive_fee_upgrade': True, 'activation_authorized': False,
                'database_restore_authorized': False})
        U.quiesce(m, r)
        observed = inclusive_backup(directory, m, r)
        inclusive_clone_compare(directory, m, r, observed)
        backup = C.read_json(directory / 'BACKUP-ACCEPTED.json')
        clone = C.read_json(directory / 'CLONE-ACCEPTED.json')
        if (clone.get('inclusive_fee_upgrade') is not True
                or clone['backup_manifest_sha256'] != backup['manifest_sha256']
                or inclusive_native(directory, 'verify-inclusive-fee-upgrade-source',
                                    '--bundle', directory / 'backup') != backup['selected_source']
                or inclusive_native(directory, 'compare-inclusive-fee-upgrade',
                                    '--bundle', directory / 'backup',
                                    '--candidate-copy', directory / 'candidate') != clone['native']):
            raise ValueError('accepted source backup or migrated rehearsal changed')
        U.sources_match(m, r, observed)
        TX.execute(directory / 'programs', lambda: U.sources_match(m, r, observed, installed=None))
        U.sources_match(m, r, observed, installed=True)
        if native_configuration_checks(directory, r) != r['native_configuration_checks']:
            raise ValueError('selected worker/configuration changed before migration')
        M.note(directory, 'MIGRATION-STARTED.json', {'inclusive_fee_upgrade': True,
                'database_restore_authorized': False, 'old_binary_rollback_authorized': False})
        offline_fee_migration_child(Q.BIN / 'cyberherd-payoutd', Q.ETC / 'cyberherd-payout/config.toml',
                                    r['baseline']['policy']['components']['payout'])
        U.stopped(m, r, installed=True)
        after = U.state_files(r)
        inclusive_native(directory, 'snapshot', '--destination', directory / 'migrated', '--writers-stopped')
        verified = inclusive_native(directory, 'verify', '--bundle', directory / 'migrated')
        compared = inclusive_native(directory, 'compare-inclusive-fee-upgrade',
                                    '--bundle', directory / 'backup', '--candidate-copy', directory / 'migrated')
        U.sources_match(m, r, after, installed=True)
        M.note(directory, 'APPLIED.json', {'inclusive_fee_upgrade': True,
                'live_images': after, 'verified': verified, 'compared': compared,
                'backup_manifest_sha256': backup['manifest_sha256'],
                'migrated_manifest_sha256': U.digest(directory / 'migrated/MANIFEST.json'),
                'retained_rows_unchanged': True, 'configurations_changed': False,
                'activation_authorized': False, 'services_started': False})
    return {'upgraded': True, 'inclusive_fee_upgrade': True, 'four_journal_backup_verified': True,
            'clone_compared': True, 'live_migration_compared': True,
            'configurations_changed': False, 'services_started': False, 'activation_authorized': False}


def rollback(directory, expected, approved):
    if not approved:
        raise ValueError('explicit pre-resumption program rollback approval required')
    m, r = load(directory, expected)
    with M.exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists():
            raise ValueError('resumption already began; database or program rollback is not authorized')
        accepted = C.read_json(directory / 'BACKUP-ACCEPTED.json')
        accepted_backup(directory, m, r, accepted['originals'], installed=None)
        M.note(directory, 'ROLLBACK-REQUESTED.json', {'programs_only': True, 'database_restore_authorized': False})
        TX.execute(directory / 'programs', lambda: U.sources_match(m, r, accepted['originals'], installed=None), rollback=True)
        accepted_backup(directory, m, r, accepted['originals'], installed=False)
        M.note(directory, 'ROLLED-BACK.json', {'programs_restored': True, 'live_images': accepted['originals'],
                                             'database_images_restored': False, 'services_started': False})
    return {'programs_restored': True, 'database_images_restored': False, 'services_started': False}


def resume(directory, expected, approved, original=False):
    if not approved:
        raise ValueError('explicit service resumption approval required')
    m, r = load(directory, expected)
    with M.exclusive(directory):
        if (directory / 'RESUME-REQUESTED.json').exists():
            raise ValueError('resumption already began; inspect rather than restarting')
        if not original and (directory / 'ROLLBACK-REQUESTED.json').exists():
            raise ValueError('program rollback was selected; selected-program resumption refused')
        receipt = C.read_json(directory / ('ROLLED-BACK.json' if original else 'APPLIED.json'))
        accepted_backup(directory, m, r, receipt['live_images'], installed=not original)
        if not original:
            if (receipt.get('activation_authorized') is not False
                    or U.digest(directory / 'reward-review/REVIEW.json') != receipt['reward_review']['report_sha256']):
                raise ValueError('accepted program or private backlog receipt changed')
            if native_configuration_checks(directory, r) != r['native_configuration_checks']:
                raise ValueError('selected worker configuration acceptance changed before resumption')
        return resume_verified_services(directory, m, r, original=original)


def resume_verified_services(directory, m, r, *, original=False):
    """Shared start tail; caller must verify its own immutable accepted state."""
    # This durable fence precedes ALL unmask/start operations. Once written,
    # no code rollback or automatic repeated start is permitted.
    M.note(directory, 'RESUME-REQUESTED.json', {'approved': True, 'original_programs': original, 'payouts_enabled': False})
    M.restore_unit(U.original(directory, r, C.UNIT), r['baseline']['files'][str(C.UNIT)])
    mask = U.MASKS / 'cyberherd.service'
    if not M.masked(mask):
        raise ValueError('membership mask changed before resumption')
    mask.unlink()
    TX.sync_directory(mask.parent)
    C.command('/usr/bin/systemctl', 'daemon-reload')
    U.stable(m, r, installed=not original)
    for role in ('core', 'membership'):
        name = U.P.ROLE_NAMES[role] + '.service'
        value, before = C.status(name), r['baseline']['states'][role]
        if (value['FragmentPath'] != before['FragmentPath'] or value['DropInPaths'] != before['DropInPaths']
                or value['NeedDaemonReload'] != 'no'):
            raise ValueError('effective service changed before resumption')
        M.start_identity(value, 'lightning-goats' if role == 'core' else 'cyberherd')
    for name, user, program, health in (
            (C.UNIT_NAME, 'lightning-goats', 'lightning-goatsd', 'http://127.0.0.1:8787/healthz'),
            ('cyberherd.service', 'cyberherd', 'cyberherdd', 'http://127.0.0.1:3340/api/v1/cyberherd')):
        C.command('/usr/bin/systemctl', 'start', '--', name, timeout=120)
        M.running(name, user, Q.BIN / program)
        M.health(health)
    U.stable(m, r, installed=not original)
    for role in ('cyberherd-payout', 'cyberherd-signer'):
        value = C.status(role + '.service')
        if (not M.masked(U.MASKS / (role + '.service')) or value['MainPID'] != '0'
                or value['LoadState'] != 'masked' or value['UnitFileState'] != 'masked'
                or value['ActiveState'] != 'inactive' or value['SubState'] != 'dead'):
            raise ValueError('outgoing role changed during zero-budget resumption')
    M.note(directory, 'RESUMED.json', {'core_running': True, 'membership_running': True,
        'original_programs': original, 'payouts_enabled': False, 'configurations_changed': False})
    return {'core_running': True, 'membership_running': True, 'original_programs': original,
            'payouts_enabled': False, 'configurations_changed': False}


def resume_inclusive_fee_upgrade(directory, expected, approved):
    if not approved:
        raise ValueError('explicit schema service resumption approval required')
    m, r = load(directory, expected, inclusive_fee_upgrade=True)
    with M.exclusive(directory):
        if any((directory / name).exists() for name in ('RESUME-REQUESTED.json', 'ROLLBACK-REQUESTED.json')):
            raise ValueError('schema resumption or rollback already began; preserve evidence and inspect')
        receipt = C.read_json(directory / 'APPLIED.json')
        if (receipt.get('inclusive_fee_upgrade') is not True
                or receipt.get('activation_authorized') is not False
                or receipt.get('services_started') is not False
                or receipt.get('configurations_changed') is not False
                or receipt.get('retained_rows_unchanged') is not True):
            raise ValueError('accepted schema migration receipt required')
        U.sources_match(m, r, receipt['live_images'], installed=True)
        if (U.digest(directory / 'backup/MANIFEST.json') != receipt['backup_manifest_sha256']
                or U.digest(directory / 'migrated/MANIFEST.json') != receipt['migrated_manifest_sha256']
                or inclusive_native(directory, 'verify', '--bundle', directory / 'migrated') != receipt['verified']
                or inclusive_native(directory, 'compare-inclusive-fee-upgrade',
                                    '--bundle', directory / 'backup', '--candidate-copy', directory / 'migrated') != receipt['compared']):
            raise ValueError('accepted schema migration images or comparison changed')
        if native_configuration_checks(directory, r) != r['native_configuration_checks']:
            raise ValueError('selected disabled configuration acceptance changed')
        U.sources_match(m, r, receipt['live_images'], installed=True)
        result = resume_verified_services(directory, m, r)
        return dict(result, inclusive_fee_upgrade=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inclusive-fee-upgrade', action='store_true', help='prepare a distinct schema-upgrade review; does not apply it')
    commands = parser.add_subparsers(dest='operation', required=True)
    preparation = commands.add_parser('prepare')
    for name in ('archive', 'review_archive', 'source', 'archive_sha256', 'review_sha256', 'directory'):
        preparation.add_argument(name, type=Path if name in ('archive', 'review_archive', 'directory') else str)
    for operation, approval in (('apply', '--exclusive-maintenance'), ('apply-inclusive-fees', '--exclusive-maintenance'),
                                ('resume-inclusive-fees', '--approve-resumption'), ('rollback', '--approve-program-rollback'),
                                ('resume', '--approve-resumption'), ('resume-original', '--approve-resumption')):
        command = commands.add_parser(operation)
        command.add_argument('directory', type=Path)
        command.add_argument('--review-sha256', required=True)
        command.add_argument(approval, required=True, action='store_true')
    args = parser.parse_args()
    if args.inclusive_fee_upgrade and args.operation != 'prepare':
        raise ValueError('inclusive-fee upgrade application requires its separate reviewed coordinator')
    if args.operation == 'prepare':
        result = prepare(args.archive, args.review_archive, args.source, args.archive_sha256, args.review_sha256, args.directory, inclusive_fee_upgrade=args.inclusive_fee_upgrade)
    elif args.operation == 'apply-inclusive-fees':
        result = apply_inclusive_fee_upgrade(args.directory, args.review_sha256, args.exclusive_maintenance)
    elif args.operation == 'resume-inclusive-fees':
        result = resume_inclusive_fee_upgrade(args.directory, args.review_sha256, args.approve_resumption)
    elif args.operation == 'apply':
        result = apply(args.directory, args.review_sha256, args.exclusive_maintenance)
    elif args.operation == 'rollback':
        result = rollback(args.directory, args.review_sha256, args.approve_program_rollback)
    else:
        result = resume(args.directory, args.review_sha256, args.approve_resumption, args.operation == 'resume-original')
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
