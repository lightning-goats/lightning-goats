#!/usr/bin/env python3
"""List original external references from a fenced Cashu recovery checkpoint."""
import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path
import runpy
import sqlite3
import time
import uuid

HERE = Path(__file__).resolve().parent
RESTORE = runpy.run_path(str(HERE / 'stage-cashu-restore.py'))


def open_images(stage, expected, common, budget):
    common['protected_directory'](stage, private=True)
    common['absent'](stage / 'INCOMPLETE')
    read = common['COMBINED']['read_private']
    receipt_raw = read(stage / 'STAGED.json', 65536)
    receipt = common['document'](receipt_raw)
    raw = read(stage / 'FENCED_IMAGES.json')
    manifest = common['document'](raw)
    checkpoint = str(uuid.UUID(manifest['checkpoint_id']))
    if (common['sha'](receipt_raw) != expected
            or receipt['version'] != 1 or receipt['result'] != 'CASHU_RESTORE_STAGED'
            or receipt['restore_authorized'] is not False or receipt['resume_authorized'] is not False
            or receipt['fenced_images_sha256'] != common['sha'](raw)
            or manifest['version'] != 1 or manifest['checkpoint_id'] != checkpoint
            or receipt['checkpoint_id'] != checkpoint
            or receipt['ciphertext_sha256'] != manifest['ciphertext_sha256']
            or set(manifest['roles']) != common['ROLES']):
        raise ValueError('review requires the selected complete fenced stage')
    images = stage / 'fenced-images'
    common['protected_directory'](images, private=True)
    if set(p.name for p in images.iterdir()) != common['ROLES']:
        raise ValueError('staged image directory inventory differs')
    selected = {}
    for role, members in manifest['roles'].items():
        directory = images / role
        common['protected_directory'](directory, private=True)
        marker = ('CASHU_RECOVERY_REQUIRED' if role in ('wallet', 'cashu_signer')
                  else 'CYBERHERD_RECOVERY_REQUIRED')
        if read(directory / marker, 128) != (checkpoint + '\n').encode():
            raise ValueError('staged recovery fence differs')
        names = {marker}
        for member in members:
            path = RESTORE['relative_path'](member['name'])
            if len(path.parts) != 1 or path.name in names:
                raise ValueError('staged file name is not unique and local')
            names.add(path.name)
            with closing(common['Frozen'](directory / path, 0, 1024**3, budget)) as file:
                if file.digest != member['sha256'] or file.expected[6] != member['bytes']:
                    raise ValueError('staged image changed since verification')
            if role == 'core' or (role == 'wallet' and path.name == 'acquisition.sqlite'):
                if role in selected:
                    raise ValueError('staged journal is ambiguous')
                selected[role] = directory / path
        if {p.name for p in directory.iterdir()} != names:
            raise ValueError('staged role has unlisted files or sidecars')
    return checkpoint, selected['core'], selected['wallet']


CORE_TREASURY = """
    SELECT i.id, i.mint_id, i.intent_digest, i.deposit_sats AS expected_deposit_sats,
           u.receive_request_id, u.payment_hash, d.receive_id, d.deposit_sats,
           c.intent_id IS NOT NULL AS completed
      FROM cashu_treasury_intents i
      LEFT JOIN cashu_treasury_issuances u ON u.intent_id=i.id
      LEFT JOIN cashu_treasury_deposits d ON d.intent_id=i.id
      LEFT JOIN cashu_treasury_completions c ON c.intent_id=i.id
    ORDER BY i.id
"""
WALLET_TREASURY = """
    SELECT w.id, w.receive_id AS receive_request_id, w.payment_hash, w.state, q.quote_id,
           p.operation_id, r.intent_id IS NOT NULL AS report_retained
      FROM treasury_instructions w
      LEFT JOIN treasury_quotes q ON q.intent_id=w.id
      LEFT JOIN treasury_prepared p ON p.intent_id=w.id
      LEFT JOIN treasury_results r ON r.intent_id=w.id
    ORDER BY w.id
"""
FEED = "SELECT id,status,threshold_sats FROM feed_attempts ORDER BY id"
ACQUISITION = "SELECT id,mint_id,state,net FROM acquisitions ORDER BY id"
GROUPS = ('strike_receives', 'mint_melts', 'feeder_requests', 'cashu_acquisitions')


def open_database(path, budget):
    db = sqlite3.connect(path.as_uri() + '?mode=ro&immutable=1', uri=True)
    db.row_factory = sqlite3.Row
    db.execute('PRAGMA trusted_schema=OFF')
    db.execute('PRAGMA query_only=ON')
    db.set_progress_handler(lambda: int(time.monotonic() >= budget.deadline), 10000)
    return db


def same(left, right, fields):
    if left and right and any(left[field] != right[field] for field in fields
                             if left[field] is not None and right[field] is not None):
        raise ValueError('immutable original operation identity differs')


def compare_rows(older, later, immutable, pending, limit, after=''):
    """Merge sorted streams and keep a bounded batch and a full change count."""
    a, b = next(older, None), next(later, None) if later is not None else None
    count, rows = 0, []
    while a is not None or b is not None:
        if b is None or (a is not None and a['id'] < b['id']):
            previous, reference = dict(a), None
            a = next(older, None)
        elif a is None or b['id'] < a['id']:
            previous, reference = None, dict(b)
            b = next(later, None)
        else:
            previous, reference = dict(a), dict(b)
            a, b = next(older, None), next(later, None)
            same(previous, reference, immutable)
        if later is None:
            include = pending(previous)
        else:
            include = previous != reference or pending(previous)
        operation = previous if previous is not None else reference
        if include and operation['id'] > after:
            count += 1
            if len(rows) < limit:
                rows.append({'staged': previous, 'reference': reference})
    cursor = None
    if count > limit:
        last = rows[-1]
        cursor = (last['staged'] or last['reference'])['id']
    return {'total': count, 'next_after': cursor, 'items': rows}


def worklist(core, wallet, reference_core=None, reference_wallet=None, limit=100, group=None, after=''):
    def identity(c, w):
        return (c.execute('SELECT stream_id FROM overlay_identity WHERE singleton=1').fetchone()[0],
                w.execute('SELECT epoch FROM wallet_epoch WHERE singleton=1').fetchone()[0],
                tuple(w.execute('SELECT instance,key_binding FROM runtime_registration WHERE singleton=1').fetchone()))

    if reference_core is not None:
        if identity(core, wallet) != identity(reference_core, reference_wallet):
            raise ValueError('reference and staged wallet registration differ')
        sequence = 'SELECT COALESCE(MAX(seq),0) FROM event_log'
        if core.execute(sequence).fetchone()[0] > reference_core.execute(sequence).fetchone()[0]:
            raise ValueError('reference precedes the staged core event history')

    def diff(before, newer, sql, immutable, pending):
        reference = iter(newer.execute(sql)) if newer is not None else None
        return compare_rows(iter(before.execute(sql)), reference, immutable, pending, limit, after)

    actions = {
        'strike_receives': lambda: diff(core, reference_core, CORE_TREASURY,
                                ('mint_id', 'intent_digest', 'expected_deposit_sats',
                                 'receive_request_id', 'payment_hash', 'receive_id', 'deposit_sats'),
                                lambda row: row is not None and not row['completed']),
        'mint_melts': lambda: diff(wallet, reference_wallet, WALLET_TREASURY,
                           ('receive_request_id', 'payment_hash', 'quote_id', 'operation_id'),
                           lambda row: row is not None and not row['report_retained']),
        'feeder_requests': lambda: diff(core, reference_core, FEED, ('threshold_sats',),
                                lambda row: row is not None and row['status'] in ('intent_committed', 'unknown')),
        'cashu_acquisitions': lambda: diff(wallet, reference_wallet, ACQUISITION, ('mint_id', 'net'),
                                   lambda row: row is not None and row['state'] != 'credited'),
    }
    return {name: actions[name]() for name in (GROUPS if group is None else (group,))}


def review(args, common):
    budget = common['Budget'](0, 300)
    checkpoint, core, wallet = open_images(args.staged, args.staged_sha256, common, budget)
    reference_id = None
    with closing(open_database(core, budget)) as core_db, closing(open_database(wallet, budget)) as wallet_db:
        if args.reference is None:
            items = worklist(core_db, wallet_db, limit=args.limit, group=args.group, after=args.after)
        else:
            reference_id, reference_core, reference_wallet = open_images(
                args.reference, args.reference_sha256, common, budget)
            if checkpoint == reference_id:
                raise ValueError('reference must be an independently retained checkpoint')
            with (closing(open_database(reference_core, budget)) as core_ref,
                  closing(open_database(reference_wallet, budget)) as wallet_ref):
                items = worklist(core_db, wallet_db, core_ref, wallet_ref, args.limit, args.group, args.after)
    return {
        'result': 'CASHU_RESTORE_WORKLIST', 'checkpoint_id': checkpoint,
        'reference_checkpoint_id': reference_id, 'records': items,
        'external_reconciliation_verified': False, 'restore_authorized': False,
        'resume_authorized': False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('staged', type=Path)
    parser.add_argument('staged_sha256')
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--reference-sha256')
    parser.add_argument('--limit', type=int, default=100)
    parser.add_argument('--group', choices=GROUPS)
    parser.add_argument('--after', default='')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        verified_tool_identity()
        if (bool(args.reference) != bool(args.reference_sha256) or not 1 <= args.limit <= 1000
                or (args.after and (args.group is None or str(uuid.UUID(args.after)) != args.after))):
            raise ValueError('invalid reference or review limit')
        common = runpy.run_path(str(HERE / 'collect-cashu-backup.py'))
        common['protected_directory'](args.output.parent, private=True)
        if (not args.output.is_absolute() or args.output.is_relative_to(args.staged)
                or (args.reference is not None and args.output.is_relative_to(args.reference))):
            raise ValueError('review output must be outside both selected stages')
        result = review(args, common)
        common['write_json'](args.output, result)
        common['sync'](args.output.parent)
        print(json.dumps({'result': result['result'],
                          'counts': {kind: batch['total'] for kind, batch in result['records'].items()},
                          'resume_authorized': False}, sort_keys=True))
    except (ValueError, OSError, KeyError, TypeError, IndexError, sqlite3.Error):
        parser.exit(1, 'Cashu restore review refused. Preserve the fenced checkpoint.\n')


if __name__ == '__main__':
    if 'verified_tool_identity' not in globals():
        raise SystemExit('use the externally verified root-owned backup launcher')
    main()
