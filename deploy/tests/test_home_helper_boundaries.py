"""Offline regressions for PR59 review: no credentials, services or live Items."""
from email.message import Message
import importlib.util
import io
import json
from pathlib import Path
import sys
import stat
import tempfile
import os
from types import SimpleNamespace
import tomllib
import unittest
import urllib.error
import urllib.request
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
sys.path.insert(0, str(SCRIPTS))
import home_gateway_safety as safety


def load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RedirectBoundaryTests(unittest.TestCase):
    def test_all_redirect_statuses_fail_without_a_second_request(self):
        # Exercise the real urllib handler chain with only its HTTP transport mocked.
        for code in (301, 302, 303, 307, 308):
            for method in ('GET', 'POST'):
                for location in ('http://example.invalid/steal', '/same-origin'):
                    with self.subTest(code=code, method=method, location=location):
                        requests = []
                        def transport(handler, request):
                            requests.append(request)
                            headers = Message()
                            headers['Location'] = location
                            response = urllib.response.addinfourl(io.BytesIO(b''), headers, request.full_url, code)
                            response.msg = 'redirect'
                            return response
                        request = urllib.request.Request('http://127.0.0.1:8080/rest/rules',
                            data=b'' if method == 'POST' else None, method=method,
                            headers={'Authorization': 'Basic SYNTHETIC'})
                        with patch.object(urllib.request.HTTPHandler, 'http_open', transport):
                            with self.assertRaises(urllib.error.HTTPError) as error:
                                safety.local_opener().open(request)
                        self.assertEqual(error.exception.code, code)
                        self.assertEqual(len(requests), 1)

    def test_direct_permission_responses_preserved_and_proxy_disabled(self):
        for code in (200, 401, 403):
            def transport(handler, request):
                response = urllib.response.addinfourl(io.BytesIO(b'OK'), Message(), request.full_url, code)
                response.msg = 'direct'
                return response
            with patch.dict('os.environ', {'http_proxy': 'http://example.invalid:9999'}):
                opener = safety.local_opener()
                self.assertFalse(any(isinstance(h, urllib.request.ProxyHandler) and h.proxies for h in opener.handlers))
                with patch.object(urllib.request.HTTPHandler, 'http_open', transport):
                    if code == 200:
                        self.assertEqual(opener.open('http://127.0.0.1:8080/').read(), b'OK')
                    else:
                        with self.assertRaises(urllib.error.HTTPError) as error:
                            opener.open('http://127.0.0.1:8080/')
                        self.assertEqual(error.exception.code, code)


class ProvisioningFileTests(unittest.TestCase):
    def test_sudo_invoker_private_file_allowed_but_other_owner_and_public_file_rejected(self):
        module = load('prepare-home-openhab-canary')
        with tempfile.TemporaryDirectory() as directory:
            env = Path(directory) / 'synthetic.env'
            env.write_text('OPENHAB_TOKEN=synthetic-only\n')
            env.chmod(0o600)
            uid = env.stat().st_uid
            with patch('os.geteuid', return_value=0), patch.dict(os.environ, {'SUDO_UID': str(uid)}):
                self.assertEqual(module.token_from_file(env), 'synthetic-only')
                env.chmod(0o644)
                with self.assertRaises(ValueError):
                    module.token_from_file(env)
            with patch.object(Path, 'lstat', return_value=SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=98765)), \
                 patch('os.geteuid', return_value=0), patch.dict(os.environ, {'SUDO_UID': '1000'}):
                with self.assertRaises(ValueError):
                    module.token_from_file(env)


class CanaryTargetTests(unittest.TestCase):
    def test_alternate_origin_and_database_fail_before_service_or_http(self):
        checker = load('check-home-canary')
        config, unit = safety.canonical_files()
        for altered in (config.replace(':8080/', ':8081/'), config.replace(':8080/', ':8080/other/'),
                        config.replace('/var/lib/lightning-goats-gateway-canary/gateway.db', '/tmp/other.db'),
                        config.replace('uuid_canary', 'feeder_request_v1')):
            with self.subTest(config=altered), patch.object(safety, 'root_file', side_effect=[altered, unit]), \
                 patch.object(checker.subprocess, 'run') as mutation, patch.object(safety, 'local_opener') as http, \
                 patch('os.geteuid', return_value=0):
                with self.assertRaises(ValueError):
                    checker.apply(Path('/unused-secret'))
                mutation.assert_not_called()
                http.assert_not_called()

    def test_unsafe_binary_rejected_before_restart(self):
        checker = load('check-home-canary')
        config, unit = safety.canonical_files()
        for mode, uid in ((stat.S_IFREG | 0o775, 0), (stat.S_IFREG | 0o755, 995),
                          (stat.S_IFLNK | 0o777, 0)):
            def info(path):
                if path == safety.BINARY:
                    return SimpleNamespace(st_mode=mode, st_uid=uid)
                return SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_uid=0)
            with patch.object(safety, 'root_file', side_effect=[config, unit]), \
                 patch.object(Path, 'lstat', info), patch.object(checker.subprocess, 'run') as mutation, \
                 patch('os.geteuid', return_value=0):
                with self.assertRaises(ValueError):
                    checker.apply(Path('/unused-secret'))
                mutation.assert_not_called()

    def test_default_config_and_effective_unit_control(self):
        safety.validate_source(*safety.canonical_files())
        props = {'FragmentPath': str(safety.UNIT), 'DropInPaths': '', 'NeedDaemonReload': 'no',
                 'User': safety.STEM, 'Group': safety.STEM, 'Environment': 'RUST_LOG=info'}
        safety.validate_properties(props)
        for field, value in [('FragmentPath', '/run/other.service'), ('DropInPaths', '/etc/override.conf'),
                             ('NeedDaemonReload', 'yes'), ('User', 'root'),
                             ('Environment', 'CREDENTIALS_DIRECTORY=/tmp'), ('EnvironmentFiles', '/tmp/env')]:
            bad = dict(props, **{field: value})
            with self.subTest(field=field), self.assertRaises(ValueError):
                safety.validate_properties(bad)

    def test_rule_consumer_inventory_fetches_full_rules(self):
        checker = load('check-home-canary')
        calls = []
        def oh(path):
            calls.append(path)
            return json.dumps([{'uid': checker.canary.RULE}, {'uid': 'other'}]) if path == 'rules' else json.dumps({'configuration': 'unrelated'})
        checker.verify_other_consumers(oh)
        self.assertEqual(calls, ['rules', 'rules/other'])
        def unsafe(path):
            return oh(path) if path == 'rules' else '{"script":"LightningGoatsCanary\\u0052equest"}'
        with self.assertRaises(ValueError):
            checker.verify_other_consumers(unsafe)


class RehearsalIsolationTests(unittest.TestCase):
    def test_default_derives_only_separate_state_config_and_credential(self):
        rehearsal = load('rehearse-home-gateway')
        config, unit = rehearsal.render(*safety.canonical_files())
        parsed = tomllib.loads(config)
        self.assertEqual(parsed['database']['url'], 'sqlite://' + str(rehearsal.STATE / 'gateway.db'))
        self.assertEqual(parsed['service']['listen'], '127.0.0.1:18790')
        self.assertNotIn(str(safety.CONFIG), unit)
        self.assertNotIn('/etc/credstore.encrypted/', unit)
        self.assertIn('LoadCredentialEncrypted=openhab-token:' + str(rehearsal.RUN / 'synthetic.cred'), unit)
        self.assertIn('StateDirectory=' + rehearsal.NAME + '\n', unit)

    def test_alternate_database_and_unit_directives_rejected_before_mutations(self):
        rehearsal = load('rehearse-home-gateway')
        config, unit = safety.canonical_files()
        variants = [(config.replace('/var/lib/lightning-goats-gateway-canary/gateway.db', '/tmp/alternate.db'), unit)]
        for old, new in [('ExecStart=', 'ExecStart=/bin/echo '), ('StateDirectory=', 'StateDirectory=other\n#'),
                         ('LoadCredentialEncrypted=', 'LoadCredential=other:/tmp/cred\n#'),
                         ('RuntimeDirectory=', 'RuntimeDirectory=other\n#')]:
            variants.append((config, unit.replace(old, new)))
        for config, unit in variants:
            with self.subTest(unit=unit), self.assertRaises(ValueError):
                rehearsal.render(config, unit)
