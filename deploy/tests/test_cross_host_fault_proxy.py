import concurrent.futures
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
import uuid

SPEC = importlib.util.spec_from_file_location('fault_proxy', Path(__file__).resolve().parents[1] / 'scripts/cross-host-fault-proxy.py')
proxy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(proxy)


class FaultProxyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.calls = []
        self.status = 200
        self.body = b'{"status":"confirmed"}'
        outer = self

        class Upstream(BaseHTTPRequestHandler):
            def do_GET(self):
                outer.calls.append((self.command, self.path))
                self.send_response(outer.status)
                self.send_header('Content-Length', str(len(outer.body)))
                self.send_header('Location', 'http://127.0.0.1:1/forbidden')
                self.end_headers()
                self.wfile.write(outer.body)

            do_POST = do_GET

            def log_message(self, *args):
                pass

        self.upstream = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
        self.upstream_thread = threading.Thread(target=self.upstream.serve_forever)
        self.upstream_thread.start()
        self.addCleanup(self.stop, self.upstream, self.upstream_thread)
        self.path = Path(self.directory.name) / 'journal.jsonl'
        self.journal = proxy.Journal(self.path, str(uuid.uuid4()), self.upstream.server_address, True)
        self.addCleanup(self.journal.close)
        self.server = proxy.Proxy(0, self.upstream.server_address, self.journal, True)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        self.addCleanup(self.stop, self.server, self.thread)
        self.feed = '/v1/feeder/request/' + str(uuid.uuid4())

    @staticmethod
    def stop(server, thread):
        server.shutdown()
        thread.join(timeout=5)
        server.server_close()

    def request(self, method, path, headers=None):
        connection = http.client.HTTPConnection(*self.server.server_address, timeout=5)
        try:
            connection.request(method, path, headers=headers or {})
            response = connection.getresponse()
            return response.status, response.read()
        finally:
            connection.close()

    def test_response_loss_and_concurrent_duplicates_never_trigger_proxy_retry(self):
        def lost(_):
            with self.assertRaises(http.client.RemoteDisconnected):
                self.request('POST', self.feed)
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(lost, range(4)))
        self.assertEqual(self.calls, [('POST', self.feed)] * 4)
        self.assertEqual(self.request('GET', self.feed), (200, self.body))
        rows = [json.loads(line) for line in self.path.read_text().splitlines()]
        intents = [r for r in rows if r['stage'] == 'intent']
        responses = [r for r in rows if r['stage'] == 'response']
        self.assertEqual(len(intents), 5)
        self.assertEqual(len(responses), 5)
        self.assertEqual(sum(r['disposition'] == 'discarded' for r in responses), 4)
        self.assertEqual({r['exchange'] for r in intents}, {r['exchange'] for r in responses})
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            proxy.Journal(self.path, str(uuid.uuid4()), self.upstream.server_address, False)

    def test_narrow_routes_and_empty_body_only(self):
        for method, path, headers in [
            ('POST', '/rest/items/Feeder', {}), ('POST', '/healthz', {}),
            ('GET', self.feed + '?redirect=x', {}),
            ('POST', self.feed, {'Content-Length': '1'}),
            ('GET', self.feed, {'Transfer-Encoding': 'chunked'}),
        ]:
            self.assertEqual(self.request(method, path, headers)[0], 400)
        self.assertEqual(self.calls, [])

    def test_failed_durable_intent_prevents_upstream_and_stays_failed(self):
        with patch.object(proxy.os, 'fsync', side_effect=OSError('injected')):
            with self.assertRaises(http.client.RemoteDisconnected):
                self.request('POST', self.feed)
        with self.assertRaises(http.client.RemoteDisconnected):
            self.request('POST', self.feed)
        self.assertEqual(self.calls, [])
        self.assertTrue(self.journal.failed)

    def test_redirect_and_oversized_response_are_not_forwarded_or_retried(self):
        self.status = 302
        with self.assertRaises(http.client.RemoteDisconnected):
            self.request('GET', self.feed)
        self.status = 200
        self.body = b'x' * (proxy.LIMIT + 1)
        with self.assertRaises(http.client.RemoteDisconnected):
            self.request('GET', self.feed)
        self.assertEqual(self.calls, [('GET', self.feed)] * 2)

    def test_origin_rejects_dns_public_credentials_and_extra_paths(self):
        self.assertEqual(proxy.upstream_origin('http://10.8.0.6:8791'), ('10.8.0.6', 8791))
        for origin in ('http://example.com:80', 'http://8.8.8.8:80',
                       'http://user@127.0.0.1:80', 'http://127.0.0.1:80/rest',
                       'http://127.0.0.1:80/?x=1', 'http://127.0.0.1',
                       'http://[::1]:80', 'http://169.254.169.254:80'):
            with self.assertRaises(ValueError):
                proxy.upstream_origin(origin)
