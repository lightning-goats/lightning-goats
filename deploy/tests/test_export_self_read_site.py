"""Exact Git-object export of public self-read bytes and retained browser pins."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('self_read_export_fixture',Path(__file__).resolve().parents[1]/'scripts/export-self-read-site.py')
E=importlib.util.module_from_spec(spec);spec.loader.exec_module(E)


class SelfReadExport(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory();self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name);self.repo=self.root/'repo';self.repo.mkdir()
        self.git('init','-q');self.git('config','user.name','Fixture');self.git('config','user.email','fixture@example.invalid')
        for name in [*E.FILES.values(),*('web/'+n for n in E.BROWSER)]:
            path=self.repo/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text('committed '+name)
        self.git('add','.');self.git('commit','-qm','fixture');self.source=self.git('rev-parse','HEAD').strip()

    def git(self,*args):
        return subprocess.check_output(['/usr/bin/git','-C',str(self.repo),*args],text=True)

    def test_dirty_checkout_does_not_change_exported_bytes_or_browser_pins(self):
        for name in E.FILES.values():(self.repo/name).write_text('uncommitted substitute')
        with patch.object(E.os,'geteuid',return_value=1000):
            result=E.export(self.repo,self.source,self.root/'payload')
        manifest=json.loads((self.root/'payload/SOURCE.json').read_text())
        for name,path in E.FILES.items():
            data=('committed '+path).encode()
            self.assertEqual((self.root/'payload'/name).read_bytes(),data)
            self.assertEqual(manifest['files'][name],hashlib.sha256(data).hexdigest())
        self.assertEqual(set(manifest['browser_files']),set(E.BROWSER))
        self.assertEqual(result['manifest_sha256'],hashlib.sha256((self.root/'payload/SOURCE.json').read_bytes()).hexdigest())
        self.assertFalse(result['installed']);self.assertFalse(result['executed'])

    def test_root_or_ref_alias_refused_before_git_or_write(self):
        with patch.object(E.os,'geteuid',return_value=0),patch.object(E.subprocess,'check_output',side_effect=AssertionError('git reached')):
            with self.assertRaises(ValueError):E.export(self.repo,self.source,self.root/'bad')
        with patch.object(E.os,'geteuid',return_value=1000):
            with self.assertRaises(ValueError):E.export(self.repo,'HEAD',self.root/'bad')
        self.assertFalse((self.root/'bad').exists())

    def test_committed_alias_and_reused_destination_refused(self):
        with patch.object(E.os,'geteuid',return_value=1000):
            E.export(self.repo,self.source,self.root/'one')
            with self.assertRaises(FileExistsError):E.export(self.repo,self.source,self.root/'one')
            path=self.repo/E.FILES['cyberherd-auth.js'];path.unlink();path.symlink_to('/dev/null')
            self.git('add','.');self.git('commit','-qm','alias')
            with self.assertRaises(ValueError):E.export(self.repo,self.git('rev-parse','HEAD').strip(),self.root/'bad')
        self.assertFalse((self.root/'bad').exists())
