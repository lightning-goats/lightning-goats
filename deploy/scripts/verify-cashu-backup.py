#!/usr/bin/env python3
"""Verify a private offline Cashu snapshot inventory; never authorize recovery."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import stat
import tempfile
import uuid

REQUIRED = {'core_ledger', 'wallet_journal', 'cdk_wallet', 'signing_broker',
            'wallet_seed', 'receiving_key', 'protected_policy'}
DATABASES = {'core_ledger', 'wallet_journal', 'cdk_wallet', 'signing_broker', 'cdk_mint_wallet'}
ALLOWED = REQUIRED | {'cdk_mint_wallet'}


def unique_fields(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate inventory field')
        result[key] = value
    return result


def private_metadata(info):
    if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
            or info.st_uid != os.geteuid() or info.st_mode & 0o077):
        raise ValueError('snapshot requires private regular files owned by verifier')
    return info


def private_regular(path):
    return private_metadata(path.lstat())


def freeze_component(path, destination, expected_checksum):
    """Hash a bounded, stable file into a private disposable copy."""
    initial = private_regular(path)
    if not 0 < initial.st_size <= 1024 * 1024 * 1024:
        raise ValueError('component size exceeds bound')
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    with os.fdopen(os.open(path, flags), 'rb') as source:
        before = private_metadata(os.fstat(source.fileno()))
        if (before.st_dev, before.st_ino) != (initial.st_dev, initial.st_ino):
            raise ValueError('component changed during open')
        checksum = hashlib.sha256()
        total = 0
        output_flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC
        with os.fdopen(os.open(destination, output_flags, 0o600), 'wb') as output:
            while chunk := source.read(1024 * 1024):
                total += len(chunk)
                if total > before.st_size or total > 1024 * 1024 * 1024:
                    raise ValueError('component changed size or exceeded bound')
                checksum.update(chunk)
                output.write(chunk)
        after = os.fstat(source.fileno())
        identity = lambda info: (info.st_dev, info.st_ino, info.st_size,
                                 info.st_mtime_ns, info.st_ctime_ns)
        if (identity(before) != identity(after) or total != before.st_size
                or checksum.hexdigest() != expected_checksum):
            raise ValueError('component changed or checksum mismatch')
    return total


def registration(connection, role, expected):
    if role == 'core_ledger':
        actual = connection.execute('SELECT stream_id FROM overlay_identity WHERE singleton=1').fetchall()
        if actual != [(expected['core_epoch'],)]:
            raise ValueError('core epoch mismatch')
    elif role == 'wallet_journal':
        epoch = connection.execute('SELECT epoch FROM wallet_epoch WHERE singleton=1').fetchall()
        instance = connection.execute('SELECT instance FROM runtime_registration WHERE singleton=1').fetchall()
        if epoch != [(expected['wallet_epoch'],)] or instance != [(expected['wallet_instance'],)]:
            raise ValueError('wallet registration mismatch')
    elif role == 'signing_broker':
        rows = connection.execute('SELECT binding FROM registration WHERE singleton=1').fetchall()
        if len(rows) != 1:
            raise ValueError('missing signing registration')
        binding = json.loads(rows[0][0])
        if (not isinstance(binding, list) or len(binding) != 6
                or binding[2:4] != [expected['wallet_instance'], expected['wallet_epoch']]):
            raise ValueError('signer wallet registration mismatch')


def receipt_agreement(databases, expected):
    with closing(sqlite3.connect(databases['core_ledger'].as_uri() + '?mode=ro&immutable=1', uri=True)) as core, closing(sqlite3.connect(databases['wallet_journal'].as_uri() + '?mode=ro&immutable=1', uri=True)) as wallet:
        checked = 0
        for identity, raw in wallet.execute("SELECT id,core_ack FROM acquisitions WHERE state='credited'"):
            if raw is None or len(raw) > 4096:
                raise ValueError('missing retained core acknowledgment')
            ack = json.loads(raw, object_pairs_hook=unique_fields)
            if (ack['version'] != 1 or ack['core_epoch'] != expected['core_epoch']
                    or ack['wallet_instance'] != expected['wallet_instance']
                    or ack['wallet_epoch'] != expected['wallet_epoch']
                    or ack['result']['acquisition_id'] != identity):
                raise ValueError('acknowledgment registration mismatch')
            rows = core.execute(
                'SELECT receipt_digest,event_seq,net_sats,original_feed_credit_sats FROM cashu_receipt_inbox WHERE wallet_instance=? AND acquisition_id=? AND wallet_epoch=? AND core_epoch=?',
                [expected['wallet_instance'], identity, expected['wallet_epoch'], expected['core_epoch']],
            ).fetchall()
            result = ack['result']
            if rows != [(result['receipt_digest'], result['event_seq'], result['net_sats'], result['original_feed_credit_sats'])]:
                raise ValueError('credited wallet acknowledgment differs from core snapshot')
            checked += 1
        for identity, net, evidence in core.execute(
            'SELECT acquisition_id,net_sats,evidence_digest FROM cashu_receipt_inbox WHERE wallet_instance=? AND wallet_epoch=? AND core_epoch=?',
            [expected['wallet_instance'], expected['wallet_epoch'], expected['core_epoch']],
        ):
            rows = wallet.execute('SELECT state,net,acquired_evidence FROM acquisitions WHERE id=?', [identity]).fetchall()
            if (len(rows) != 1 or rows[0][0] not in ('credit_export_pending', 'credited')
                    or rows[0][1] != net or not isinstance(rows[0][2], bytes)
                    or rows[0][2].hex() != evidence):
                raise ValueError('core import missing matching acquired wallet history')
        for identity, raw, intent_raw in core.execute(
            'SELECT r.intent_id,r.result_json,i.intent_json FROM cashu_treasury_wallet_results r LEFT JOIN cashu_treasury_intents i ON i.id=r.intent_id'
        ):
            if intent_raw is None:
                raise ValueError('treasury result lacks retained intent')
            intent = json.loads(intent_raw, object_pairs_hook=unique_fields)
            if any(intent[key] != expected[key] for key in expected):
                continue
            reports = wallet.execute('SELECT report FROM treasury_results WHERE intent_id=?', [identity]).fetchall()
            if (len(reports) != 1 or len(reports[0][0]) > 65536 or len(raw) > 65536
                    or json.loads(raw, object_pairs_hook=unique_fields)
                    != json.loads(reports[0][0], object_pairs_hook=unique_fields)):
                raise ValueError('core treasury result missing matching wallet report')
        return checked


def namespace_agreement(databases, namespaces):
    with closing(sqlite3.connect(databases['wallet_journal'].as_uri() + '?mode=ro&immutable=1', uri=True)) as journal:
        layout = journal.execute("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cdk_storage_layout'").fetchone()[0]
        if not layout:
            if namespaces:
                raise ValueError('mint stores lack a journal namespace layout')
            return
        if journal.execute('SELECT version FROM cdk_storage_layout WHERE singleton=1').fetchall() != [(1,)]:
            raise ValueError('invalid namespace layout')
        retained = journal.execute('SELECT mint_id,mint_url,ready FROM cdk_mint_namespaces').fetchall()
        if {r[0] for r in retained} != set(namespaces) or any(r[2] != 1 for r in retained):
            raise ValueError('missing or incomplete retained mint namespace')
        for mint_id, mint_url, _ in retained:
            path, declared_url = namespaces[mint_id]
            if mint_url != declared_url:
                raise ValueError('inventory mint URL differs from journal')
            with closing(sqlite3.connect(path.as_uri() + '?mode=ro&immutable=1', uri=True)) as store:
                if store.execute('SELECT mint_id,mint_url FROM lg_mint_namespace WHERE singleton=1').fetchall() != [(mint_id,mint_url)]:
                    raise ValueError('CDK namespace differs from journal binding')
        with closing(sqlite3.connect(databases['cdk_wallet'].as_uri() + '?mode=ro&immutable=1', uri=True)) as legacy:
            if legacy.execute('SELECT (SELECT COUNT(*) FROM proof)+(SELECT COUNT(*) FROM wallet_sagas)').fetchone()[0]:
                raise ValueError('legacy shared-store custody requires reconciliation')


def traversal_error(error):
    raise error


def verify(root):
    if root.absolute() != root.resolve(strict=True):
        raise ValueError('snapshot directory must be canonical')
    for directory, dirs, files in os.walk(root, followlinks=False, onerror=traversal_error):
        info = Path(directory).lstat()
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid()
                or info.st_mode & 0o077):
            raise ValueError('snapshot directories must be private and owned by verifier')
        for name in dirs + files:
            entry = (Path(directory) / name).lstat()
            if not (stat.S_ISREG(entry.st_mode) or stat.S_ISDIR(entry.st_mode)):
                raise ValueError('snapshot aliases and special entries are forbidden')
    manifest_path = root / 'inventory.json'
    if private_regular(manifest_path).st_size > 1024 * 1024:
        raise ValueError('inventory exceeds bound')
    manifest = json.loads(manifest_path.read_bytes(), object_pairs_hook=unique_fields)
    if (set(manifest) != {'version', 'checkpoint_id', 'registrations', 'components'}
            or type(manifest['version']) is not int or manifest['version'] != 1):
        raise ValueError('unsupported inventory')
    if (not isinstance(manifest['checkpoint_id'], str)
            or uuid.UUID(manifest['checkpoint_id']).int == 0):
        raise ValueError('missing checkpoint identity')
    expected = manifest['registrations']
    if not isinstance(expected, dict) or set(expected) != {'core_epoch', 'wallet_instance', 'wallet_epoch'}:
        raise ValueError('invalid expected registrations')
    for value in expected.values():
        if not isinstance(value, str) or str(uuid.UUID(value)) != value or uuid.UUID(value).int == 0:
            raise ValueError('invalid expected registration identity')
    rows = manifest['components']
    if not isinstance(rows, list) or not 7 <= len(rows) <= 256:
        raise ValueError('invalid component inventory')
    roles, paths = set(), set()
    databases = {}
    namespaces = {}
    with tempfile.TemporaryDirectory(prefix='lg-cashu-backup-check-') as temporary:
        for index, row in enumerate(rows):
            role = row['role']
            fields = {'role', 'path', 'sha256'} | ({'mint_id', 'mint_url'} if role == 'cdk_mint_wallet' else set())
            if set(row) != fields or role not in ALLOWED:
                raise ValueError('unknown component fields or role')
            relative = Path(row['path'])
            if (relative.is_absolute() or '..' in relative.parts or str(relative) != row['path']
                    or relative == Path('inventory.json') or relative in paths):
                raise ValueError('invalid or duplicate component path')
            if role not in {'protected_policy', 'cdk_mint_wallet'} and role in roles:
                raise ValueError('duplicate component role')
            path = root / relative
            copy = Path(temporary) / str(index)
            size = freeze_component(path, copy, row['sha256'])
            if role in {'wallet_seed', 'receiving_key'} and size != (64 if role == 'wallet_seed' else 32):
                raise ValueError('key component size mismatch')
            if role in DATABASES:
                # Validate frozen bytes, never open the source snapshot as a writer.
                if role == 'cdk_mint_wallet':
                    if not isinstance(row['mint_id'],str) or not row['mint_id'] or row['mint_id'] in namespaces:
                        raise ValueError('duplicate or invalid mint namespace')
                    namespaces[row['mint_id']] = (copy,row['mint_url'])
                else:
                    databases[role] = copy
                with closing(sqlite3.connect(copy.as_uri() + '?mode=ro&immutable=1', uri=True)) as connection:
                    connection.execute('PRAGMA trusted_schema=OFF')
                    connection.execute('PRAGMA query_only=ON')
                    result = connection.execute('PRAGMA integrity_check').fetchall()
                    if result != [('ok',)]:
                        raise ValueError('database integrity check failed')
                    if connection.execute('PRAGMA foreign_key_check').fetchone() is not None:
                        raise ValueError('database foreign-key relationship check failed')
                    registration(connection, role, expected)
            roles.add(role)
            paths.add(relative)
        acknowledged_receipts = receipt_agreement(databases, expected)
        namespace_agreement(databases, namespaces)
    actual = {p.relative_to(root) for p in root.rglob('*') if p.is_file()}
    if not REQUIRED.issubset(roles) or actual != paths | {Path('inventory.json')}:
        raise ValueError('missing component or unlisted snapshot file/sidecar')
    return {'result': 'CASHU_BACKUP_INVENTORY_VERIFIED', 'components': len(rows), 'mint_stores_checked': len(namespaces), 'acknowledged_receipts_checked': acknowledged_receipts,
            'external_reconciliation_verified': False, 'restore_authorized': False,
            'source_databases_opened': False, 'core_wallet_signer_registration_agrees': True,
            'database_foreign_keys_checked': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('snapshot', type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(verify(args.snapshot), sort_keys=True))
    except (ValueError, OSError, sqlite3.Error, KeyError, TypeError):
        parser.exit(1, 'Cashu backup inventory rejected; inspect privately.\n')


if __name__ == '__main__':
    main()
