"""Root-only disposable filesystem and quiescence tests; no service mutations."""
from contextlib import closing
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


class RecoveryWorklistTests(unittest.TestCase):
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
