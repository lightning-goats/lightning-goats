#!/usr/bin/env python3
"""Verify Cashu custody inventory together with all four native service journals.

Read-only offline review. Never starts services, restores files, reconciles an
external effect or authorizes credits, spends, rewards or physical feeding.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import sqlite3
import stat
import subprocess
import tempfile
import uuid

HERE = Path(__file__).resolve().parent
CASHU = runpy.run_path(str(HERE / 'verify-cashu-backup.py'))


def read_private(path, maximum=1024 * 1024):
    info = CASHU['private_regular'](path)
    if info.st_size > maximum:
        raise ValueError('combined metadata exceeds bound')
    with path.open('rb') as source:
        raw = source.read(maximum + 1)
    if len(raw) > maximum:
        raise ValueError('combined metadata changed size')
    return raw


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def document(raw):
    return json.loads(raw, object_pairs_hook=CASHU['unique_fields'])


def validate_links(envelope, inventory, native):
    checkpoint = envelope['checkpoint_id']
    if (inventory['checkpoint_id'] != checkpoint or native['id'] != checkpoint
            or native['version'] != 2
            or native['purpose'] != 'cashu_four_journal_correlation_review'
            or native['cashu_inventory_sha256'] != envelope['cashu_inventory_sha256']
            or native['source_policy_sha256'] != envelope['maintenance_policy_sha256']
            or native['policy']['epoch'] != inventory['registrations']['core_epoch']):
        raise ValueError('combined checkpoint or native inventory link differs')
    core = [row for row in inventory['components'] if row['role'] == 'core_ledger']
    native_core = [row for row in native['images'] if row['name'] == 'core.db']
    if len(core) != 1 or len(native_core) != 1 or core[0]['sha256'] != native_core[0]['sha256']:
        raise ValueError('Cashu and native reviews require identical core image bytes')
    return core[0]['sha256']


def freeze_executable(source, destination, expected):
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    with os.fdopen(os.open(source, flags), 'rb') as reader:
        initial = os.fstat(reader.fileno())
        if not stat.S_ISREG(initial.st_mode) or not 0 < initial.st_size <= 512 * 1024 * 1024:
            raise ValueError('invalid selected native executable')
        checksum, total = hashlib.sha256(), 0
        with destination.open('xb') as writer:
            os.fchmod(writer.fileno(), 0o600)
            while chunk := reader.read(1024 * 1024):
                total += len(chunk)
                if total > initial.st_size:
                    raise ValueError('selected executable changed size')
                checksum.update(chunk)
                writer.write(chunk)
        final = os.fstat(reader.fileno())
        if (total != initial.st_size or checksum.hexdigest() != expected
                or (initial.st_size, initial.st_mtime_ns, initial.st_ctime_ns)
                != (final.st_size, final.st_mtime_ns, final.st_ctime_ns)):
            raise ValueError('selected executable changed or checksum differs')
    destination.chmod(0o500)


def verify(root, native_cli, native_sha256):
    if os.geteuid() != 0:
        raise ValueError('combined native review requires the local root operator')
    if root.absolute() != root.resolve(strict=True):
        raise ValueError('combined directory must be canonical')
    # The existing native verifier enforces protected ancestry for its bundle
    # and policy. Reject extra/unlisted material in the combined envelope too.
    for directory in [root, root / 'cashu', root / 'native']:
        info = directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o077:
            raise ValueError('combined directories must be private and root-owned')
    if {p.name for p in root.iterdir()} != {'combined.json', 'cashu', 'native', 'maintenance-policy.toml'}:
        raise ValueError('unlisted combined recovery material')
    raw = read_private(root / 'combined.json')
    envelope = document(raw)
    if (set(envelope) != {'version', 'checkpoint_id', 'cashu_inventory_sha256',
                          'native_manifest_sha256', 'maintenance_policy_sha256'}
            or type(envelope['version']) is not int or envelope['version'] != 1):
        raise ValueError('unsupported combined recovery envelope')
    checkpoint = envelope['checkpoint_id']
    if not isinstance(checkpoint, str) or str(uuid.UUID(checkpoint)) != checkpoint or not uuid.UUID(checkpoint).int:
        raise ValueError('invalid combined checkpoint')
    retained = {}
    for key, relative in [('cashu_inventory_sha256', 'cashu/inventory.json'),
                          ('native_manifest_sha256', 'native/MANIFEST.json'),
                          ('maintenance_policy_sha256', 'maintenance-policy.toml')]:
        value = read_private(root / relative)
        if digest(value) != envelope[key]:
            raise ValueError('combined component metadata checksum differs')
        retained[relative] = value
    inventory = document(retained['cashu/inventory.json'])
    native = document(retained['native/MANIFEST.json'])
    core_digest = validate_links(envelope, inventory, native)
    cashu_result = CASHU['verify'](root / 'cashu')
    if cashu_result['result'] != 'CASHU_BACKUP_INVENTORY_VERIFIED':
        raise ValueError('Cashu inventory review did not complete')
    source = native_cli.resolve(strict=True)
    info = source.stat()
    if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= 512 * 1024 * 1024:
        raise ValueError('invalid selected native maintenance executable')
    with tempfile.TemporaryDirectory(prefix='lg-cashu-combined-review-', dir='/run') as temporary:
        temporary = Path(temporary)
        program = temporary / 'statectl'
        freeze_executable(source, program, native_sha256)
        report_path = temporary / 'review.json'
        result = subprocess.run([str(program), '--config', str(root / 'maintenance-policy.toml'),
                                 'review-cashu-correlations', '--bundle', str(root / 'native'),
                                 '--destination', str(report_path)],
                                env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=120)
        if result.returncode or len(result.stdout) > 65536 or len(result.stderr) > 65536:
            raise ValueError('native correlation review refused; inspect privately')
        review = document(read_private(report_path))
        if (review['version'] != 1 or review['id'] != checkpoint
                or review['input_manifest_sha256'] != envelope['native_manifest_sha256']
                or review['policy_sha256'] != envelope['maintenance_policy_sha256']
                or review['cashu_inventory_sha256'] != envelope['cashu_inventory_sha256']
                or review['core_image_sha256'] != core_digest
                or review['cashu_wallet_verified'] is not False
                or review['combined_restore_authorized'] is not False
                or review['resume_authorized'] is not False
                or review['four_journal_correlations']['resume_authorized'] is not False):
            raise ValueError('native correlation report scope differs')
    if read_private(root / 'combined.json') != raw or any(
            read_private(root / relative) != value for relative, value in retained.items()):
        raise ValueError('combined metadata changed during verification')
    return {'result': 'CASHU_COMBINED_BACKUP_REVIEW_VERIFIED',
            'checkpoint_id': checkpoint, 'combined_manifest_sha256': digest(raw),
            'native_cli_sha256': native_sha256, 'core_image_sha256': core_digest,
            'cashu_inventory_result': cashu_result,
            'native_four_journal_correlations_verified': True,
            'identical_core_image_verified': True,
            'external_reconciliation_verified': False, 'restore_authorized': False,
            'resume_authorized': False, 'services_changed': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('--native-cli', type=Path, required=True)
    parser.add_argument('--native-cli-sha256', required=True)
    args = parser.parse_args()
    try:
        result = verify(args.snapshot, args.native_cli, args.native_cli_sha256)
        print(json.dumps(result, sort_keys=True))
    except (ValueError, OSError, KeyError, TypeError, sqlite3.Error, subprocess.SubprocessError):
        parser.exit(1, 'Combined Cashu recovery review rejected; inspect privately.\n')


if __name__ == '__main__':
    main()
