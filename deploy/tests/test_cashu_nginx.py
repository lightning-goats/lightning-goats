"""Real isolated nginx Cashu routing, bounds and local log suppression."""
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
        size=int(self.headers.get('Content-Length','0'))
        received=self.rfile.read(size) if size else b''
        self.server.calls.append((self.command, self.path, received))
        body = b'{"version":1,"roster":{"members":[]}}'
        self.send_response(200 if self.server.available else 503)
        self.send_header('Cache-Control','no-store')
        self.send_header('Access-Control-Allow-Origin','*')
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_POST = do_PUT = do_PATCH = do_DELETE = do_OPTIONS = do_GET


class CashuNginxTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        binary = shutil.which('nginx')
        if not binary:
            raise RuntimeError('nginx is required for the Cashu proxy fixture')
        temporary = tempfile.TemporaryDirectory(prefix='lg-cashu-nginx-')
        cls.addClassCleanup(temporary.cleanup)
        root = Path(temporary.name)
        root.chmod(0o755)
        cls.root=root
        cls.backend = ThreadingHTTPServer(('127.0.0.1', 0), Backend)
        cls.backend.calls = []
        cls.backend.available = True
        cls.addClassCleanup(cls.backend.server_close)
        cls.addClassCleanup(cls.backend.shutdown)
        threading.Thread(target=cls.backend.serve_forever, daemon=True).start()
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            cls.port = listener.getsockname()[1]
        source = Path(__file__).resolve().parents[1] / 'nginx/lightning-goats-cashu.conf.example'
        snippet = source.read_text()
        (root / 'readonly.conf').write_text(snippet)
        config = root / 'nginx.conf'
        config.write_text(f'''
daemon off;
worker_processes 1;
pid {root}/nginx.pid;
error_log {root}/error.log;
events {{ worker_connections 64; }}
http {{
    access_log {root}/access.log;
    client_body_temp_path {root}/body;
    proxy_temp_path {root}/proxy;
    fastcgi_temp_path {root}/fastcgi;
    uwsgi_temp_path {root}/uwsgi;
    scgi_temp_path {root}/scgi;
    upstream lightning_goats_backend {{ server 127.0.0.1:{cls.backend.server_port}; }}
    limit_req_zone $binary_remote_addr zone=lightning_goats_lnurl_invoice:1m rate=10000r/s;
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
    def request(cls, method, path, body=None):
        connection = http.client.HTTPConnection('127.0.0.1', cls.port, timeout=3)
        try:
            connection.request(method, path, body=body, headers={'Host': 'lightning-goats.com','Content-Type':'application/json'})
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def setUp(self):
        self.backend.calls.clear()
        self.backend.available = True

    def test_exact_routes_preserve_bodies_cors_and_no_store(self):
        body=b'{"proofs":"synthetic-private-token-marker"}'
        for method,path,payload in [('POST','/api/v1/cashu',body),('OPTIONS','/api/v1/cashu',None),('GET','/api/v1/cashu/request',None),('OPTIONS','/api/v1/cashu/request',None),('POST','/api/v1/cashu/status',b'{"capability":"synthetic-only"}'),('OPTIONS','/api/v1/cashu/status',None)]:
            status,headers,_=self.request(method,path,payload)
            self.assertEqual(status,200)
            self.assertEqual(headers['Cache-Control'],'no-store')
            self.assertEqual(headers['Access-Control-Allow-Origin'],'*')
            self.assertEqual(self.backend.calls[-1],(method,path,payload or b''))

    def test_queries_methods_and_prefixes_are_rejected_before_upstream(self):
        for method,path,status in [('GET','/api/v1/cashu',405),('POST','/api/v1/cashu/request',405),('PUT','/api/v1/cashu',405),('POST','/api/v1/cashu?token=private-query-marker',400),('GET','/api/v1/cashu/request?token=private-query-marker',400),('GET','/api/v1/cashu/operator',404),('GET','/api/v1/cashu/request/extra',404),('GET','/api/v1/cashu/status',405),('POST','/api/v1/cashu/status?capability=private-query-marker',400),('POST','/api/v1/cashu/status/extra',404)]:
            self.assertEqual(self.request(method,path)[0],status)
        self.assertEqual(self.backend.calls,[])

    def test_oversized_bodies_are_not_forwarded_or_spooled(self):
        # Header-only refusal avoids sending a large body after edge rejection.
        for path,length in [('/api/v1/cashu',1024*1024+1),('/api/v1/cashu/status',513)]:
            connection=http.client.HTTPConnection('127.0.0.1',self.port,timeout=3)
            try:
                connection.putrequest('POST',path)
                connection.putheader('Content-Length',str(length))
                connection.endheaders()
                response=connection.getresponse()
                self.assertEqual(response.status,413)
                response.read()
            finally:
                connection.close()
        self.assertEqual(self.backend.calls,[])
        self.assertFalse(any(p.is_file() for p in (self.root/'body').rglob('*')))

    def test_private_body_and_query_do_not_enter_local_edge_logs(self):
        marker='fixture-token-log-marker'
        self.request('POST','/api/v1/cashu',('{"proofs":"'+marker+'"}').encode())
        self.request('POST','/api/v1/cashu?token='+marker)
        self.request('GET','/api/v1/cashu/request?token='+marker)
        self.request('POST','/api/v1/cashu/status',('{"capability":"'+marker+'"}').encode())
        self.request('POST','/api/v1/cashu/status?capability='+marker)
        for name in ['access.log','error.log']:
            file=self.root/name
            if file.exists(): self.assertNotIn(marker,file.read_text())
        self.assertFalse(any(p.is_file() for p in (self.root/'body').rglob('*')))
