"""Minimal contact website deployment; root cases use actual disposable files.

nginx, systemd and the core readiness check are mocked. Real nginx/WAF and browser
semantics have their own tests; no production configuration is read here.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import test_upgrade_contact_prepare as preparation

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': preparation.TX}):
    spec = importlib.util.spec_from_file_location('private_contact_site', SCRIPTS / 'install-private-contact-site.py')
    E = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(E)

FLAGS = 'window.LightningGoatsSite = Object.freeze({ paymentsEnabled: true, feedingAvailable: false, cyberherdEnabled: true, cyberherdAuthEnabled: false });\n'


class ContactSiteBoundaries(unittest.TestCase):
    def test_flags_preserve_every_existing_choice_and_enable_only_contact(self):
        changed = E.site_flags(FLAGS)
        self.assertIn('feedingAvailable: false', changed)
        self.assertIn('paymentsEnabled: true', changed)
        self.assertIn('cyberherdAuthEnabled: false', changed)
        self.assertIn('contactPaymentsEnabled: true', changed)
        for invalid in [changed, FLAGS + 'extra()', FLAGS.replace('feedingAvailable: false,', ''),
                        FLAGS.replace('feedingAvailable: false', 'feedingAvailable: f()'),
                        FLAGS.replace('feedingAvailable: false', 'feedingAvailable: false, feedingAvailable: true')]:
            with self.assertRaises(ValueError):
                E.site_flags(invalid)

    def test_site_change_only_changes_release_root_and_adds_exact_include(self):
        old = '    root ' + str(E.WWW / ('a' * 40)) + ';\n' + E.ANCHOR + '    location / { try_files $uri =404; }\n'
        original, release, new = E.site_change(old, 'b' * 40)
        self.assertEqual(original, E.WWW / ('a' * 40))
        self.assertEqual(release, E.WWW / ('contact-' + 'b' * 40))
        self.assertEqual(new.replace(str(release), str(original)).replace('    include ' + str(E.SNIPPET) + ';\n', ''), old)
        for invalid in [old + old, old.replace(str(E.WWW), '/etc'), old.replace(E.ANCHOR, ''), new]:
            with self.assertRaises(ValueError):
                E.site_change(invalid, 'b' * 40)

    def test_direct_execution_and_missing_flag_refuse_before_helpers_or_host(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copyfile(SCRIPTS / 'install-private-contact-site.py', root / 'site.py')
            (root / 'maintain-contact-upgrade.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'site.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)
        with patch.object(E.C, 'root', side_effect=AssertionError('host accessed')):
            with self.assertRaisesRegex(ValueError, 'explicit contact site activation'):
                E.apply(Path('/missing'), 'a' * 64, False)


@unittest.skipUnless(os.geteuid() == 0, 'protected root website fixture runs separately')
class ContactSiteFiles(unittest.TestCase):
    def setUp(self):
        previous = os.umask(0o077)
        self.addCleanup(os.umask, previous)
        temporary = tempfile.TemporaryDirectory(prefix='lg-contact-site-', dir='/run')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.www, self.base, self.etc = (self.root / name for name in ('www', 'upgrades', 'etc'))
        for directory in (self.www, self.base, self.etc):
            directory.mkdir(mode=0o700)
        self.original = self.www / ('a' * 40)
        self.original.mkdir(mode=0o755)
        self.original.chmod(0o755)
        self.site, self.snippet = self.etc / 'site.conf', self.etc / 'contact.conf'
        self.write(self.site, ('    root ' + str(self.original) + ';\n' + E.ANCHOR).encode())
        for name in ('index.html', 'overlay.html', 'overlay.js', 'overlay.css', 'images/goat.png', 'vendor/retained.js'):
            path = self.original / name
            path.parent.mkdir(mode=0o755, exist_ok=True)
            path.parent.chmod(0o755)
            self.write(path, ('original ' + name).encode())
        self.write(self.original / 'site-config.js', FLAGS.encode())
        self.original_site = self.site.read_bytes()
        self.identity = {'source_commit': 'b' * 40, 'manifest_sha256': 'c' * 64}
        self.calls, self.nginx_checks = [], 0
        self.fail_new_config = False
        for obj, name, value in [(E, 'WWW', self.www), (E, 'SITE', self.site), (E, 'SNIPPET', self.snippet),
                (E.C, 'BASE', self.base), (E.C, 'root', lambda: None),
                (E.M, 'C', E.C),
                (E, 'verified_tool_identity', lambda: self.identity),
                (E, 'contact_ready', lambda: {'fixed': 'original private core configuration'}),
                (E.C, 'command', self.command)]:
            context = patch.object(obj, name, value)
            context.start()
            self.addCleanup(context.stop)
        self.before = E.inventory(self.original)
        self.payload = self.base / 'payload'
        self.payload.mkdir(mode=0o700)
        for name in E.PAYLOAD:
            self.write(self.payload / name, ('source-pinned ' + name).encode(), 0o600)
        manifest = {'version': 1, 'source_commit': self.identity['source_commit'], 'files': {
            name: hashlib.sha256((self.payload / name).read_bytes()).hexdigest() for name in E.PAYLOAD}}
        self.write(self.payload / 'SOURCE.json', json.dumps(manifest).encode(), 0o600)
        self.directory = self.base / 'prepared'
        self.directory.mkdir(mode=0o700)

    @staticmethod
    def write(path, data, mode=0o644):
        path.write_bytes(data)
        path.chmod(mode)

    def command(self, *arguments, **kwargs):
        self.calls.append(arguments)
        if arguments == ('/usr/sbin/nginx', '-t'):
            self.nginx_checks += 1
            if self.fail_new_config and self.nginx_checks == 2:
                raise ValueError('synthetic new nginx configuration refused')
        return ''

    def prepare(self):
        result = E.prepare(self.payload, E.U.digest(self.payload / 'SOURCE.json'), self.directory)
        self.assertEqual(result, {'prepared': True, 'services_changed': False, 'contact_site_enabled': False})
        self.assertEqual(self.calls, [])
        self.assertEqual(self.site.read_bytes(), self.original_site)
        return E.U.digest(self.directory / 'PREPARED.json')

    def test_minimal_release_preserves_artwork_flags_and_only_reloads_nginx(self):
        digest = self.prepare()
        self.assertTrue(E.apply(self.directory, digest, True)['contact_site_enabled'])
        self.assertEqual(self.calls, [('/usr/sbin/nginx', '-t'), ('/usr/sbin/nginx', '-t'),
                                     ('/usr/bin/systemctl', 'reload', 'nginx.service')])
        self.assertEqual(E.inventory(self.original), self.before)
        release = self.www / ('contact-' + 'b' * 40)
        for name in ('overlay.html', 'overlay.js', 'overlay.css', 'images/goat.png', 'vendor/retained.js'):
            self.assertEqual(E.TX.fingerprint(release / name), self.before[name])
        self.assertIn('contactPaymentsEnabled: true', (release / 'site-config.js').read_text())
        self.assertIn('feedingAvailable: false', (release / 'site-config.js').read_text())
        with self.assertRaisesRegex(ValueError, 'already began'):
            E.apply(self.directory, digest, True)

    def test_failed_nginx_validation_restores_only_site_and_never_reloads(self):
        digest = self.prepare()
        self.fail_new_config = True
        with self.assertRaisesRegex(ValueError, 'synthetic new nginx'):
            E.apply(self.directory, digest, True)
        self.assertEqual(self.nginx_checks, 3)
        self.assertEqual(self.site.read_bytes(), self.original_site)
        self.assertFalse(any('reload' in row for row in self.calls))
        self.assertEqual(E.inventory(self.original), self.before)
        self.assertTrue((self.directory / 'CONFIGURATION-RESTORED.json').exists())

    def test_unknown_site_or_artwork_changes_refuse_before_configuration_replacement(self):
        digest = self.prepare()
        release = self.www / ('contact-' + 'b' * 40)
        (release / 'images/goat.png').write_bytes(b'changed artwork')
        with self.assertRaisesRegex(ValueError, 'reviewed contact website'):
            E.apply(self.directory, digest, True)
        self.assertEqual(self.site.read_bytes(), self.original_site)
        self.assertEqual(self.calls, [])

    def test_source_mismatch_or_symlink_never_creates_a_release(self):
        (self.payload / 'contact.js').unlink()
        (self.payload / 'contact.js').symlink_to(self.original / 'index.html')
        with self.assertRaises(ValueError):
            E.prepare(self.payload, E.U.digest(self.payload / 'SOURCE.json'), self.directory)
        self.assertEqual(E.inventory(self.original), self.before)
        self.assertFalse((self.www / ('contact-' + 'b' * 40)).exists())
