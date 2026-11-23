"""Offline snapshot verification uses disposable synthetic databases and keys."""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import runpy
import sqlite3
import socket
import tempfile
import tracemalloc
import unittest
import uuid

MODULE = runpy.run_path(str(Path(__file__).parents[1] / 'scripts/verify-cashu-backup.py'))


class BackupInventoryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.registrations = {key: str(uuid.uuid4()) for key in ['core_epoch', 'wallet_instance', 'wallet_epoch']}
        self.rows = []
        for role in sorted(MODULE['REQUIRED']):
            path = self.root / role
            if role in MODULE['DATABASES']:
                with closing(sqlite3.connect(path)) as connection:
                    connection.execute('CREATE TABLE retained (identity TEXT)')
                    connection.execute("INSERT INTO retained VALUES ('original')")
                    if role == 'core_ledger':
                        connection.execute('CREATE TABLE cashu_treasury_intents(id TEXT,intent_json TEXT)')
                        connection.execute('CREATE TABLE cashu_treasury_wallet_results(intent_id TEXT,result_json TEXT)')
                        connection.execute('CREATE TABLE cashu_receipt_inbox(wallet_instance TEXT, acquisition_id TEXT, wallet_epoch TEXT, core_epoch TEXT, receipt_digest TEXT, event_seq INTEGER, net_sats INTEGER, original_feed_credit_sats INTEGER, evidence_digest TEXT)')
                        connection.execute('CREATE TABLE overlay_identity(singleton INTEGER, stream_id TEXT)')
                        connection.execute('INSERT INTO overlay_identity VALUES(1,?)', [self.registrations['core_epoch']])
                    elif role == 'wallet_journal':
                        connection.execute('CREATE TABLE treasury_results(intent_id TEXT,report BLOB)')
                        connection.execute('CREATE TABLE acquisitions(id TEXT,state TEXT,core_ack BLOB,net INTEGER,acquired_evidence BLOB)')
                        connection.execute('CREATE TABLE wallet_epoch(singleton INTEGER, epoch TEXT)')
                        connection.execute('CREATE TABLE runtime_registration(singleton INTEGER, instance TEXT)')
                        connection.execute('INSERT INTO wallet_epoch VALUES(1,?)', [self.registrations['wallet_epoch']])
                        connection.execute('INSERT INTO runtime_registration VALUES(1,?)', [self.registrations['wallet_instance']])
                    elif role == 'signing_broker':
                        connection.execute('CREATE TABLE registration(singleton INTEGER, binding BLOB)')
                        binding = [str(uuid.uuid4()), str(uuid.uuid4()), self.registrations['wallet_instance'], self.registrations['wallet_epoch'], 12345, 'ab' * 32]
                        connection.execute('INSERT INTO registration VALUES(1,?)', [json.dumps(binding).encode()])
                    connection.commit()
            else:
                path.write_bytes(b'x' * (64 if role == 'wallet_seed' else 32))
            path.chmod(0o600)
            self.rows.append({'role': role, 'path': role,
                              'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
        self.save()

    def save(self):
        path = self.root / 'inventory.json'
        path.write_text(json.dumps({'version': 1, 'checkpoint_id': str(uuid.uuid4()),
                                    'registrations': self.registrations, 'components': self.rows}))
        path.chmod(0o600)

    def test_valid_inventory_is_read_only_and_does_not_authorize_restore(self):
        before = {p.name: p.read_bytes() for p in self.root.iterdir()}
        result = MODULE['verify'](self.root)
        self.assertFalse(result['restore_authorized'])
        self.assertFalse(result['external_reconciliation_verified'])
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.root.iterdir()})

    def test_missing_role_is_rejected(self):
        self.rows.pop()
        self.save()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_orphaned_internal_relationship_is_refused(self):
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute('CREATE TABLE backup_parent(id TEXT PRIMARY KEY)')
            connection.execute('CREATE TABLE backup_child(parent_id TEXT REFERENCES backup_parent(id))')
            connection.execute("INSERT INTO backup_child VALUES('missing-original')")
            connection.commit()
            self.assertEqual(connection.execute('PRAGMA integrity_check').fetchall(), [('ok',)])
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute("INSERT INTO backup_parent VALUES('missing-original')")
            connection.commit()
        self.refresh_hashes()
        self.assertFalse(MODULE['verify'](self.root)['restore_authorized'])

    def test_large_policy_verification_has_bounded_python_memory(self):
        row = next(r for r in self.rows if r['role'] == 'protected_policy')
        path = self.root / row['path']
        chunk = b'p' * (1024 * 1024)
        checksum = hashlib.sha256()
        with path.open('wb') as stream:
            for _ in range(32):
                stream.write(chunk)
                checksum.update(chunk)
        row['sha256'] = checksum.hexdigest()
        self.save()
        tracemalloc.start()
        try:
            result = MODULE['verify'](self.root)
            _, peak = tracemalloc.get_traced_memory()
        finally:
            tracemalloc.stop()
        self.assertFalse(result['restore_authorized'])
        self.assertLess(peak, 16 * 1024 * 1024,
                        'snapshot verification retained a whole large component')

    def test_unlisted_special_entries_are_rejected(self):
        fifo = self.root / 'unexpected-pipe'
        os.mkfifo(fifo, 0o600)
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        fifo.unlink()
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
            listener.bind(str(self.root / 'unexpected-socket'))
            with self.assertRaises(ValueError):
                MODULE['verify'](self.root)

    @unittest.skipIf(os.geteuid() == 0, 'root can traverse owner-inaccessible directories')
    def test_inaccessible_unlisted_directory_cannot_hide_components(self):
        directory = self.root / 'inaccessible'
        directory.mkdir(mode=0o700)
        (directory / 'unlisted-state').write_bytes(b'not inventoried')
        directory.chmod(0)
        try:
            with self.assertRaises(OSError):
                MODULE['verify'](self.root)
        finally:
            directory.chmod(0o700)

    def test_unlisted_wal_alias_and_exposure_are_rejected(self):
        (self.root / 'core_ledger-wal').write_bytes(b'uncheckpointed')
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        (self.root / 'core_ledger-wal').unlink()
        os.symlink('core_ledger', self.root / 'alias')
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        (self.root / 'alias').unlink()
        (self.root / 'wallet_seed').chmod(0o644)
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_changed_bytes_and_invalid_paths_are_rejected(self):
        seed = self.root / 'wallet_seed'
        seed.write_bytes(b'y' * 64)
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        seed.write_bytes(b'x' * 64)
        row = next(r for r in self.rows if r['role'] == 'wallet_seed')
        for path in ['../wallet_seed', '/wallet_seed', './wallet_seed']:
            row['path'] = path
            self.save()
            with self.assertRaises(ValueError):
                MODULE['verify'](self.root)

    def test_wrong_key_size_and_hardlinks_are_rejected(self):
        seed = self.root / 'wallet_seed'
        seed.write_bytes(b'x' * 63)
        next(r for r in self.rows if r['role'] == 'wallet_seed')['sha256'] = hashlib.sha256(seed.read_bytes()).hexdigest()
        self.save()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        seed.write_bytes(b'x' * 64)
        os.link(seed, self.root / 'seed-alias')
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_ambiguous_inventory_and_noninteger_version_are_rejected(self):
        path = self.root / 'inventory.json'
        original = path.read_text()
        path.write_text(original.replace('"version": 1', '"version": 1, "version": 1'))
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        path.write_text(original.replace('"version": 1', '"version": true'))
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_otherwise_valid_snapshot_with_wrong_wallet_epoch_is_rejected(self):
        self.registrations['wallet_epoch'] = str(uuid.uuid4())
        self.save()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_signer_binding_from_another_wallet_is_rejected(self):
        path = self.root / 'signing_broker'
        with closing(sqlite3.connect(path)) as connection:
            binding = [str(uuid.uuid4()) for _ in range(4)] + [12345, 'ab' * 32]
            connection.execute('UPDATE registration SET binding=?', [json.dumps(binding).encode()])
            connection.commit()
        next(r for r in self.rows if r['role'] == 'signing_broker')['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
        self.save()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_credited_ack_requires_the_original_core_receipt(self):
        identity = str(uuid.uuid4())
        result = {'acquisition_id': identity, 'receipt_digest': 'ab' * 32,
                  'event_seq': 1, 'net_sats': 8, 'original_feed_credit_sats': 8}
        ack = dict(version=1, **self.registrations, result=result)
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute('INSERT INTO acquisitions VALUES(?,?,?,?,?)',
                               [identity, 'credited', json.dumps(ack).encode(), 8, bytes.fromhex('ab' * 32)])
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        with closing(sqlite3.connect(self.root / 'core_ledger')) as connection:
            connection.execute('INSERT INTO cashu_receipt_inbox VALUES(?,?,?,?,?,?,?,?,?)',
                               [self.registrations['wallet_instance'], identity,
                                self.registrations['wallet_epoch'], self.registrations['core_epoch'],
                                result['receipt_digest'], 1, 8, 8, 'ab' * 32])
            connection.commit()
        self.refresh_hashes()
        self.assertEqual(MODULE['verify'](self.root)['acknowledged_receipts_checked'], 1)
        with closing(sqlite3.connect(self.root / 'core_ledger')) as connection:
            connection.execute('UPDATE cashu_receipt_inbox SET net_sats=9')
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_lost_ack_is_allowed_but_rolled_back_wallet_history_is_rejected(self):
        identity = str(uuid.uuid4())
        with closing(sqlite3.connect(self.root / 'core_ledger')) as connection:
            connection.execute('INSERT INTO cashu_receipt_inbox VALUES(?,?,?,?,?,?,?,?,?)',
                               [self.registrations['wallet_instance'], identity,
                                self.registrations['wallet_epoch'], self.registrations['core_epoch'],
                                'cd' * 32, 1, 8, 8, 'ab' * 32])
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute('INSERT INTO acquisitions VALUES(?,?,?,?,?)',
                               [identity, 'credit_export_pending', None, 8, bytes.fromhex('ab' * 32)])
            connection.commit()
        self.refresh_hashes()
        self.assertEqual(MODULE['verify'](self.root)['acknowledged_receipts_checked'], 0)
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute("UPDATE acquisitions SET state='validated',net=NULL,acquired_evidence=NULL")
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_core_treasury_report_requires_original_wallet_result(self):
        identity = str(uuid.uuid4())
        report = json.dumps({'intent_id': identity, 'operation_evidence_digest': 'ab' * 32})
        with closing(sqlite3.connect(self.root / 'core_ledger')) as connection:
            connection.execute('INSERT INTO cashu_treasury_intents VALUES(?,?)', [identity, json.dumps(self.registrations)])
            connection.execute('INSERT INTO cashu_treasury_wallet_results VALUES(?,?)', [identity, report])
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute('INSERT INTO treasury_results VALUES(?,?)', [identity, report.encode()])
            connection.commit()
        self.refresh_hashes()
        self.assertFalse(MODULE['verify'](self.root)['restore_authorized'])
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute('UPDATE treasury_results SET report=?', [b'{}'])
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def add_mint_namespaces(self):
        with closing(sqlite3.connect(self.root / 'wallet_journal')) as connection:
            connection.execute('CREATE TABLE cdk_storage_layout(singleton INTEGER,version INTEGER)')
            connection.execute('INSERT INTO cdk_storage_layout VALUES(1,1)')
            connection.execute('CREATE TABLE cdk_mint_namespaces(mint_id TEXT,mint_url TEXT,ready INTEGER)')
            for mint in ['first','second']:
                connection.execute('INSERT INTO cdk_mint_namespaces VALUES(?,?,1)', [mint,f'https://{mint}.example'])
            connection.commit()
        with closing(sqlite3.connect(self.root / 'cdk_wallet')) as connection:
            connection.execute('CREATE TABLE proof(y BLOB)')
            connection.execute('CREATE TABLE wallet_sagas(id TEXT)')
            connection.commit()
        for mint in ['first','second']:
            path = self.root / (mint + '.sqlite')
            with closing(sqlite3.connect(path)) as connection:
                connection.execute('CREATE TABLE lg_mint_namespace(singleton INTEGER,mint_id TEXT,mint_url TEXT)')
                connection.execute('INSERT INTO lg_mint_namespace VALUES(1,?,?)', [mint,f'https://{mint}.example'])
                connection.commit()
            path.chmod(0o600)
            self.rows.append({'role':'cdk_mint_wallet','path':path.name,'sha256':'',
                              'mint_id':mint,'mint_url':f'https://{mint}.example'})
        self.refresh_hashes()

    def test_every_retained_mint_store_is_required_even_without_current_policy(self):
        self.add_mint_namespaces()
        self.assertEqual(MODULE['verify'](self.root)['mint_stores_checked'],2)
        removed = self.rows.pop()
        (self.root / removed['path']).unlink()
        self.save()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def test_rebound_mint_store_and_legacy_custody_are_rejected(self):
        self.add_mint_namespaces()
        with closing(sqlite3.connect(self.root / 'first.sqlite')) as connection:
            connection.execute("UPDATE lg_mint_namespace SET mint_url='https://other.example'")
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)
        with closing(sqlite3.connect(self.root / 'first.sqlite')) as connection:
            connection.execute("UPDATE lg_mint_namespace SET mint_url='https://first.example'")
            connection.commit()
        with closing(sqlite3.connect(self.root / 'cdk_wallet')) as connection:
            connection.execute("INSERT INTO proof VALUES(X'01')")
            connection.commit()
        self.refresh_hashes()
        with self.assertRaises(ValueError):
            MODULE['verify'](self.root)

    def refresh_hashes(self):
        for row in self.rows:
            row['sha256'] = hashlib.sha256((self.root / row['path']).read_bytes()).hexdigest()
        self.save()

    def test_matching_hash_does_not_hide_database_corruption(self):
        path = self.root / 'core_ledger'
        path.write_bytes(b'not a database')
        next(r for r in self.rows if r['role'] == 'core_ledger')['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
        self.save()
        with self.assertRaises(sqlite3.Error):
            MODULE['verify'](self.root)


if __name__ == '__main__':
    unittest.main()
