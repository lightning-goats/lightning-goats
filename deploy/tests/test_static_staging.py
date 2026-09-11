"""Real nginx, loopback only, ephemeral TLS, no provider/owner connections."""
import http.client
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import tempfile
import time
import unittest

from test_nginx import free_port


class StaticStagingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        nginx = shutil.which(os.environ.get("NGINX_BIN", "nginx"))
        if not nginx:
            raise RuntimeError("nginx required")
        cls.directory = tempfile.TemporaryDirectory(prefix="lg-static-")
        cls.addClassCleanup(cls.directory.cleanup)
        root = Path(cls.directory.name)
        root.chmod(0o755)
        web = root / "web"
        web.mkdir()
        repo = Path(__file__).resolve().parents[2]
        shutil.copytree(repo / "web", web, dirs_exist_ok=True)
        # Public static allowlist must win over accidental copies of private or
        # retired files, and the server must override a payments-enabled asset.
        for name in [".env", "api/v1/status", "lnurlp/herd/callback", "backup.json"]:
            p = web / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text("must not be exposed")
        (web / "site-config.js").write_text("window.LightningGoatsSite={paymentsEnabled:true};")
        acme = root / "acme/.well-known/acme-challenge"
        acme.mkdir(parents=True)
        (acme / "test_token-123").write_text("harmless challenge")
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                        "-subj", "/CN=feeder.lightning-goats.com", "-keyout", str(root / "key.pem"),
                        "-out", str(root / "cert.pem")], check=True, capture_output=True, timeout=30)
        cls.http_port, cls.tls_port = free_port(), free_port()
        site = (repo / "deploy/nginx/lightning-goats-static-staging.conf.example").read_text()
        site = site.replace("64.177.40.118:80", f"127.0.0.1:{cls.http_port}")
        site = site.replace("64.177.40.118:443", f"127.0.0.1:{cls.tls_port}")
        site = site.replace("/var/www/lightning-goats-static-staging", str(web))
        site = site.replace("/var/lib/lightning-goats-acme", str(root / "acme"))
        site = site.replace("/etc/letsencrypt/live/feeder.lightning-goats.com/fullchain.pem", str(root / "cert.pem"))
        site = site.replace("/etc/letsencrypt/live/feeder.lightning-goats.com/privkey.pem", str(root / "key.pem"))
        config = root / "nginx.conf"
        config.write_text(f"""daemon off;
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
{site}
}}
""")
        command = [nginx, "-p", str(root), "-c", str(config)]
        subprocess.run(command + ["-t"], check=True, capture_output=True, timeout=10)
        cls.process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        cls.addClassCleanup(cls.stop_nginx)
        deadline = time.monotonic() + 5
        while True:
            if cls.process.poll() is not None:
                raise RuntimeError(cls.process.stderr.read().decode())
            try:
                with socket.create_connection(("127.0.0.1", cls.tls_port), timeout=0.1):
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

    def request(self, path, method="GET", host="feeder.lightning-goats.com", tls=True):
        if tls:
            connection = http.client.HTTPSConnection("127.0.0.1", self.tls_port, timeout=5,
                                                     context=ssl._create_unverified_context())
        else:
            connection = http.client.HTTPConnection("127.0.0.1", self.http_port, timeout=5)
        try:
            connection.request(method, path, headers={"Host": host})
            r = connection.getresponse()
            return r.status, dict(r.getheaders()), r.read()
        finally:
            connection.close()

    def test_assets_and_payment_disable_override(self):
        for path, mime in [("/", "text/html"), ("/index.html", "text/html"),
                           ("/site.js", "application/javascript"), ("/chat.js", "application/javascript"),
                           ("/images/preview-image.png", "image/png"),
                           ("/images/lightninggoatslogo1.png", "image/png")]:
            status, headers, body = self.request(path)
            self.assertEqual(status, 200, path)
            self.assertEqual(headers["Content-Type"], mime)
            self.assertTrue(body)
        status, headers, body = self.request("/site-config.js")
        self.assertEqual(status, 200)
        self.assertIn(b"paymentsEnabled: false", body)
        self.assertNotIn(b"true", body)
        self.assertEqual(headers["Cache-Control"], "no-store")

    def test_application_and_unlisted_files_are_inaccessible(self):
        for path in ["/.well-known/lnurlp/herd", "/lnurlp/herd/callback?amount=2340000",
                     "/api/v1/strike/webhook", "/api/v1/status", "/healthz", "/ws/overlay",
                     "/rest/items", "/invoice", "/.env", "/backup.json", "/images/", "/site.js/extra"]:
            self.assertEqual(self.request(path)[0], 404, path)
        for path in ["/", "/site-config.js", "/site.js", "/api/v1/strike/webhook"]:
            self.assertEqual(self.request(path, "POST")[0], 405, path)
        self.assertEqual(self.request("/", host="lightning-goats.com")[0], 421)
        self.assertEqual(self.request("/", "HEAD")[2], b"")

    def test_http_challenge_and_fixed_redirect(self):
        self.assertEqual(self.request("/.well-known/acme-challenge/test_token-123", tls=False)[::2],
                         (200, b"harmless challenge"))
        self.assertEqual(self.request("/.well-known/acme-challenge/missing", tls=False)[0], 404)
        self.assertEqual(self.request("/.well-known/acme-challenge/test_token-123", "POST", tls=False)[0], 405)
        status, headers, _ = self.request("/example?q=1", tls=False)
        self.assertEqual(status, 308)
        self.assertEqual(headers["Location"], "https://feeder.lightning-goats.com/example?q=1")
        self.assertEqual(self.request("/", host="untrusted.example", tls=False)[0], 421)
