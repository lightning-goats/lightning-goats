#!/usr/bin/env python3
"""List original external references from a fenced Cashu recovery checkpoint."""
import argparse
from contextlib import closing, contextmanager
from itertools import zip_longest
import json
from pathlib import Path
import re
import runpy
import sqlite3
import time
import uuid

HERE = Path(__file__).resolve().parent
RESTORE = runpy.run_path(str(HERE / 'stage-cashu-restore.py'))


def open_images(stage, expected, common, budget, mint_store=None):
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
                if role == 'wallet' and path.name == mint_store:
                    selected['mint'] = file.digest
            if role == 'core' or (role == 'wallet' and path.name == 'acquisition.sqlite'):
                if role in selected:
                    raise ValueError('staged journal is ambiguous')
                selected[role] = directory / path
        if {p.name for p in directory.iterdir()} != names:
            raise ValueError('staged role has unlisted files or sidecars')
    paths = (checkpoint, selected['core'], selected['wallet'])
    return paths if mint_store is None else (*paths, selected['mint'])


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
STRIKE_SELECTION = """
    SELECT i.intent_json,i.intent_digest,i.deposit_sats,u.receive_request_id,
           u.payment_hash,r.payment_hash AS invoice_hash,r.invoice,r.amount_msat,
           r.address_user,r.credit_pool,r.description_hash,d.receive_id,
           d.payment_hash AS deposit_hash,d.deposit_sats AS verified_deposit_sats,o.stream_id
      FROM cashu_treasury_intents i
      JOIN cashu_treasury_issuances u ON u.intent_id=i.id
      JOIN strike_receive_requests r ON r.receive_request_id=u.receive_request_id
      JOIN overlay_identity o ON o.singleton=1
      LEFT JOIN cashu_treasury_deposits d ON d.intent_id=i.id
     WHERE i.id=?
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
MINT_SELECTION = """
    SELECT i.issued_json,i.receive_id,i.payment_hash,q.mint_id,q.quote_id,q.quote_json,
           p.operation_id,m.derivation,e.epoch,r.instance
      FROM treasury_instructions i
      JOIN treasury_quotes q ON q.intent_id=i.id
      JOIN treasury_prepared p ON p.intent_id=i.id
      JOIN treasury_melt_requests m ON m.intent_id=i.id
      JOIN wallet_epoch e ON e.singleton=1
      JOIN runtime_registration r ON r.singleton=1
     WHERE i.id=? AND i.state IN ('submitted','reconciled')
       AND length(i.issued_json)<=65536 AND length(q.quote_json)<=65536 AND length(m.derivation)<=1048576
"""
WALLET_SPLITS = """
    SELECT r.intent_id AS id, r.split_id, q.mint_id, p.cdk_operation,
           c.intent_id IS NOT NULL AS execution_claimed,
           s.submitted_at, CASE WHEN s.request_digest IS NOT NULL
                               THEN lower(hex(s.request_digest)) END AS submitted_request_digest,
           v.intent_id IS NOT NULL AS reply_retained, f.intent_id IS NOT NULL AS result_retained
      FROM treasury_split_reservations r
      LEFT JOIN treasury_quotes q ON q.intent_id=r.intent_id
      LEFT JOIN treasury_split_execution_claims c ON c.intent_id=r.intent_id
      LEFT JOIN treasury_split_requests p ON p.intent_id=r.intent_id
      LEFT JOIN treasury_split_submissions s ON s.intent_id=r.intent_id
      LEFT JOIN treasury_split_replies v ON v.intent_id=r.intent_id
      LEFT JOIN treasury_split_results f ON f.intent_id=r.intent_id
    ORDER BY r.intent_id
"""
FEED = "SELECT id,status,threshold_sats FROM feed_attempts ORDER BY id"
ACQUISITION = "SELECT id,mint_id,state,net FROM acquisitions ORDER BY id"
GROUPS = ('strike_receives', 'mint_melts', 'mint_splits', 'feeder_requests', 'cashu_acquisitions')


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


def require_event_prefix(staged, reference):
    end = staged.execute('SELECT COALESCE(MAX(seq),0) FROM event_log').fetchone()[0]
    events = 'SELECT seq,event_type,payload_json,created_at FROM event_log WHERE seq<=? ORDER BY seq'
    if any(before != later for before, later in zip_longest(
            staged.execute(events, (end,)), reference.execute(events, (end,)))):
        raise ValueError('reference does not preserve the staged core event history')


def select_strike(core, checkpoint, receipt_sha, intent_id, receive_id, decode):
    row = core.execute(STRIKE_SELECTION, (intent_id,)).fetchone()
    if (row is None or row['address_user'] != 'cashu_treasury' or row['credit_pool'] != 'cashu_treasury'
            or row['payment_hash'] != row['invoice_hash'] or row['intent_digest'] != row['description_hash']
            or row['amount_msat'] != row['deposit_sats'] * 1000):
        raise ValueError('original treasury invoice is missing or contradicts its issuance')
    retained = row['receive_id']
    if retained is not None and (row['deposit_hash'] != row['payment_hash']
                                 or row['verified_deposit_sats'] != row['deposit_sats']):
        raise ValueError('retained Strike deposit contradicts original invoice')
    if retained is not None and receive_id is not None and retained != receive_id:
        raise ValueError('requested receive differs from retained original')
    receive_id = retained or receive_id
    if receive_id is None or str(uuid.UUID(receive_id)) != receive_id or not uuid.UUID(receive_id).int:
        raise ValueError('an exact original provider receive ID is required')
    intent = decode(row['intent_json'])
    if (intent['id'] != intent_id or intent['deposit_sats'] != row['deposit_sats']
            or intent['core_epoch'] != row['stream_id']):
        raise ValueError('stored intent differs from original invoice binding')
    return {
        'version': 1, 'purpose': 'cashu_restore_strike_receive', 'checkpoint_id': checkpoint,
        'staged_receipt_sha256': receipt_sha, 'receive_id': receive_id,
        'intent_digest': row['intent_digest'], 'issued': {
            'intent': intent, 'receive_request_id': row['receive_request_id'],
            'payment_hash': row['payment_hash'], 'invoice': row['invoice'],
        },
    }


def select_mint(wallet, checkpoint, receipt_sha, intent_id, decode):
    row = wallet.execute(MINT_SELECTION, (intent_id,)).fetchone()
    if row is None:
        raise ValueError('original submitted mint quote is missing')
    issued, quote, derivation = (decode(row[field]) for field in ('issued_json', 'quote_json', 'derivation'))
    intent = issued['intent']
    if (intent['id'] != intent_id or intent['mint_id'] != row['mint_id']
            or intent['wallet_instance'] != row['instance'] or intent['wallet_epoch'] != row['epoch']
            or issued['receive_request_id'] != row['receive_id'] or issued['payment_hash'] != row['payment_hash']
            or quote['id'] != row['quote_id'] or quote['request'] != issued['invoice']
            or derivation['quote_id'] != row['quote_id'] or derivation['mint_url'] != quote['mint_url']
            or derivation['cdk_operation'] != row['operation_id']):
        raise ValueError('submitted mint quote differs from the original wallet binding')
    return {
        'version': 1, 'purpose': 'cashu_restore_mint_melt', 'checkpoint_id': checkpoint,
        'staged_receipt_sha256': receipt_sha, 'operation_id': row['operation_id'], 'issued': issued,
        'quote': {field: quote[field] for field in ('id', 'mint_url', 'unit', 'amount', 'request',
                                                    'fee_reserve', 'expiry', 'payment_method', 'state')},
    }


def select_feeder(core, checkpoint, receipt_sha, request_id):
    row = core.execute('SELECT status,threshold_sats FROM feed_attempts WHERE id=?', (request_id,)).fetchone()
    if row is None:
        raise ValueError('original feeder request is missing')
    return {'version': 1, 'purpose': 'cashu_restore_feeder_request', 'checkpoint_id': checkpoint,
            'staged_receipt_sha256': receipt_sha, 'request_id': request_id,
            'checkpoint_status': row['status'], 'threshold_sats': row['threshold_sats']}


def custody_total(store, mint_url):
    total, invalid = store.execute("""
        SELECT COUNT(*), COALESCE(MAX(CASE WHEN typeof(y)!='blob' OR length(y)!=33
                 OR unit!='sat' OR rtrim(mint_url,'/')!=? OR typeof(amount)!='integer' OR amount<=0
                 THEN 1 ELSE 0 END),0)
          FROM proof WHERE state!='SPENT'
    """, (mint_url.rstrip('/'),)).fetchone()
    if invalid:
        raise ValueError('retained proof metadata differs from the original sat mint namespace')
    return total


def custody_page(store, mint, mint_url, checkpoint, receipt_sha, store_sha, after=None, total=None):
    if after is not None and not re.fullmatch(r'0[23][0-9a-f]{64}', after):
        raise ValueError('invalid custody cursor')
    cursor = bytes.fromhex(after) if after is not None else b''
    if total is None:
        total = custody_total(store, mint_url)
    if after is not None and store.execute("SELECT 1 FROM proof WHERE y=? AND state!='SPENT'", (cursor,)).fetchone() is None:
        raise ValueError('custody cursor does not belong to this checkpoint')
    rows = store.execute("SELECT y,amount,state FROM proof WHERE state!='SPENT' AND y>? ORDER BY y LIMIT 129", (cursor,)).fetchall()
    proofs = []
    for y, amount, state in rows:
        if (type(y) is not bytes or not re.fullmatch(r'0[23][0-9a-f]{64}', y.hex())
                or type(amount) is not int or amount <= 0
                or state not in ('UNSPENT', 'RESERVED', 'PENDING', 'UNKNOWN', 'PENDING_SPENT')):
            raise ValueError('invalid retained CDK proof metadata')
        proofs.append({'y': y.hex(), 'amount_sats': amount, 'local_state': state})
    return {'version': 1, 'purpose': 'cashu_restore_mint_custody', 'checkpoint_id': checkpoint,
            'staged_receipt_sha256': receipt_sha, 'mint_id': mint, 'mint_url': mint_url,
            'mint_store_sha256': store_sha, 'total_candidates': total, 'after_y': after,
            'next_after_y': proofs[127]['y'] if len(proofs) == 129 else None, 'proofs': proofs[:128]}


def mint_store_name(mint, common):
    if not isinstance(mint, str) or not 1 <= len(mint) <= 256:
        raise ValueError('invalid mint namespace ID')
    return 'mint-' + common['sha'](b'lightning-goats/cdk-mint-store/v1\0' + mint.encode()) + '.sqlite'


@contextmanager
def open_custody_store(wallet_path, mint, expected_store_sha, common, budget):
    with closing(open_database(wallet_path, budget)) as wallet:
        rows = wallet.execute('SELECT mint_url,ready FROM cdk_mint_namespaces WHERE mint_id=?', (mint,)).fetchall()
    if len(rows) != 1 or rows[0]['ready'] != 1:
        raise ValueError('mint custody requires one initialized namespace')
    mint_url = rows[0]['mint_url']
    path = wallet_path.parent / mint_store_name(mint, common)
    with closing(common['Frozen'](path, 0, 1024**3, budget)) as original, closing(open_database(path, budget)) as store:
        if original.digest != expected_store_sha:
            raise ValueError('mint custody store changed after reading stage manifest')
        namespaces = store.execute('SELECT mint_id,mint_url FROM lg_mint_namespace WHERE singleton=1').fetchall()
        if [tuple(row) for row in namespaces] != [(mint, mint_url)]:
            raise ValueError('mint custody store differs from retained namespace')
        yield store, mint_url
        original.check()


def worklist(core, wallet, reference_core=None, reference_wallet=None, limit=100, group=None, after=''):
    def identity(c, w):
        return (c.execute('SELECT stream_id FROM overlay_identity WHERE singleton=1').fetchone()[0],
                w.execute('SELECT epoch FROM wallet_epoch WHERE singleton=1').fetchone()[0],
                tuple(w.execute('SELECT instance,key_binding FROM runtime_registration WHERE singleton=1').fetchone()))

    if reference_core is not None:
        if identity(core, wallet) != identity(reference_core, reference_wallet):
            raise ValueError('reference and staged wallet registration differ')
        require_event_prefix(core, reference_core)

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
        'mint_splits': lambda: diff(wallet, reference_wallet, WALLET_SPLITS,
                            ('split_id', 'mint_id', 'cdk_operation', 'submitted_at',
                             'submitted_request_digest'),
                            lambda row: row is not None and not row['result_retained']),
        'feeder_requests': lambda: diff(core, reference_core, FEED, ('threshold_sats',),
                                lambda row: row is not None and row['status'] in ('intent_committed', 'unknown')),
        'cashu_acquisitions': lambda: diff(wallet, reference_wallet, ACQUISITION, ('mint_id', 'net'),
                                   lambda row: row is not None and row['state'] != 'credited'),
    }
    return {name: actions[name]() for name in (GROUPS if group is None else (group,))}


def review(args, common):
    budget = common['Budget'](0, 300)
    if args.custody_mint is not None:
        name = mint_store_name(args.custody_mint, common)
        checkpoint, _, wallet, store_sha = open_images(args.staged, args.staged_sha256, common, budget, name)
        with open_custody_store(wallet, args.custody_mint, store_sha, common, budget) as (store, mint_url):
            return custody_page(store, args.custody_mint, mint_url, checkpoint,
                                args.staged_sha256, store_sha, args.custody_after)
    checkpoint, core, wallet = open_images(args.staged, args.staged_sha256, common, budget)
    if args.strike_intent is not None:
        with closing(open_database(core, budget)) as core_db:
            return select_strike(core_db, checkpoint, args.staged_sha256, args.strike_intent,
                                 args.receive_id, common['document'])
    if args.mint_intent is not None:
        with closing(open_database(wallet, budget)) as wallet_db:
            return select_mint(wallet_db, checkpoint, args.staged_sha256, args.mint_intent, common['document'])
    if args.feeder_request is not None:
        with closing(open_database(core, budget)) as core_db:
            return select_feeder(core_db, checkpoint, args.staged_sha256, args.feeder_request)
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
        'version': 1, 'result': 'CASHU_RESTORE_WORKLIST', 'checkpoint_id': checkpoint,
        'staged_receipt_sha256': args.staged_sha256, 'reference_checkpoint_id': reference_id,
        'reference_receipt_sha256': args.reference_sha256,
        'selection': {'group': args.group, 'after': args.after or None, 'limit': args.limit},
        'records': items,
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
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument('--strike-intent', type=uuid.UUID, help='select one original treasury invoice for provider verification')
    selection.add_argument('--mint-intent', type=uuid.UUID, help='select one original submitted mint quote')
    selection.add_argument('--feeder-request', type=uuid.UUID, help='select one original feeder operation')
    selection.add_argument('--custody-mint', help='select one mint namespace for a bounded custody proof check')
    parser.add_argument('--custody-after', help='next_after_y from the preceding custody page')
    parser.add_argument('--receive-id', type=uuid.UUID, help='original provider receive ID if it is absent from the stage')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        verified_tool_identity()
        if (bool(args.reference) != bool(args.reference_sha256) or not 1 <= args.limit <= 1000
                or (args.after and (args.group is None or str(uuid.UUID(args.after)) != args.after))):
            raise ValueError('invalid reference or review limit')
        selected = args.strike_intent or args.mint_intent or args.feeder_request
        if (args.receive_id is not None and args.strike_intent is None
                or args.custody_after is not None and args.custody_mint is None
                or selected is not None and not selected.int
                or (selected is not None or args.custody_mint is not None)
                and (args.reference or args.group or args.after or args.limit != 100)):
            raise ValueError('original selection requires one staged checkpoint and one operation')
        for field in ('strike_intent', 'receive_id', 'mint_intent', 'feeder_request'):
            if getattr(args, field) is not None:
                setattr(args, field, str(getattr(args, field)))
        common = runpy.run_path(str(HERE / 'collect-cashu-backup.py'))
        common['protected_directory'](args.output.parent, private=True)
        if (not args.output.is_absolute() or args.output.is_relative_to(args.staged)
                or (args.reference is not None and args.output.is_relative_to(args.reference))):
            raise ValueError('review output must be outside both selected stages')
        result = review(args, common)
        common['write_json'](args.output, result)
        common['sync'](args.output.parent)
        summary = {'resume_authorized': False}
        if args.strike_intent is not None:
            summary['result'] = 'CASHU_RESTORE_STRIKE_SELECTED'
        elif args.mint_intent is not None:
            summary['result'] = 'CASHU_RESTORE_MINT_SELECTED'
        elif args.feeder_request is not None:
            summary['result'] = 'CASHU_RESTORE_FEEDER_SELECTED'
        elif args.custody_mint is not None:
            summary['result'] = 'CASHU_RESTORE_CUSTODY_SELECTED'
        else:
            summary['result'] = result['result']
            summary['counts'] = {kind: batch['total'] for kind, batch in result['records'].items()}
        print(json.dumps(summary, sort_keys=True))
    except (ValueError, OSError, KeyError, TypeError, IndexError, sqlite3.Error):
        parser.exit(1, 'Cashu restore review refused. Preserve the fenced checkpoint.\n')


if __name__ == '__main__':
    if 'verified_tool_identity' not in globals():
        raise SystemExit('use the externally verified root-owned backup launcher')
    main()
