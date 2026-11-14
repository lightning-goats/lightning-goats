"""Actual optional TLS proxy with harmless mocks; native auth is tested separately.

These transport fixtures deliberately do not claim a signature was accepted by
the member, an operator was authorized or production ingress was installed.
"""
import base64
import hashlib
import json
import unittest

import test_nginx

COMMAND = "/api/v1/cyberherd/operator/command"
READS = ("/api/v1/cyberherd/me", "/api/v1/cyberherd/operator/state",
         "/api/v1/cyberherd/operator/templates")


def transport_header(path, method, body=b""):
    # Signature-shaped data tests exact header transport, not cryptography.
    tags = [["u", "https://lightning-goats.com" + path], ["method", method]]
    if method == "POST":
        tags.append(["payload", hashlib.sha256(body).hexdigest()])
    event = {"id": "11" * 32, "pubkey": "22" * 32, "created_at": 1794370000,
             "kind": 27235, "tags": tags, "content": "", "sig": "33" * 64}
    return "Nostr " + base64.b64encode(json.dumps(event, separators=(",", ":")).encode()).decode()


class AuthenticatedCyberHerdNginxTests(unittest.TestCase):
    CYBERHERD_AUTH_ENABLED = True
    setUpClass = classmethod(test_nginx.NginxAssemblyTests.setUpClass.__func__)
    stop_nginx = classmethod(test_nginx.NginxAssemblyTests.stop_nginx.__func__)
    request = test_nginx.NginxAssemblyTests.request

    def test_exact_read_paths_preserve_query_authorization_and_origin(self):
        for index, path in enumerate((READS[0], READS[1] + "?offset=20&revision=7",
                                      READS[2] + "?category=member_added&offset=0&revision=7")):
            authorization = transport_header(path, "GET")
            status, headers, _ = self.request("production", path, headers={
                "Authorization": authorization, "Origin": "https://lightning-goats.com"},
                source=f"127.0.1.{index + 1}")
            self.assertEqual(status, 200)
            self.assertEqual(headers["Cache-Control"], "no-store")
            method, observed, forwarded = self.backends["production"].calls[-1]
            self.assertEqual((method, observed), ("GET", path))
            self.assertEqual(forwarded["Authorization"], authorization)
            self.assertEqual(forwarded["Origin"], "https://lightning-goats.com")

    def test_command_preserves_exact_payload_and_does_not_log_private_fields(self):
        sentinel = "LG_OPERATOR_PRIVATE_9b8d2f"
        body = ('{ "id": "10000000-0000-0000-0000-000000000001", "expected_revision": 7,\n'
                '"action": {"type":"template","edit":{"Set":{"category":"member_added",'
                '"variant":"test","text":"' + sentinel + ' ⚡ {member_name}"}}}}\n').encode()
        authorization = transport_header(COMMAND, "POST", body)
        status, headers, _ = self.request("production", COMMAND, "POST", body,
            {"Content-Type": "application/json", "Authorization": authorization,
             "Origin": "https://lightning-goats.com"}, source="127.0.1.10")
        self.assertEqual(status, 200)
        self.assertEqual(headers["Cache-Control"], "no-store")
        self.assertEqual(self.backends["production"].bodies[-1], body)
        self.assertEqual(self.backends["production"].calls[-1][2]["Authorization"], authorization)
        for path in self.root.rglob("*.log"):
            text = path.read_text(errors="replace")
            self.assertNotIn(sentinel, text)
            self.assertNotIn(authorization, text)

    def test_methods_oversize_unknown_paths_and_canary_never_reach_backend(self):
        before = {name: len(backend.calls) for name, backend in self.backends.items()}
        for index, path in enumerate(READS):
            for offset, method in enumerate(("POST", "PUT", "PATCH", "DELETE", "OPTIONS")):
                self.assertEqual(self.request("production", path, method, b"{}",
                    source=f"127.0.2.{index * 10 + offset + 1}")[0], 403)
        for offset, method in enumerate(("GET", "HEAD", "PUT", "PATCH", "DELETE", "OPTIONS")):
            self.assertEqual(self.request("production", COMMAND, method,
                source=f"127.0.3.{offset + 1}")[0], 403)
        self.assertEqual(self.request("production", COMMAND, "POST", b"x" * 16385,
            {"Content-Type": "application/json"}, source="127.0.3.10")[0], 413)
        for path in (*READS, COMMAND):
            self.assertEqual(self.request("canary", path)[0], 404)
        for path in (COMMAND + "/", READS[0] + "/extra", "/api/v1/cyberherd/operator/other"):
            self.assertEqual(self.request("production", path)[0], 404)
        self.assertEqual({name: len(backend.calls) for name, backend in self.backends.items()}, before)
