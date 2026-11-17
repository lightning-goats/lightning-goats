"""Root-only disposable filesystem and quiescence tests; no service mutations."""
import hashlib
import json
import os
from pathlib import Path
import pwd
import runpy
import shutil
import subprocess
import tempfile
import types
import unittest
from unittest.mock import patch

SOURCE=Path(__file__).resolve().parents[2]/'deploy/scripts'
MODULE=runpy.run_path(str(SOURCE/'collect-cashu-backup.py'))
LAUNCH=runpy.run_path(str(SOURCE/'launch-cashu-backup.py'))


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
