"""Real nginx/connector/CRS acceptance; loopback mocks only, no live services.

Usage: python3 -B deploy/waf/acceptance.py /path/to/coreruleset-4.30.0 /path/to/module.so
LG_WAF_MODE=DetectionOnly exercises the tuning stage; default On proves blocking.
"""
import hashlib
import hmac
import json
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tests"))
from test_nginx import NginxAssemblyTests
from test_cyberherd_authenticated_nginx import transport_header


class WafAcceptance(NginxAssemblyTests):
    CONTACT_ENABLED = True
    CYBERHERD_AUTH_ENABLED = True

    def test_cyberherd_operator_payload_and_authorization_pass_without_private_logs(self):
        path = "/api/v1/cyberherd/operator/command"
        sentinel = "LG_PRIVATE_OPERATOR_5a692e"
        body = json.dumps({"id": "10000000-0000-0000-0000-000000000001", "expected_revision": 7,
            "action": {"type": "template", "edit": {"Set": {"category": "member_added",
                "variant": "example", "text": sentinel + " Welcome {member_name}"}}}}).encode()
        authorization = transport_header(path, "POST", body)
        status, headers, _ = self.request("production", path, "POST", body,
            {"Content-Type": "application/json", "Origin": "https://lightning-goats.com",
             "Authorization": authorization}, source="127.0.4.1")
        self.assertEqual(status, 200)
        self.assertEqual(headers["Cache-Control"], "no-store")
        self.assertEqual(self.backends["production"].bodies[-1], body)
        self.assertEqual(self.backends["production"].calls[-1][2]["Authorization"], authorization)
        for file in self.root.rglob("*.log"):
            text = file.read_text(errors="replace")
            self.assertNotIn(sentinel, text)
            self.assertNotIn(authorization, text)

    def test_private_contact_body_and_status_are_not_logged(self):
        sentinel = "LG_PRIVATE_CONTACT_66d8af"
        body = json.dumps({"capability": sentinel, "reply_email": sentinel + "@example.org",
                           "envelope": {"content": "Ag" + "a" * 180}}).encode()
        status, _, _ = self.request("production", "/api/v1/contact", "POST", body,
                                    {"Content-Type": "application/json", "Origin": "https://lightning-goats.com"})
        self.assertEqual(status, 200)
        self.assertEqual(self.backends["production"].bodies[-1], body)
        for file in self.root.rglob("*.log"):
            self.assertNotIn(sentinel, file.read_text(errors="replace"), str(file))

    def test_exact_signed_body_and_headers(self):
        body = b'{ "id": "synthetic-private-body", "eventType": "receive-request.receive-completed" }\n'
        signature = hmac.new(b"synthetic-test-secret", body, hashlib.sha256).hexdigest()
        headers = {"Content-Type": "application/json", "X-Webhook-Signature": signature}
        status, _, _ = self.request("production", "/api/v1/strike/webhook", "POST", body, headers)
        self.assertEqual(status, 200)
        backend = self.backends["production"]
        self.assertEqual(backend.bodies[-1], body)
        self.assertEqual(backend.calls[-1][2]["X-Webhook-Signature"], signature)

    def test_crs_and_engine_enforcement(self):
        for path, headers in [
            ("/api/v1/status", {"X-Lightning-Goats-Waf-Probe": "blocked"}),
            ("/api/v1/status?q=1%27%20OR%201%3D1%20--%20", {}),
            ("/api/v1/status?q=%3Cscript%3Ealert(1)%3C%2Fscript%3E", {}),
        ]:
            with self.subTest(path=path, headers=headers):
                before = len(self.backends["production"].calls)
                status, _, body = self.request("production", path, headers=headers)
                if os.environ.get("LG_WAF_MODE", "On") == "On":
                    self.assertEqual(status, 403, body)
                    self.assertEqual(len(self.backends["production"].calls), before)
                else:
                    self.assertEqual(status, 200, body)

    def test_matched_data_is_not_logged(self):
        sentinel = "LG_PRIVATE_SENTINEL_12d937e"
        self.request("production", "/api/v1/status?q=%3Cscript%3E" + sentinel + "%3C%2Fscript%3E",
                     headers={"Authorization": "Bearer " + sentinel, "Cookie": "session=" + sentinel})
        for file in self.root.rglob("*.log"):
            self.assertNotIn(sentinel, file.read_text(errors="replace"), str(file))


class SelfReadWaf(unittest.TestCase):
    CYBERHERD_SELF_READ_ENABLED = True
    setUpClass = classmethod(NginxAssemblyTests.setUpClass.__func__)
    stop_nginx = classmethod(NginxAssemblyTests.stop_nginx.__func__)
    request = NginxAssemblyTests.request

    def test_signed_self_transport_and_private_headers_with_crs(self):
        path = '/api/v1/cyberherd/me'
        authorization = transport_header(path, 'GET')
        status, headers, _ = self.request('production', path, headers={
            'Authorization': authorization, 'Origin': 'https://lightning-goats.com'}, source='127.0.7.1')
        self.assertEqual(status, 200)
        self.assertEqual(headers['Cache-Control'], 'no-store')
        self.assertEqual(self.backends['production'].calls[-1][2]['Authorization'], authorization)
        for file in self.root.rglob('*.log'):
            self.assertNotIn(authorization, file.read_text(errors='replace'))

    def test_self_probe_blocks_and_operator_route_remains_closed(self):
        before = len(self.backends['production'].calls)
        status, _, _ = self.request('production', '/api/v1/cyberherd/me',
            headers={'X-Lightning-Goats-Waf-Probe': 'blocked'}, source='127.0.7.2')
        self.assertEqual(status, 403 if os.environ.get('LG_WAF_MODE', 'On') == 'On' else 200)
        expected = before if os.environ.get('LG_WAF_MODE', 'On') == 'On' else before + 1
        self.assertEqual(len(self.backends['production'].calls), expected)
        self.assertEqual(self.request('production', '/api/v1/cyberherd/operator/state')[0], 404)
        self.assertEqual(len(self.backends['production'].calls), expected)


class CashuWaf(unittest.TestCase):
    CASHU_ENABLED = True
    setUpClass = classmethod(NginxAssemblyTests.setUpClass.__func__)
    stop_nginx = classmethod(NginxAssemblyTests.stop_nginx.__func__)
    request = NginxAssemblyTests.request

    def test_cashu_routes_proxy_without_logging_private_material(self):
        marker = 'SYNTHETIC_CASHU_EDGE_7083'
        headers = {'Content-Type': 'application/json', 'Origin': 'https://wallet.example'}
        proof = {'amount': 1, 'id': '00' + 'ad' * 7, 'secret': marker,
                 'C': '02' + 'ad' * 32, 'dleq': {field: 'ac' * 32 for field in ('e', 's', 'r')}}
        routes = [
            ('OPTIONS', '/api/v1/cashu', None),
            ('POST', '/api/v1/cashu', {'id': 'lg1-' + 'ad' * 16, 'unit': 'sat',
                                      'mint': 'https://mint.example', 'proofs': [proof]}),
            ('GET', '/api/v1/cashu/request', None),
            ('POST', '/api/v1/cashu/status', {'capability': 'a1' * 32}),
        ]
        backend = self.backends['production']
        for index, (method, path, data) in enumerate(routes, 1):
            body = json.dumps(data, separators=(',', ':')).encode() if data is not None else None
            request_headers = dict(headers)
            if method == 'OPTIONS':
                request_headers.update({'Access-Control-Request-Method': 'POST',
                                        'Access-Control-Request-Headers': 'content-type'})
            status, _, _ = self.request('production', path, method, body, request_headers,
                                        source=f'127.0.8.{index}')
            self.assertEqual(status, 200, (method, path))
            self.assertEqual(backend.calls[-1][:2], (method, path))
            if body is not None:
                self.assertEqual(backend.bodies[-1], body)
            self.assertEqual(self.request('canary', path, method, body, headers, source=f'127.0.8.{index}')[0], 404)
        protected = len(backend.calls)
        status, _, _ = self.request('production', f'/api/v1/cashu/status?capability={marker}', 'POST',
                                    b'{}', headers, source='127.0.8.5')
        self.assertEqual(status, 400)
        self.assertEqual(len(backend.calls), protected)
        for path in self.root.rglob('*.log'):
            data = path.read_bytes()
            self.assertNotIn(marker.encode(), data)
            self.assertNotIn(('a1' * 32).encode(), data)

    def test_cashu_waf_enforces_the_wallet_body_boundary(self):
        if os.environ.get('LG_WAF_MODE', 'On') != 'On':
            self.skipTest('body rejection requires enforcement mode')
        path = '/api/v1/cashu'
        headers = {'Content-Type': 'application/json'}
        backend = self.backends['production']
        for index, length, expected in [(1, 65536, 200), (2, 65537, 403)]:
            body = b'{}' + b' ' * (length - 2)
            before = len(backend.calls)
            status, _, _ = self.request('production', path, 'POST', body, headers, source=f'127.0.9.{index}')
            self.assertEqual(status, expected)
            self.assertEqual(len(backend.calls), before + (expected == 200))
            if expected == 200:
                self.assertEqual(backend.bodies[-1], body)
        before = len(backend.calls)
        status, _, _ = self.request('production', '/api/v1/strike/webhook', 'POST',
                                    b'{}' + b' ' * 32767, headers, source='127.0.9.3')
        self.assertEqual(status, 413)
        self.assertEqual(len(backend.calls), before)

    def test_inherited_waf_still_blocks_on_all_three_cashu_routes(self):
        backend = self.backends['production']
        for index, (method, path) in enumerate([('POST', '/api/v1/cashu'),
                                                ('GET', '/api/v1/cashu/request'),
                                                ('POST', '/api/v1/cashu/status')], 1):
            before = len(backend.calls)
            body = b'{}' if method == 'POST' else None
            status, _, _ = self.request('production', path, method, body,
                {'Content-Type': 'application/json', 'X-Lightning-Goats-Waf-Probe': 'blocked'},
                source=f'127.0.10.{index}')
            enforcing = os.environ.get('LG_WAF_MODE', 'On') == 'On'
            self.assertEqual(status, 403 if enforcing else 200)
            self.assertEqual(len(backend.calls), before + (not enforcing))


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    os.environ["LG_WAF_CRS"], os.environ["LG_WAF_MODULE"] = sys.argv[1:]
    unittest.main(argv=[sys.argv[0]], defaultTest=("WafAcceptance", "SelfReadWaf", "CashuWaf"), verbosity=2)
