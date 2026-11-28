#!/usr/bin/env python3
"""Exercise custody-only repair on synthetic SQLite state with the native CLI."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile


POINT = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
SPENT = '02' + POINT
AVAILABLE = '03' + POINT
HELD = '02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
MINT_URL = 'https://mint.example.test'
CHECKPOINT = '11f9573d-39b1-4ae8-aa32-35ca32938f72'
ANCHOR = '19' * 32


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, document):
    path.write_text(json.dumps(document, sort_keys=True))
    path.chmod(0o600)
    return digest(path)


def fixture(directory):
    directory.mkdir(mode=0o700)
    store = directory / 'mint-copy.sqlite'
    with sqlite3.connect(store) as db:
        db.executescript('''
            CREATE TABLE lg_mint_namespace(singleton INTEGER PRIMARY KEY, mint_id TEXT, mint_url TEXT);
            CREATE TABLE proof(y BLOB PRIMARY KEY, amount INTEGER, state TEXT, mint_url TEXT,
                               unit TEXT, used_by_operation TEXT, history TEXT);
            CREATE TABLE unrelated(id TEXT PRIMARY KEY, bytes BLOB);
            INSERT INTO unrelated VALUES('original', X'01020304');
        ''')
        db.execute('INSERT INTO lg_mint_namespace VALUES(1,?,?)', ('fixture', MINT_URL))
        db.executemany('INSERT INTO proof VALUES(?,?,?,?,?,?,?)', [
            (bytes.fromhex(SPENT), 7, 'UNSPENT', MINT_URL, 'sat', None, 'retained history'),
            (bytes.fromhex(AVAILABLE), 12, 'UNSPENT', MINT_URL, 'sat', None, 'other proof'),
            (bytes.fromhex(HELD), 34, 'RESERVED', MINT_URL, 'sat', 'original operation', 'held history'),
        ])
    store.chmod(0o600)
    source = digest(store)
    proofs = [{'y': point, 'amount_sats': sats, 'local_state': before, 'mint_state': observed}
              for point, sats, before, observed in (
                  (SPENT, 7, 'UNSPENT', 'SPENT'), (AVAILABLE, 12, 'UNSPENT', 'UNSPENT'),
                  (HELD, 34, 'RESERVED', 'SPENT'))]
    observation = {'version': 1, 'result': 'CASHU_RESTORE_MINT_CUSTODY_OBSERVED',
                   'checkpoint_id': CHECKPOINT, 'staged_receipt_sha256': ANCHOR,
                   'mint_store_sha256': source, 'mint_id': 'fixture', 'mint_url': MINT_URL,
                   'total_candidates': len(proofs), 'after_y': None, 'next_after_y': None,
                   'proofs': sorted(proofs, key=lambda proof: proof['y']), 'queried_mint': True,
                   'stale_local_unspent_sats': 7, 'external_reconciliation_verified': False,
                   'restore_authorized': False, 'resume_authorized': False}
    sha = write(directory / 'observation-0.json', observation)
    plan = {'version': 1, 'checkpoint_id': CHECKPOINT, 'observation_anchor_sha256': ANCHOR,
            'source_sha256': source, 'mint_id': 'fixture', 'mint_url': MINT_URL,
            'observation_sha256': [sha], 'approved_points': [SPENT]}
    return store, plan, observation


def run(cli, directory, plan_sha256, success):
    result = subprocess.run([cli, 'prepare-custody-repair', directory, plan_sha256],
                            capture_output=True, check=False, timeout=25)
    if success:
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)
    assert result.returncode == 1


def verify_state(source, repaired):
    with sqlite3.connect(source) as before, sqlite3.connect(repaired) as after:
        for table in ('sqlite_schema', 'lg_mint_namespace', 'unrelated'):
            sql = f'SELECT * FROM {table} ORDER BY rowid'
            assert before.execute(sql).fetchall() == after.execute(sql).fetchall()
        rows = before.execute('SELECT * FROM proof ORDER BY y').fetchall()
        expected = [row[:2] + ('SPENT',) + row[3:] if row[0] == bytes.fromhex(SPENT) else row for row in rows]
        assert after.execute('SELECT * FROM proof ORDER BY y').fetchall() == expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--statectl', type=Path, required=True)
    parser.add_argument('--parent', type=Path, required=True, help='protected root-owned directory for fixtures')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='custody-repair-test.', dir=args.parent) as path:
        root = Path(path)
        good = root / 'good'
        source, plan, _ = fixture(good)
        sha = write(good / 'REPAIR-PLAN.json', plan)
        report = run(args.statectl, good, sha, True)
        assert report['retired_proofs'] == 1 and report['retired_sats'] == 7
        assert report['restore_authorized'] is False and report['resume_authorized'] is False
        result = good / 'repaired/mint-copy.sqlite'
        assert digest(result) == report['repaired_sha256']
        verify_state(source, result)
        assert run(args.statectl, good, sha, True) == report
        assert digest(source) == plan['source_sha256']
        for variant in ('wrong-digest', 'not-approved', 'wrong-namespace', 'missing-proof', 'held-proof'):
            staged = root / variant
            untouched, plan, evidence = fixture(staged)
            if variant == 'not-approved':
                plan['approved_points'] = [AVAILABLE]
            elif variant == 'wrong-namespace':
                plan['mint_id'] = 'other-mint'
            elif variant == 'missing-proof':
                evidence['proofs'].pop()
            elif variant == 'held-proof':
                with sqlite3.connect(untouched) as db:
                    db.execute('UPDATE proof SET used_by_operation=? WHERE y=?', ('other-op', bytes.fromhex(SPENT)))
                plan['source_sha256'] = evidence['mint_store_sha256'] = digest(untouched)
            plan['observation_sha256'] = [write(staged / 'observation-0.json', evidence)]
            sha = write(staged / 'REPAIR-PLAN.json', plan)
            run(args.statectl, staged, '0' * 64 if variant == 'wrong-digest' else sha, False)
            assert not (staged / 'repaired').exists() and digest(untouched) == plan['source_sha256']
        print('Custody repair verified exact change, retained history, exact retry and five refusal cases.')


if __name__ == '__main__':
    main()
