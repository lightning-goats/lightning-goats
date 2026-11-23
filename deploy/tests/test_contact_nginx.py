"""Optional exact contact routes against harmless nginx/upstream fixtures."""
import json
import test_nginx


class ContactNginxTests(test_nginx.NginxAssemblyTests):
    CONTACT_ENABLED = True

    def test_private_contact_post_preserves_body_without_log_or_public_cors(self):
        payload = json.dumps({"capability": "ab" * 32, "reply_email": "private-sentinel@example.org",
                              "envelope": {"content": "private-ciphertext-sentinel"}}).encode()
        status, headers, _ = self.request("production", "/api/v1/contact", "POST", payload,
                                          {"Content-Type": "application/json", "Origin": "https://lightning-goats.com"}, source="127.0.0.40")
        self.assertEqual(status, 200)
        self.assertEqual(headers["Cache-Control"], "no-store")
        self.assertNotIn("Access-Control-Allow-Origin", headers)
        self.assertEqual(self.backends["production"].bodies[-1], payload)
        for file in self.root.rglob("*.log"):
            self.assertNotIn("private-sentinel", file.read_text(errors="replace"))
            self.assertNotIn("private-ciphertext-sentinel", file.read_text(errors="replace"))

    def test_contact_rejects_methods_types_url_capabilities_and_prefixes_before_backend(self):
        before = len(self.backends["production"].calls)
        for path in ("/api/v1/contact", "/api/v1/contact/status"):
            # Keep independent assertions from exhausting the real per-IP rate
            # bucket. The inherited burst test still verifies quota enforcement.
            for index, method in enumerate(("GET", "HEAD", "PUT", "DELETE", "OPTIONS")):
                self.assertEqual(self.request("production", path, method, source=f"127.0.0.{50+index}")[0], 403, (path, method))
            self.assertEqual(self.request("production", path, "POST", b"{}", {"Content-Type": "text/plain"})[0], 415)
            self.assertEqual(self.request("production", path + "?capability=private", "POST", b"{}", {"Content-Type": "application/json"})[0], 400)
            self.assertEqual(self.request("production", path + "/unexpected", "POST", b"{}", {"Content-Type": "application/json"})[0], 404)
        self.assertEqual(len(self.backends["production"].calls), before)

    def test_contact_and_status_body_limits_are_separate(self):
        for path, size in (("/api/v1/contact", 32769), ("/api/v1/contact/status", 1025)):
            before = len(self.backends["production"].calls)
            status, _, _ = self.request("production", path, "POST", b" " * size, {"Content-Type": "application/json"})
            self.assertEqual(status, 413)
            self.assertEqual(len(self.backends["production"].calls), before)
