"""A real disposable Git repository proves export uses pinned objects only."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/export-upgrade-tools.py'
SPEC = importlib.util.spec_from_file_location('export_tools', SCRIPT)
EXPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORT)


class ExportTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.repo = self.root / 'repo'
        self.repo.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        (self.repo / 'deploy/scripts').mkdir(parents=True)
        for name in EXPORT.FILES:
            (self.repo / 'deploy/scripts' / name).write_text('raise RuntimeError("must never execute")\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture')
        self.source = self.git('rev-parse', 'HEAD').strip()

    def git(self, *args):
        return subprocess.check_output(['/usr/bin/git', '-C', str(self.repo), *args], text=True)

    def test_pinned_bytes_deterministic_despite_dirty_checkout_and_git_environment(self):
        for name in EXPORT.FILES:
            (self.repo / 'deploy/scripts' / name).write_text('uncommitted substitution')
        with patch.dict(os.environ, {'GIT_DIR': '/missing', 'GIT_WORK_TREE': '/missing'}), patch.object(EXPORT.os, 'geteuid', return_value=1000):
            first = EXPORT.export(self.repo, self.source, self.root / 'one')
            second = EXPORT.export(self.repo, self.source, self.root / 'two')
        self.assertEqual(first, second)
        self.assertFalse(first['installed'])
        for name in [*EXPORT.FILES, 'TOOL.json']:
            a, b = self.root / 'one' / name, self.root / 'two' / name
            self.assertEqual(a.read_bytes(), b.read_bytes())
            self.assertEqual(a.stat().st_mode & 0o777, 0o600)
        self.assertIn('must never execute', (self.root / 'one' / EXPORT.FILES[0]).read_text())
        self.assertEqual(json.loads((self.root / 'one/TOOL.json').read_text())['source_commit'], self.source)

    def test_alias_source_and_reused_destination_refused(self):
        with patch.object(EXPORT.os, 'geteuid', return_value=1000):
            with self.assertRaises(ValueError):
                EXPORT.export(self.repo, 'HEAD', self.root / 'bad')
            EXPORT.export(self.repo, self.source, self.root / 'one')
            with self.assertRaises(FileExistsError):
                EXPORT.export(self.repo, self.source, self.root / 'one')
            target = self.repo / 'deploy/scripts' / EXPORT.FILES[0]
            target.unlink()
            target.symlink_to('/dev/null')
            self.git('add', '.')
            self.git('commit', '-qm', 'symlink fixture')
            with self.assertRaisesRegex(ValueError, 'regular committed'):
                EXPORT.export(self.repo, self.git('rev-parse', 'HEAD').strip(), self.root / 'bad')
        self.assertFalse((self.root / 'bad').exists())

    def test_root_refused_before_git_or_writes(self):
        with patch.object(EXPORT.os, 'geteuid', return_value=0), patch.object(EXPORT.subprocess, 'check_output', side_effect=AssertionError('git reached')):
            with self.assertRaisesRegex(ValueError, 'unprivileged'):
                EXPORT.export('/missing', self.source, self.root / 'bad')
