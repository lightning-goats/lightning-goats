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


class WafAcceptance(NginxAssemblyTests):
    CONTACT_ENABLED = True

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


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    os.environ["LG_WAF_CRS"], os.environ["LG_WAF_MODULE"] = sys.argv[1:]
    unittest.main(argv=[sys.argv[0]], defaultTest="WafAcceptance", verbosity=2)
