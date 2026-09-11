"""Assemble shipped sites with test-only paths/ports and real isolated nginx.

All listeners bind loopback. TLS uses an ephemeral self-signed certificate.
Upstreams are harmless local HTTP/WebSocket mocks; no daemon, provider or feeder
is contacted. The test starts and terminates only its own nginx subprocess.
"""

import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import http.client
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import unittest


EXAMPLES = Path(__file__).resolve().parents[1] / "nginx"


class MockBackend(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        self.server.calls.append((self.command, self.path, dict(self.headers)))
        if self.path == "/ws/overlay" and self.headers.get("Upgrade", "").lower() == "websocket":
            key = self.headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            self.send_response(101)
            self.send_header("Upgrade", "websocket")
            self.send_header("Connection", "Upgrade")
            self.send_header("Sec-WebSocket-Accept", base64.b64encode(hashlib.sha1(key.encode()).digest()).decode())
            self.end_headers()
            self.wfile.write(b"\x81\x02ok")
            self.wfile.flush()
            self.close_connection = True
            return
        data = json.dumps({"path": self.path, "backend": self.server.label}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.do_GET()


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


class NginxAssemblyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.nginx = shutil.which(os.environ.get("NGINX_BIN", "nginx"))
        if not cls.nginx:
            raise RuntimeError("nginx is required; install it or set NGINX_BIN")
        cls.directory = tempfile.TemporaryDirectory(prefix="lg-nginx-")
        cls.addClassCleanup(cls.directory.cleanup)
        cls.root = Path(cls.directory.name)
        # Permit an nginx worker that drops privileges to traverse test assets.
        cls.root.chmod(0o755)
        cls.backends = {}
        for mode in ("production", "canary"):
            backend = ThreadingHTTPServer(("127.0.0.1", 0), MockBackend)
            backend.calls = []
            backend.label = mode
            cls.addClassCleanup(backend.server_close)
            cls.addClassCleanup(backend.shutdown)
            threading.Thread(target=backend.serve_forever, daemon=True).start()
            cls.backends[mode] = backend
        subprocess.run([
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            "-subj", "/CN=localhost", "-keyout", str(cls.root / "key.pem"),
            "-out", str(cls.root / "cert.pem"),
        ], check=True, capture_output=True, timeout=30)
        (cls.root / "snippets").mkdir()
        common = (EXAMPLES / "lightning-goats-http.conf.example").read_text()
        common = common.replace("127.0.0.1:8787", f"127.0.0.1:{cls.backends['production'].server_port}")
        common = common.replace("127.0.0.1:8788", f"127.0.0.1:{cls.backends['canary'].server_port}")
        (cls.root / "http.conf").write_text(common)
        cls.ports = {}
        cls.hosts = {}
        for mode in cls.backends:
            for suffix in ("conf", "site.conf"):
                source = (EXAMPLES / f"lightning-goats-{mode}.{suffix}.example" if suffix == "conf"
                          else EXAMPLES / f"lightning-goats-{mode}-site.conf.example")
                data = source.read_text()
                if suffix == "conf":
                    (cls.root / "snippets" / f"lightning-goats-{mode}.conf").write_text(data)
                    continue
                tls_port, http_port = free_port(), free_port()
                cls.ports[mode] = (tls_port, http_port)
                host = "lightning-goats.com" if mode == "production" else "feeder.lightning-goats.com"
                cls.hosts[mode] = host
                data = data.replace("listen 443 ssl;", f"listen 127.0.0.1:{tls_port} ssl;")
                data = data.replace("listen 80;", f"listen 127.0.0.1:{http_port};")
                # No IPv6 dependency for local CI; deployed IPv6 reachability is a live gate.
                data = data.replace("listen [::]:443 ssl;", "").replace("listen [::]:80;", "")
                data = data.replace("/etc/nginx/", f"{cls.root}/")
                data = data.replace(f"/etc/letsencrypt/live/{host}/fullchain.pem", str(cls.root / "cert.pem"))
                data = data.replace(f"/etc/letsencrypt/live/{host}/privkey.pem", str(cls.root / "key.pem"))
                web = cls.root / f"web-{mode}"
                web.mkdir()
                (web / "index.html").write_text(f"static {mode}")
                # Reserved endpoints must be denied even if static files exist.
                for retired in ("invoice", "api", "lnbits", "v1"):
                    (web / retired).mkdir()
                    (web / retired / "forbidden").write_text("must not be served")
                data = data.replace(f"/var/www/lightning-goats-{mode}", str(web))
                (cls.root / f"{mode}.conf").write_text(data)
        config = cls.root / "nginx.conf"
        config.write_text(f"""
daemon off;
worker_processes 1;
pid {cls.root}/nginx.pid;
error_log {cls.root}/error.log;
events {{ worker_connections 128; }}
http {{
    access_log off;
    client_body_temp_path {cls.root}/body;
    proxy_temp_path {cls.root}/proxy;
    fastcgi_temp_path {cls.root}/fastcgi;
    uwsgi_temp_path {cls.root}/uwsgi;
    scgi_temp_path {cls.root}/scgi;
    include {cls.root}/http.conf;
    include {cls.root}/production.conf;
    include {cls.root}/canary.conf;
}}
""")
        command = [cls.nginx, "-p", str(cls.root), "-c", str(config)]
        subprocess.run(command + ["-t"], check=True, capture_output=True, timeout=10)
        cls.process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        cls.addClassCleanup(cls.stop_nginx)
        deadline = time.monotonic() + 5
        while True:
            if cls.process.poll() is not None:
                raise RuntimeError(cls.process.stderr.read().decode())
            try:
                with socket.create_connection(("127.0.0.1", cls.ports["production"][0]), timeout=0.1):
                    break
            except OSError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.025)

    @classmethod
    def stop_nginx(cls):
        cls.process.terminate()
        try:
            cls.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            cls.process.kill()
            cls.process.wait(timeout=5)
        cls.process.stderr.close()

    def request(self, mode, path, method="GET", body=None, headers=None, source="127.0.0.1"):
        connection = http.client.HTTPSConnection(
            "127.0.0.1", self.ports[mode][0], timeout=5,
            context=ssl._create_unverified_context(), source_address=(source, 0),
        )
        try:
            connection.request(method, path, body, {"Host": self.hosts[mode], **(headers or {})})
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def test_tls_static_redirect_and_routes(self):
        for mode in self.backends:
            with self.subTest(mode=mode):
                status, _, body = self.request(mode, "/")
                self.assertEqual((status, body), (200, f"static {mode}".encode()))
                for path in ("/.well-known/lnurlp/herd", "/lnurlp/goat.name/callback?amount=1000", "/api/v1/status", "/healthz"):
                    status, headers, body = self.request(mode, path)
                    self.assertEqual(status, 200, (path, body))
                    self.assertEqual(json.loads(body), {"path": path, "backend": mode})
                    forwarded = self.backends[mode].calls[-1][2]
                    self.assertEqual(forwarded["X-Forwarded-Proto"], "https")
                    self.assertEqual(forwarded["Host"], self.hosts[mode])
                    if "lnurlp" in path:
                        self.assertEqual(headers["Access-Control-Allow-Origin"], "*")
                connection = http.client.HTTPConnection("127.0.0.1", self.ports[mode][1], timeout=5)
                connection.request("GET", "/example?q=1", headers={"Host": self.hosts[mode]})
                response = connection.getresponse()
                self.assertEqual(response.status, 308)
                self.assertEqual(response.getheader("Location"), f"https://{self.hosts[mode]}/example?q=1")
                response.read()
                connection.close()

    def test_methods_unknown_retired_and_health_acl(self):
        for mode in self.backends:
            before = len(self.backends[mode].calls)
            for path in ("/.well-known/lnurlp/herd", "/lnurlp/herd/callback", "/api/v1/status", "/ws/overlay", "/healthz"):
                self.assertEqual(self.request(mode, path, "POST", b"")[0], 403)
            for path in ("/unknown", "/api/v1/admin", "/invoice/forbidden", "/api/forbidden", "/lnbits/forbidden", "/v1/forbidden", "/rest/items", "/clnrest", "/cyberherd", "/ws/admin", "/.well-known/lnurlp/" + "a" * 65):
                self.assertEqual(self.request(mode, path)[0], 404, path)
            self.assertEqual(self.request(mode, "/healthz", source="127.0.0.2")[0], 403)
            self.assertEqual(len(self.backends[mode].calls), before)

    def test_webhook_content_type_methods_and_size(self):
        path = "/api/v1/strike/webhook"
        for mode in self.backends:
            before = len(self.backends[mode].calls)
            self.assertEqual(self.request(mode, path, "GET", headers={"Content-Type": "application/json"})[0], 403)
            self.assertEqual(self.request(mode, path, "POST", b"{}", {"Content-Type": "text/plain"})[0], 415)
            self.assertEqual(self.request(mode, path, "POST", b"x" * 32769, {"Content-Type": "application/json"})[0], 413)
            self.assertEqual(len(self.backends[mode].calls), before)
            self.assertEqual(self.request(mode, path, "POST", b"{}", {"Content-Type": "application/json; charset=utf-8"})[0], 200)
            self.assertEqual(len(self.backends[mode].calls), before + 1)

    def test_websocket_upgrade_and_frame(self):
        for mode in self.backends:
            context = ssl._create_unverified_context()
            with socket.create_connection(("127.0.0.1", self.ports[mode][0]), timeout=5) as raw:
                with context.wrap_socket(raw, server_hostname=self.hosts[mode]) as stream:
                    stream.sendall((f"GET /ws/overlay HTTP/1.1\r\nHost: {self.hosts[mode]}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
                    response = b""
                    while b"\r\n\r\n" not in response:
                        chunk = stream.recv(4096)
                        self.assertTrue(chunk)
                        response += chunk
                    header, frame = response.split(b"\r\n\r\n", 1)
                    self.assertIn(b"101 Switching Protocols", header)
                    self.assertIn(b"s3pPLMBiTxaQ9kYGzzhZRbK+xOo=", header)
                    while len(frame) < 4:
                        chunk = stream.recv(4 - len(frame))
                        self.assertTrue(chunk)
                        frame += chunk
                    self.assertEqual(frame[:4], b"\x81\x02ok")

    def test_invoice_burst_is_bounded_before_upstream(self):
        for mode in self.backends:
            before = len(self.backends[mode].calls)
            statuses = [self.request(mode, "/lnurlp/herd/callback?amount=1000", source="127.0.0.3")[0] for _ in range(12)]
            self.assertIn(200, statuses)
            self.assertIn(429, statuses)
            self.assertTrue(set(statuses) <= {200, 429}, statuses)
            self.assertEqual(len(self.backends[mode].calls) - before, statuses.count(200))
