"""Real isolated nginx routing to a synthetic read-only membership backend."""
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import unittest


class Backend(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        self.server.calls.append((self.command, self.path))
        body = b'{"version":1,"roster":{"members":[]}}'
        self.send_response(200 if self.server.available else 503)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_POST = do_PUT = do_PATCH = do_DELETE = do_OPTIONS = do_GET


class ReadOnlyCyberHerdNginxTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        binary = shutil.which('nginx')
        if not binary:
            raise RuntimeError('nginx is required for the read-only proxy fixture')
        temporary = tempfile.TemporaryDirectory(prefix='lg-readonly-nginx-')
        cls.addClassCleanup(temporary.cleanup)
        root = Path(temporary.name)
        root.chmod(0o755)
        cls.backend = ThreadingHTTPServer(('127.0.0.1', 0), Backend)
        cls.backend.calls = []
        cls.backend.available = True
        cls.addClassCleanup(cls.backend.server_close)
        cls.addClassCleanup(cls.backend.shutdown)
        threading.Thread(target=cls.backend.serve_forever, daemon=True).start()
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            cls.port = listener.getsockname()[1]
        source = Path(__file__).resolve().parents[1] / 'cyberherd/readonly-nginx.conf.example'
        snippet = source.read_text().replace('127.0.0.1:3340', f'127.0.0.1:{cls.backend.server_port}')
        (root / 'readonly.conf').write_text(snippet)
        config = root / 'nginx.conf'
        config.write_text(f'''
daemon off;
worker_processes 1;
pid {root}/nginx.pid;
error_log {root}/error.log;
events {{ worker_connections 64; }}
http {{
    access_log off;
    client_body_temp_path {root}/body;
    proxy_temp_path {root}/proxy;
    fastcgi_temp_path {root}/fastcgi;
    uwsgi_temp_path {root}/uwsgi;
    scgi_temp_path {root}/scgi;
    limit_req_zone $binary_remote_addr zone=lightning_goats_read:1m rate=10000r/s;
    server {{
        listen 127.0.0.1:{cls.port};
        server_name lightning-goats.com;
        include {root}/readonly.conf;
        location ~ "^/(api|cyberherd|leaderboard|extensions)(/|$)" {{ return 404; }}
        location / {{ return 404; }}
    }}
}}
''')
        checked = subprocess.run([binary, '-t', '-p', str(root), '-c', str(config)], capture_output=True)
        if checked.returncode:
            raise RuntimeError('isolated nginx configuration refused: ' + checked.stderr.decode())
        cls.process = subprocess.Popen([binary, '-p', str(root), '-c', str(config)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        cls.addClassCleanup(cls.stop)
        for _ in range(50):
            if cls.process.poll() is not None:
                raise RuntimeError('isolated nginx failed: ' + cls.process.stderr.read().decode())
            try:
                cls.request('GET', '/missing')
                break
            except OSError:
                time.sleep(0.1)
        else:
            raise RuntimeError('isolated nginx readiness deadline')

    @classmethod
    def stop(cls):
        cls.process.terminate()
        try:
            cls.process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            cls.process.kill()
            cls.process.communicate(timeout=5)

    @classmethod
    def request(cls, method, path):
        connection = http.client.HTTPConnection('127.0.0.1', cls.port, timeout=3)
        try:
            connection.request(method, path, headers={'Host': 'lightning-goats.com'})
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def setUp(self):
        self.backend.calls.clear()
        self.backend.available = True

    def test_exact_read_proxy_preserves_pagination_and_aliases_override_retired_regex(self):
        status, headers, body = self.request('GET', '/api/v1/cyberherd?offset=0&limit=50&revision=3')
        self.assertEqual(status, 200)
        self.assertEqual(headers['Cache-Control'], 'no-store')
        self.assertIn(b'"version":1', body)
        self.assertEqual(self.backend.calls, [('GET', '/api/v1/cyberherd?offset=0&limit=50&revision=3')])
        for path in ('/leaderboard', '/cyberherd'):
            status, headers, _ = self.request('GET', path)
            self.assertEqual(status, 302)
            self.assertTrue(headers['Location'].endswith('/cyberherd.html'))
        self.assertEqual(len(self.backend.calls), 1)

    def test_mutations_operator_routes_and_prefix_paths_never_reach_backend(self):
        for method in ('POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'):
            self.assertEqual(self.request(method, '/api/v1/cyberherd')[0], 403)
        for path in ('/api/v1/cyberherd/me', '/api/v1/cyberherd/operator/state',
                     '/api/v1/cyberherd/operator/command', '/api/v1/cyberherd/',
                     '/cyberherd/operator', '/leaderboard/operator', '/extensions'):
            self.assertEqual(self.request('GET', path)[0], 404)
        self.assertEqual(self.backend.calls, [])

    def test_unavailable_view_is_not_replaced_by_a_stale_success(self):
        self.backend.available = False
        status, headers, _ = self.request('GET', '/api/v1/cyberherd')
        self.assertEqual(status, 503)
        self.assertEqual(headers['Cache-Control'], 'no-store')
        self.assertEqual(self.backend.calls, [('GET', '/api/v1/cyberherd')])
