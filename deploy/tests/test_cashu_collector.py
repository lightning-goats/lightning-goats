"""Root-only disposable filesystem and quiescence tests; no service mutations."""
from contextlib import ExitStack, closing
import hashlib
import io
import json
import os
from pathlib import Path
import pwd
import runpy
import shutil
import sqlite3
import subprocess
import tarfile
import tempfile
import types
import unittest
from unittest.mock import patch

SOURCE=Path(__file__).resolve().parents[2]/'deploy/scripts'
MODULE=runpy.run_path(str(SOURCE/'collect-cashu-backup.py'))
LAUNCH=runpy.run_path(str(SOURCE/'launch-cashu-backup.py'))
RESTORE=runpy.run_path(str(SOURCE/'stage-cashu-restore.py'))
REVIEW=runpy.run_path(str(SOURCE/'review-cashu-restore.py'))
RECONCILE=runpy.run_path(str(SOURCE/'reconcile-cashu-restore.py'))
INSTALL=runpy.run_path(str(SOURCE/'install-cashu-restore.py'))


class RecoveryWorklistTests(unittest.TestCase):
    def test_mint_custody_pages_preserve_exact_unspent_and_reserved_points(self):
        url = 'https://mint.example/Bitcoin'
        page = REVIEW['custody_page']
        with closing(sqlite3.connect(':memory:')) as store:
            store.execute('CREATE TABLE proof(y BLOB PRIMARY KEY,amount INTEGER,state TEXT,mint_url TEXT,unit TEXT,secret TEXT)')
            points = [b'\x02' + n.to_bytes(32, 'big') for n in range(130)]
            store.executemany('INSERT INTO proof VALUES(?,?,?,?,?,?)',
                              [(point, 1, 'RESERVED' if n == 128 else 'UNSPENT', url, 'sat', 'do not export')
                               for n, point in enumerate(points)])
            store.execute("UPDATE proof SET state='SPENT' WHERE y=?", (points[0],))
            first = page(store, 'mint-name', url, 'fixture-checkpoint', '11' * 32, '22' * 32)
            self.assertEqual(first['total_candidates'], 129)
            self.assertEqual(first['next_after_y'], points[128].hex())
            self.assertEqual(len(first['proofs']), 128)
            self.assertEqual(set(first['proofs'][0]), {'y', 'amount_sats', 'local_state'})
            self.assertEqual(first['proofs'][-1]['local_state'], 'RESERVED')
            final = page(store, 'mint-name', url, 'fixture-checkpoint', '11' * 32, '22' * 32, first['next_after_y'])
            self.assertEqual(final['proofs'][0]['y'], points[129].hex())
            self.assertIsNone(final['next_after_y'])
            with self.assertRaisesRegex(ValueError, 'cursor does not belong'):
                page(store, 'mint-name', url, 'fixture-checkpoint', '11' * 32, '22' * 32, points[0].hex())
            store.execute("UPDATE proof SET mint_url='https://foreign.example' WHERE y=?", (points[129],))
            with self.assertRaisesRegex(ValueError, 'original sat mint namespace'):
                page(store, 'mint-name', url, 'fixture-checkpoint', '11' * 32, '22' * 32)
            store.execute("UPDATE proof SET mint_url=?,unit=NULL WHERE state='UNSPENT'", (url,))
            with self.assertRaisesRegex(ValueError, 'original sat mint namespace'):
                page(store, 'mint-name', url, 'fixture-checkpoint', '11' * 32, '22' * 32)

    def test_recovery_keeps_missing_later_id_and_pages_after_original_id(self):
        older = [{'id': 'a', 'receive_id': None}, {'id': 'b', 'receive_id': 'strike-b'}]
        later = [{'id': 'a', 'receive_id': 'strike-a'}, {'id': 'b', 'receive_id': 'strike-b'},
                 {'id': 'c', 'receive_id': 'strike-c'}]
        compare = REVIEW['compare_rows']
        page = compare(iter(older), iter(later), ('receive_id',), lambda _: False, 1)
        self.assertEqual(page['total'], 2)
        self.assertEqual(page['next_after'], 'a')
        second = compare(iter(older), iter(later), ('receive_id',), lambda _: False, 1, page['next_after'])
        self.assertEqual(second, {'total': 1, 'next_after': None,
                                  'items': [{'staged': None, 'reference': later[-1]}]})

    def test_conflicting_provider_id_is_refused_even_outside_requested_page(self):
        older = [{'id': 'a', 'payment_hash': 'original'}]
        later = [{'id': 'a', 'payment_hash': 'different'}]
        with self.assertRaisesRegex(ValueError, 'immutable original'):
            REVIEW['compare_rows'](iter(older), iter(later), ('payment_hash',), lambda _: False, 1, 'b')

    def test_reference_preserves_exact_core_event_prefix(self):
        require_prefix = REVIEW['require_event_prefix']
        with (closing(sqlite3.connect(':memory:')) as staged,
              closing(sqlite3.connect(':memory:')) as reference):
            for db in (staged, reference):
                db.execute('CREATE TABLE event_log(seq INTEGER PRIMARY KEY,event_type TEXT,payload_json TEXT,created_at INTEGER)')
                db.executemany('INSERT INTO event_log VALUES(?,?,?,?)',
                               [(1, 'payment_received', '{"amount":10}', 123),
                                (3, 'feeder_confirmed', '{}', 456)])
            reference.execute("INSERT INTO event_log VALUES(4,'interface_info','{}',789)")
            require_prefix(staged, reference)
            reference.execute("UPDATE event_log SET payload_json='{}' WHERE seq=1")
            with self.assertRaisesRegex(ValueError, 'staged core event history'):
                require_prefix(staged, reference)
            reference.execute('DELETE FROM event_log WHERE seq=1')
            with self.assertRaisesRegex(ValueError, 'staged core event history'):
                require_prefix(staged, reference)

    def test_recovery_lists_original_split_operation_across_submission(self):
        with (closing(sqlite3.connect(':memory:')) as staged,
              closing(sqlite3.connect(':memory:')) as reference):
            for db in (staged, reference):
                db.row_factory = sqlite3.Row
                db.executescript('''
                    CREATE TABLE overlay_identity(singleton,stream_id);
                    INSERT INTO overlay_identity VALUES(1,'core');
                    CREATE TABLE wallet_epoch(singleton,epoch);
                    INSERT INTO wallet_epoch VALUES(1,'wallet');
                    CREATE TABLE runtime_registration(singleton,instance,key_binding);
                    INSERT INTO runtime_registration VALUES(1,'instance',X'01');
                    CREATE TABLE event_log(seq,event_type,payload_json,created_at);
                    CREATE TABLE treasury_split_reservations(intent_id,split_id);
                    CREATE TABLE treasury_quotes(intent_id,mint_id);
                    CREATE TABLE treasury_split_execution_claims(intent_id);
                    CREATE TABLE treasury_split_requests(intent_id,cdk_operation);
                    CREATE TABLE treasury_split_submissions(intent_id,submitted_at,request_digest);
                    CREATE TABLE treasury_split_replies(intent_id);
                    CREATE TABLE treasury_split_results(intent_id);
                    INSERT INTO treasury_split_reservations VALUES('intent','split');
                    INSERT INTO treasury_quotes VALUES('intent','fixture-mint');
                    INSERT INTO treasury_split_execution_claims VALUES('intent');
                ''')
            reference.execute("INSERT INTO treasury_split_requests VALUES('intent','swap-operation')")
            reference.execute('INSERT INTO treasury_split_submissions VALUES(?,?,?)',
                              ('intent', 123, b'\xaa' * 32))
            reference.execute("INSERT INTO treasury_split_replies VALUES('intent')")
            report = REVIEW['worklist'](staged, staged, reference, reference, group='mint_splits')
            item, = report['mint_splits']['items']
            self.assertIsNone(item['staged']['submitted_request_digest'])
            self.assertEqual(item['reference']['split_id'], 'split')
            self.assertEqual(item['reference']['cdk_operation'], 'swap-operation')
            self.assertEqual(item['reference']['submitted_request_digest'], 'aa' * 32)
            self.assertEqual(item['reference']['reply_retained'], 1)
            self.assertEqual(item['reference']['result_retained'], 0)


class RestoreArchiveTests(unittest.TestCase):
    @staticmethod
    def tar(entries, format=tarfile.USTAR_FORMAT):
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode='w', format=format) as archive:
            for name, kind, data in entries:
                member = tarfile.TarInfo(name)
                member.type = kind
                member.mode = 0o777
                member.uid = 123
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
        raw.seek(0)
        return raw

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def extract(self, archive, name='stage', maximum=1024):
        return RESTORE['extract_plain_tar'](archive, self.root / name, MODULE['Budget'](maximum, 10))

    def test_regular_files_and_directories_get_private_modes(self):
        archive = self.tar([('wallet', tarfile.DIRTYPE, b''),
                            ('wallet/journal', tarfile.REGTYPE, b'stored bytes')])
        self.extract(archive)
        wallet = self.root / 'stage/wallet'
        self.assertEqual((wallet / 'journal').read_bytes(), b'stored bytes')
        self.assertEqual(wallet.stat().st_mode & 0o777, 0o700)
        self.assertEqual((wallet / 'journal').stat().st_mode & 0o777, 0o600)

    def test_unsafe_members_and_duplicate_names_are_refused(self):
        for i, entries in enumerate([
            [('../outside', tarfile.REGTYPE, b'ignored')],
            [('linked', tarfile.SYMTYPE, b'')],
            [('linked', tarfile.LNKTYPE, b'')],
            [('duplicate', tarfile.REGTYPE, b'a'), ('duplicate', tarfile.REGTYPE, b'b')],
        ]):
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                self.extract(self.tar(entries), f'stage-{i}')
        self.assertFalse((self.root / 'outside').exists())
        self.assertEqual((self.root / 'stage-3/duplicate').read_bytes(), b'a')

    def test_extended_headers_and_data_after_terminator_are_refused(self):
        extended = self.tar([('x' * 256, tarfile.REGTYPE, b'a')], tarfile.PAX_FORMAT)
        with self.assertRaises(ValueError):
            self.extract(extended, 'extended')
        ordinary = self.tar([('file', tarfile.REGTYPE, b'original')])
        with self.assertRaises(ValueError):
            self.extract(io.BytesIO(ordinary.getvalue() + b'extra'), 'trailing')

    def test_component_cannot_overrun_aggregate_remaining_bytes(self):
        archive = self.tar([('file', tarfile.REGTYPE, b'data')])
        with self.assertRaises(ValueError):
            self.extract(archive, maximum=3)
        self.assertFalse((self.root / 'stage/file').exists())


@unittest.skipUnless(os.geteuid()==0,'requires disposable root-owned files')
class CollectorTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='lg-collector-unit-',dir='/run')
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.root.chmod(0o700)
        self.budget=MODULE['Budget'](1024*1024,30)

    def test_custody_comparison_requires_every_page_in_checkpoint_order(self):
        mint, url, stage = 'mint-name', 'https://mint.example/Bitcoin', '11' * 32
        retired, retired_url = 'retired-mint', 'https://retired.example'
        checkpoint = '10000000-0000-4000-8000-000000000001'
        journal = self.root / 'acquisition.sqlite'
        path = self.root / REVIEW['mint_store_name'](mint, MODULE)
        with closing(sqlite3.connect(journal)) as db:
            db.executescript('CREATE TABLE cdk_storage_layout(singleton,version); INSERT INTO cdk_storage_layout VALUES(1,1);')
            db.execute('CREATE TABLE cdk_mint_namespaces(mint_id,mint_url,ready)')
            db.executemany('INSERT INTO cdk_mint_namespaces VALUES(?,?,1)', [(mint,url), (retired,retired_url)])
            db.commit()
        with closing(sqlite3.connect(path)) as db:
            db.execute('CREATE TABLE lg_mint_namespace(singleton,mint_id,mint_url)')
            db.execute('INSERT INTO lg_mint_namespace VALUES(1,?,?)', (mint,url))
            db.execute('CREATE TABLE proof(y BLOB PRIMARY KEY,amount INTEGER,state TEXT,mint_url TEXT,unit TEXT)')
            db.executemany('INSERT INTO proof VALUES(?,1,?,?,?)',
                           [(b'\x02' + n.to_bytes(32, 'big'), 'UNSPENT', url, 'sat') for n in range(129)])
            db.commit()
        for database in (path, journal):
            database.chmod(0o600)
        store_sha = MODULE['sha'](path.read_bytes())
        with REVIEW['open_custody_store'](journal, mint, store_sha, MODULE, self.budget) as (db, url):
            first = REVIEW['custody_page'](db, mint, url, checkpoint, stage, store_sha)
            last = REVIEW['custody_page'](db, mint, url, checkpoint, stage, store_sha, first['next_after_y'])
        observations = []
        for index, page in enumerate((first,last)):
            state = 'SPENT' if index == 0 else 'PENDING'
            report = {**page, **RECONCILE['NO_AUTHORITY'], 'result': RECONCILE['RESULTS']['custody'],
                      'selection_sha256': MODULE['sha'](MODULE['json_bytes'](page)),
                      'queried_mint': True, 'observed_at': 123 + index,
                      'proofs': [{**p, 'mint_state': state} for p in page['proofs']],
                      'mint_unspent_sats': 0, 'mint_spent_sats': 128 if index == 0 else 0,
                      'mint_unresolved_sats': index, 'stale_local_unspent_sats': 128 if index == 0 else 0}
            raw = MODULE['json_bytes'](report)
            file = self.source(f'page-{index}.json', raw)
            observations.append({'file': file.name, 'sha256': MODULE['sha'](raw)})

        def compare(pages):
            entry = {'mint_id': mint, 'staged_receipt_sha256': stage, 'observations': pages}
            return RECONCILE['compare_custody'](entry, self.root, {stage: (checkpoint, None, None)},
                                               (journal, store_sha), MODULE, self.budget)

        result = compare(observations)
        self.assertEqual(result['selected_count'], 129)
        self.assertEqual(result['stale_local_unspent_sats'], 128)
        self.assertEqual(result['mint_unresolved_sats'], 1)
        with self.assertRaisesRegex(ValueError, 'omit a required page'):
            compare(observations[:1])
        with self.assertRaisesRegex(ValueError, 'exact retained original selection'):
            compare(observations[::-1])

        second = self.root / REVIEW['mint_store_name'](retired, MODULE)
        with closing(sqlite3.connect(second)) as db:
            db.executescript('CREATE TABLE lg_mint_namespace(singleton,mint_id,mint_url);'
                             'CREATE TABLE proof(y BLOB PRIMARY KEY,amount INTEGER,state TEXT,mint_url TEXT,unit TEXT);')
            db.execute('INSERT INTO lg_mint_namespace VALUES(1,?,?)', (retired, retired_url))
            db.execute('INSERT INTO proof VALUES(?,16,?,?,?)', (b'\x03' + bytes(32), 'UNSPENT', retired_url, 'sat'))
            db.commit()
        legacy = self.root / 'wallet.sqlite'
        with closing(sqlite3.connect(legacy)) as db:
            db.executescript('CREATE TABLE proof(y); CREATE TABLE wallet_sagas(id);')
        for database in (legacy, second):
            database.chmod(0o600)
        files = {file.name: MODULE['sha'](file.read_bytes()) for file in (journal, legacy, path, second)}
        with REVIEW['open_custody_store'](journal, retired, files[second.name], MODULE, self.budget) as (db, original):
            page = REVIEW['custody_page'](db, retired, original, checkpoint, stage, files[second.name])
        retired_report = {**page, **RECONCILE['NO_AUTHORITY'], 'result': RECONCILE['RESULTS']['custody'],
                          'selection_sha256': MODULE['sha'](MODULE['json_bytes'](page)), 'queried_mint': True,
                          'observed_at': 130, 'proofs': [{**p, 'mint_state': 'UNSPENT'} for p in page['proofs']],
                          'mint_unspent_sats': 16, 'mint_spent_sats': 0, 'mint_unresolved_sats': 0,
                          'stale_local_unspent_sats': 0}
        raw = MODULE['json_bytes'](retired_report)
        observation = {'file': self.source('retired.json', raw).name, 'sha256': MODULE['sha'](raw)}

        def compare_all(entries, source=None):
            return RECONCILE['compare_custody_all']({'staged_receipt_sha256': stage, 'mints': entries},
                self.root, {stage: (checkpoint, None, None)}, (journal, files if source is None else source),
                MODULE, self.budget)

        entries = {mint: observations, retired: [observation]}
        report = compare_all(entries)
        self.assertEqual(report['mint_namespaces_compared'], 2)
        self.assertEqual(report['selected_count'], 130)
        self.assertEqual(report['mint_unspent_sats'], 16)
        self.assertEqual(report['stale_local_unspent_sats'], 128)
        self.assertEqual(report['mint_unresolved_sats'], 1)
        self.assertEqual((report['observed_from'], report['observed_through']), (123, 130))
        with self.assertRaisesRegex(ValueError, 'cover every retained mint'):
            compare_all({mint: observations})
        with self.assertRaisesRegex(ValueError, 'exact retained original selection'):
            compare_all({mint: observations, retired: observations})
        with self.assertRaisesRegex(ValueError, 'unbound mint store'):
            compare_all(entries, {**files, 'mint-unknown.sqlite': '00' * 32})
        with self.assertRaisesRegex(ValueError, 'missing, duplicate or unfinished retained mint namespace'):
            compare_all(entries, {key: value for key, value in files.items() if key != second.name})
        with closing(sqlite3.connect(legacy)) as db:
            db.execute('INSERT INTO proof VALUES(1)')
            db.commit()
        with self.assertRaisesRegex(ValueError, 'legacy shared-store custody requires reconciliation'):
            compare_all(entries, {**files, legacy.name: MODULE['sha'](legacy.read_bytes())})

    def source(self,name='input',raw=b'disposable bytes',mode=0o600):
        p=self.root/name;p.write_bytes(raw);p.chmod(mode);return p

    def frozen(self,path,**kwargs):
        f=MODULE['Frozen'](path,0,1024,self.budget,**kwargs)
        self.addCleanup(f.close);return f

    def test_copy_is_private_exact_and_no_replace(self):
        f=self.frozen(self.source());target=self.root/'copy'
        self.assertEqual(f.copy(target)['sha256'],hashlib.sha256(b'disposable bytes').hexdigest())
        self.assertEqual(target.read_bytes(),b'disposable bytes')
        self.assertEqual(target.stat().st_mode&0o777,0o600)
        with self.assertRaises(FileExistsError):f.copy(target)

    def test_changed_original_is_refused(self):
        p=self.source();f=self.frozen(p);p.write_bytes(b'changed original')
        with self.assertRaises(ValueError):f.check()

    def test_replaced_original_is_refused(self):
        p=self.source();f=self.frozen(p);p.unlink();p.write_bytes(b'disposable bytes');p.chmod(0o600)
        with self.assertRaises(ValueError):f.check()

    def test_fifo_is_refused_without_waiting(self):
        p=self.root/'fifo';os.mkfifo(p,0o600)
        with self.assertRaises(ValueError):self.frozen(p)

    def test_alias_and_hardlink_refuse(self):
        p=self.source();alias=self.root/'alias';alias.symlink_to(p.name)
        with self.assertRaises(ValueError):self.frozen(alias)
        alias.unlink();os.link(p,alias)
        with self.assertRaises(ValueError):self.frozen(p)

    def test_secret_permissions_and_config_permissions_differ(self):
        p=self.source(mode=0o440)
        with self.assertRaises(ValueError):self.frozen(p)
        self.frozen(p,secret=False).check()

    def test_signer_clients_require_original_keys_and_current_host_encryption(self):
        stage = self.root / 'stage'
        archive = stage / 'checkpoint-evidence/recovery-material'
        archive.mkdir(mode=0o700, parents=True)
        store = self.root / 'credential-store'
        store.mkdir(mode=0o700)
        host_key = self.root / 'fixture-host-key'
        tool_env = INSTALL['systemd_credential'].__globals__['CLEAN_ENV']
        originals, policy = {}, {}
        for role, (field, filename, _, _) in INSTALL['SIGNER_CLIENTS'].items():
            raw = b'0' * 63 + (b'2' if role == 'cashu_signer' else b'5\n')
            MODULE['write'](archive / filename, raw)
            policy[field] = '/original/' + filename
            originals['recovery-material/' + filename] = {
                'source': policy[field], 'source_uid': 0, 'sha256': MODULE['sha'](raw), 'bytes': len(raw),
            }

        def prepare(stack):
            return INSTALL['prepare_signer_clients'](store, stage, originals, policy, MODULE, self.budget, stack)

        with patch.dict(tool_env, SYSTEMD_CREDENTIAL_SECRET=str(host_key)):
            subprocess.run(['/usr/bin/systemd-creds', 'setup'], check=True,
                           stdin=subprocess.DEVNULL, capture_output=True, timeout=5, env=tool_env)
            key = MODULE['Frozen'](host_key, 0, 65536, self.budget)
            with closing(key):
                INSTALL['ensure_existing_host_key'](key, self.budget)
            with ExitStack() as temporary:
                candidates = prepare(temporary)
                cashu, path, checksum = candidates['cashu_signer']
                INSTALL['publish_no_replace'](path, cashu, self.budget)
                self.assertEqual(MODULE['sha'](cashu.read_bytes()), checksum)

            with ExitStack() as temporary:
                candidates = prepare(temporary)
                self.assertEqual(candidates['cashu_signer'], (cashu, None, checksum))
                payout, path, _ = candidates['signer']
                self.assertFalse(payout.exists())
                INSTALL['publish_no_replace'](path, payout, self.budget)
            misplaced = cashu.read_bytes()
            payout.write_bytes(misplaced)
            with ExitStack() as temporary, self.assertRaises(ValueError):
                prepare(temporary)
            self.assertEqual(payout.read_bytes(), misplaced)

        wrong_identity = self.root / 'foreign-host-key'
        MODULE['write'](wrong_identity, b'\0' * 17)
        wrong_identity.chmod(0o400)
        with (closing(MODULE['Frozen'](wrong_identity, 0, 65536, self.budget)) as key,
              self.assertRaisesRegex(ValueError, 'current systemd credential host key required')):
            INSTALL['ensure_existing_host_key'](key, self.budget)

    def test_file_and_aggregate_limits_refuse(self):
        p=self.source(raw=b'x'*1025)
        with self.assertRaises(ValueError):self.frozen(p)
        with self.assertRaises(ValueError):self.budget.add(1024*1024+1)

    def test_expired_deadline_refuses(self):
        self.budget.deadline=0
        with self.assertRaises(ValueError):self.budget.remaining()

    def workers(self):
        used={v.pw_uid for v in pwd.getpwall()}
        used|={int(v) for v in subprocess.check_output(['ps','-e','-o','uid='],text=True).split()}
        uid=next(v for v in range(61000,65000) if v not in used)
        return {'one':{'unit':'lg-collector-unit.service','uid':uid}}

    def test_quiescence_requires_complete_unique_masked_inactive_identity(self):
        workers=self.workers()
        good='Id=lg-collector-unit.service\nLoadState=masked\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\nUnitFileState=masked\nControlGroup=\n'
        def check(text):
            with patch.object(subprocess,'run',return_value=types.SimpleNamespace(returncode=0,stdout=text.encode())):
                MODULE['check_quiescence'](workers,self.budget)
        check(good)
        for before,after in [('Id=lg-collector-unit.service','Id=other.service'),
                             ('ActiveState=inactive','ActiveState=active'),('MainPID=0','MainPID=2'),
                             ('ControlPID=0','ControlPID=3'),('UnitFileState=masked','UnitFileState=disabled'),
                             ('ControlGroup=','ControlGroup=/../escape')]:
            with self.subTest(after=after),self.assertRaises(ValueError):check(good.replace(before,after))
        with self.assertRaises(ValueError):check(good+'Id=duplicate.service\n')

    def capsule(self):
        capsule=self.root/'capsule';capsule.mkdir(mode=0o700)
        files={}
        for name in LAUNCH['FILES']:
            p=capsule/name;shutil.copyfile(SOURCE/name,p);p.chmod(0o444)
            files[name]=hashlib.sha256(p.read_bytes()).hexdigest()
        raw=json.dumps({'version':1,'source_commit':'ab'*20,'files':files}).encode()
        (capsule/'TOOL.json').write_bytes(raw);(capsule/'TOOL.json').chmod(0o600)
        return capsule,hashlib.sha256(raw).hexdigest()

    def test_authenticated_capsule_and_wrong_digest(self):
        capsule,digest=self.capsule()
        self.assertEqual(LAUNCH['verify'](capsule,digest)['source_commit'],'ab'*20)
        with self.assertRaises(ValueError):LAUNCH['verify'](capsule,'aa'*32)

    def test_changed_tool_payload_refuses(self):
        capsule,digest=self.capsule();p=capsule/'collect-cashu-backup.py';p.chmod(0o600);p.write_bytes(b'changed')
        with self.assertRaises(ValueError):LAUNCH['verify'](capsule,digest)

    def test_extra_capsule_file_refuses(self):
        capsule,digest=self.capsule();(capsule/'unexpected').write_bytes(b'x')
        with self.assertRaises(ValueError):LAUNCH['verify'](capsule,digest)

    def test_actual_isolated_launcher_refuses_unreviewed_policy_before_destination(self):
        capsule=self.root/'actual-capsule';capsule.mkdir(mode=0o700)
        for name in LAUNCH['FILES']:
            shutil.copyfile(SOURCE/name,capsule/name);(capsule/name).chmod(0o444)
        manifest={'version':1,'source_commit':'0'*39+'1',
                  'files':{name:hashlib.sha256((capsule/name).read_bytes()).hexdigest() for name in LAUNCH['FILES']}}
        raw=json.dumps(manifest).encode();(capsule/'TOOL.json').write_bytes(raw);(capsule/'TOOL.json').chmod(0o600)
        policy=self.source('unreviewed-policy',b'{}')
        destination=self.root/'must-remain-absent'
        outcome=subprocess.run(['/usr/bin/python3','-I','-S','-B',str(capsule/'launch-cashu-backup.py'),
            hashlib.sha256(raw).hexdigest(),str(policy),'aa'*32,str(destination)],
            stdin=subprocess.DEVNULL,capture_output=True,timeout=10,
            env={'PATH':'/usr/bin:/bin','LC_ALL':'C'})
        self.assertEqual(outcome.returncode,1)
        self.assertEqual(outcome.stdout,b'')
        self.assertEqual(outcome.stderr,b'Cashu collection refused; preserve the incomplete private checkpoint.\n')
        self.assertFalse(destination.exists())


if __name__=='__main__':unittest.main()
