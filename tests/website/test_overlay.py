"""Browser-source tests with synthetic WebSocket messages and no external traffic."""
import json
import mimetypes
import os
from pathlib import Path
import unittest
from urllib.parse import urlsplit

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2] / "web"
STREAM = "76a209bd-58fb-431b-a76d-3c63cdc9bb1a"


class OverlayBrowserTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.playwright = sync_playwright().start()
        options = {"headless": True, "chromium_sandbox": True}
        if os.environ.get("LG_CHROMIUM"):
            options["executable_path"] = os.environ["LG_CHROMIUM"]
        cls.browser = cls.playwright.chromium.launch(**options)

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()

    def setUp(self):
        context = self.browser.new_context(service_workers="block")
        self.addCleanup(context.close)
        self.page = context.new_page()
        self.errors = []
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        context.route("**/*", self.route)
        context.add_init_script("""
            window.socketUrls = [];
            window.WebSocket = class {
                constructor(url) { window.socketUrls.push(url); window.mockSocket = this; }
                close() { this.onclose?.(); }
            };
        """)
        self.page.clock.install()
        self.page.goto("https://lightning-goats.com/overlay.html")
        self.page.wait_for_function("Boolean(window.mockSocket && window.mockSocket.onmessage)")

    def route(self, route):
        url = urlsplit(route.request.url)
        file = ROOT / url.path.lstrip("/")
        if url.netloc == "lightning-goats.com" and file.is_file() and file.resolve().is_relative_to(ROOT):
            route.fulfill(content_type=mimetypes.guess_type(file)[0] or "text/plain", body=file.read_bytes())
        else:
            route.abort()

    def send(self, payload):
        self.page.evaluate("data => window.mockSocket.onmessage({data: JSON.stringify(data)})", payload)

    def snapshot(self, seq=0, credit=0, **extra):
        self.send({"type": "snapshot", "version": 1, "stream_id": STREAM,
                   "seq": seq, "feed_credit_sats": credit, "threshold_sats": 1000, **extra})

    def test_credit_confirmation_and_untrusted_text(self):
        self.snapshot(credit=2340)
        self.assertEqual(self.page.locator("#credit").inner_text(), "2,340 sats credit")
        self.assertEqual(self.page.locator("#due").inner_text(), "2 feeds queued")
        self.assertFalse(self.page.locator("main").evaluate("el => el.classList.contains('fed')"))
        self.send({"type": "feeder_trigger", "source_type": "feeder_confirmed", "seq": 1,
                   "feed_credit_sats": 1340, "message": "<img src=x onerror=alert(1)>"})
        self.assertEqual(self.page.locator("#credit").inner_text(), "1,340 sats credit")
        self.assertEqual(self.page.locator("#message img").count(), 0)
        self.assertTrue(self.page.locator("main").evaluate("el => el.classList.contains('fed')"))
        self.send({"type": "feeder_trigger", "source_type": "feeder_confirmed", "seq": 2,
                   "feed_credit_sats": 340, "message": "Confirmed."})
        self.assertEqual(self.page.locator("#progress").get_attribute("aria-valuenow"), "34")
        self.send({"type": "sats_received", "seq": 1, "feed_credit_sats": 9999})
        self.assertEqual(self.page.locator("#credit").inner_text(), "340 sats credit")
        self.assertEqual(self.errors, [])

    def test_resume_suppresses_historical_feed_and_gap_resets(self):
        self.snapshot(seq=4, credit=1340)
        self.page.evaluate("window.mockSocket.close()")
        self.page.wait_for_function("window.socketUrls.length === 2")
        url = self.page.evaluate("window.socketUrls[1]")
        self.assertIn("version=1", url)
        self.assertIn("after=4", url)
        self.send({"type": "resume", "version": 1, "stream_id": STREAM, "after": 4, "through": 5})
        self.send({"type": "feeder_trigger", "source_type": "feeder_confirmed", "seq": 5,
                   "feed_credit_sats": 340, "message": "Historical feed"})
        self.snapshot(seq=5, credit=340)
        self.assertFalse(self.page.locator("main").evaluate("el => el.classList.contains('fed')"))
        self.assertEqual(self.page.locator("#message").inner_text(), "")
        self.send({"type": "sats_received", "seq": 7, "feed_credit_sats": 500})
        self.page.wait_for_function("window.socketUrls.length === 3")
        self.assertNotIn("?", self.page.evaluate("window.socketUrls[2]"))
        self.snapshot(seq=7, credit=500, reset_reason="resume_unavailable")
        self.assertEqual(self.page.locator("#credit").inner_text(), "500 sats credit")
        self.assertEqual(self.errors, [])

    def test_stale_weather_is_not_shown(self):
        self.snapshot()
        self.send({"type": "weather_status", "source_type": "weather_status", "seq": 1,
                   "message": "Old weather", "data": {"observed_at": "2020-01-01T00:00:00Z"}})
        self.assertEqual(self.page.locator("#weather").inner_text(), "")
        now = self.page.evaluate("new Date().toISOString()")
        self.send({"type": "weather_status", "source_type": "weather_status", "seq": 2,
                   "message": "Current weather", "data": {"observed_at": now}})
        self.assertEqual(self.page.locator("#weather").inner_text(), "Current weather")
        self.page.clock.fast_forward(301000)
        self.assertEqual(self.page.locator("#weather").inner_text(), "")
        self.assertEqual(self.errors, [])
