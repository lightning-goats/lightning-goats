"""Exercise webhook administration using fake HTTP; no keys or live requests."""
from contextlib import ExitStack, redirect_stdout
import hashlib
import hmac
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch
import urllib.error

SPEC = importlib.util.spec_from_file_location("strike_setup", Path(__file__).parents[1] / "scripts/setup-strike-webhook.py")
SETUP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SETUP)
ID = "6932966a-9d10-410d-a156-5b40bbdbe2f4"
SECRET = "a" * 40


def subscription(enabled=False):
    return {"id": ID, "webhookUrl": SETUP.URL, "webhookVersion": "v1", "eventTypes": [SETUP.EVENT], "enabled": enabled}


class Response(io.BytesIO):
    def __init__(self, body, status=200):
        super().__init__(json.dumps(body).encode())
        self.status = status


class FakeHTTP:
    def __init__(self, existing=None, denied=False, wrong_secret=False):
        self.existing = existing or []
        self.denied = denied
        self.wrong_secret = wrong_secret
        self.calls = []

    def open(self, req, timeout):
        method = req.get_method()
        body = json.loads(req.data) if req.data else None
        self.calls.append((method, req.full_url, body))
        if req.full_url == SETUP.URL:
            signature = req.get_header("X-webhook-signature")
            expected = hmac.new(SECRET.encode(), req.data, hashlib.sha256).hexdigest()
            code = 400 if signature == expected and not self.wrong_secret else 401
            raise urllib.error.HTTPError(req.full_url, code, "fixture", {}, io.BytesIO())
        if self.denied:
            raise urllib.error.HTTPError(req.full_url, 403, "fixture", {}, io.BytesIO())
        if method == "GET":
            return Response(self.existing)
        if method in ("POST", "PATCH"):
            item = subscription(body["enabled"])
            return Response(item, 201 if method == "POST" else 200)
        raise AssertionError("unexpected request")


class WebhookSetupTests(unittest.TestCase):
    def run_setup(self, http, arguments):
        output = io.StringIO()
        with ExitStack() as stack:
            stack.enter_context(patch("sys.argv", ["setup-strike-webhook.py", *arguments]))
            stack.enter_context(patch.object(SETUP.os, "geteuid", return_value=0))
            stack.enter_context(patch.object(SETUP, "secret", return_value=SECRET))
            key = stack.enter_context(patch.object(SETUP, "setup_key", return_value="synthetic-setup-key"))
            stack.enter_context(patch.object(SETUP.urllib.request, "build_opener", return_value=http))
            stack.enter_context(patch.object(SETUP.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)))
            stack.enter_context(patch.object(Path, "exists", return_value=False))
            stack.enter_context(redirect_stdout(output))
            SETUP.main()
        self.assertNotIn(SECRET, output.getvalue())
        self.assertNotIn("synthetic-setup-key", output.getvalue())
        return output.getvalue(), key

    def test_default_only_prints_plan(self):
        http = FakeHTTP()
        _, key = self.run_setup(http, [])
        self.assertEqual(http.calls, [])
        key.assert_not_called()

    def test_new_subscription_is_disabled_and_uses_saved_secret(self):
        http = FakeHTTP()
        output, _ = self.run_setup(http, ["--apply"])
        self.assertEqual([call[0] for call in http.calls], ["GET", "POST"])
        payload = http.calls[-1][2]
        self.assertEqual(payload, {"webhookUrl": SETUP.URL, "webhookVersion": "v1", "eventTypes": [SETUP.EVENT], "secret": SECRET, "enabled": False})
        self.assertIn("Prepared disabled", output)

    def test_existing_disabled_subscription_is_preserved(self):
        http = FakeHTTP([subscription()])
        self.run_setup(http, ["--apply"])
        self.assertEqual([call[0] for call in http.calls], ["GET"])

    def test_existing_enabled_state_is_reported_correctly(self):
        http = FakeHTTP([subscription(True)])
        output, _ = self.run_setup(http, ["--apply"])
        self.assertIn("Subscription is enabled", output)
        self.assertNotIn("Prepared disabled", output)
        self.assertEqual([call[0] for call in http.calls], ["GET"])

    def test_enable_checks_signature_and_binds_saved_secret(self):
        http = FakeHTTP([subscription()])
        self.run_setup(http, ["--apply", "--enable"])
        self.assertEqual([call[0] for call in http.calls], ["POST", "POST", "GET", "PATCH"])
        self.assertTrue(http.calls[-1][2]["enabled"])
        self.assertEqual(http.calls[-1][2]["secret"], SECRET)
        self.assertTrue(http.calls[-1][1].endswith("subscriptions/" + ID))

    def test_wrong_receiver_secret_prevents_registration(self):
        http = FakeHTTP(wrong_secret=True)
        with self.assertRaisesRegex(ValueError, "signature preflight"):
            self.run_setup(http, ["--apply", "--enable"])
        self.assertTrue(all(call[1] == SETUP.URL for call in http.calls))

    def test_missing_manage_scope_stops_before_create(self):
        http = FakeHTTP(denied=True)
        with self.assertRaisesRegex(ValueError, "partner.webhooks.manage"):
            self.run_setup(http, ["--apply"])
        self.assertEqual([call[0] for call in http.calls], ["GET"])

    def test_conflicts_and_duplicates_are_not_changed(self):
        wrong = subscription()
        wrong["webhookVersion"] = "v2"
        for existing in ([wrong], [subscription(), subscription()]):
            with self.subTest(existing=existing):
                http = FakeHTTP(existing)
                with self.assertRaises(ValueError):
                    self.run_setup(http, ["--apply"])
                self.assertEqual([call[0] for call in http.calls], ["GET"])

    def test_key_file_argument_reaches_key_loader(self):
        _, key = self.run_setup(FakeHTTP(), ["--apply", "--key-file", "/root/setup-key.txt"])
        key.assert_called_once_with(Path("/root/setup-key.txt"))

    def test_redirects_are_rejected(self):
        self.assertIsNone(SETUP.NoRedirect().redirect_request(None, None, 302, "", {}, "https://example.invalid/"))
