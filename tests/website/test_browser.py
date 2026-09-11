"""Offline browser contracts. All HTTP/WebSocket traffic is intercepted.

Run: python3 -m unittest discover -s tests/website -v
Requires playwright==1.62.0 and its Chromium, or LG_CHROMIUM=/path/to/chromium.
These are UI mocks, not live wallet, relay, video or daemon acceptance.
"""
import json
import mimetypes
import os
from pathlib import Path
import unittest
from urllib.parse import urlsplit

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2] / "web"
ORIGIN = "https://feeder.lightning-goats.com"
INVOICE = "lnbc10u1" + "q" * 120  # Deliberately NOT a valid payment invoice.
NOSTR = """
window.mockSubscriptions = [];
window.NostrTools = {
  SimplePool: class {
    subscribe(relays, filter, callbacks) {
      window.mockSubscriptions.push({filter, callbacks});
      return { close() {} };
    }
    close() {}
    querySync() { return Promise.resolve([]); }
  },
  nip19: { npubEncode(key) { return 'npub-test-' + key; } },
  verifyEvent(event) { return event.mockVerified === true; }
};
"""


class WebsiteBrowserTests(unittest.TestCase):
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
        self.context = self.browser.new_context(service_workers="block")
        self.addCleanup(self.context.close)
        self.page = self.context.new_page()
        self.errors = []
        self.requests = []
        self.payments = []
        self.enabled = False
        self.mode = "success"
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        self.context.route("**/*", self.route)
        self.context.route_web_socket("**/*", lambda socket: socket.close())

    def route(self, route):
        url = route.request.url
        self.requests.append(url)
        parsed = urlsplit(url)
        if url.startswith(ORIGIN + "/"):
            path = parsed.path
            if path.startswith("/.well-known/lnurlp/"):
                self.payments.append(url)
                user = path.rsplit("/", 1)[1]
                callback = f"{ORIGIN}/lnurlp/{user}/callback"
                if self.mode == "cross_origin":
                    callback = "https://attacker.invalid/lnurlp/herd/callback"
                if self.mode == "credentials":
                    callback = callback.replace("https://", "https://user:pass@")
                if self.mode == "redirect":
                    route.fulfill(status=302, headers={"Location": "https://attacker.invalid/"})
                    return
                payload = {"tag": "payRequest", "minSendable": 1000,
                           "maxSendable": 1000000000, "callback": callback}
                body = json.dumps(payload)
                if self.mode == "oversize":
                    body = " " * 32769 + body
                route.fulfill(content_type="application/json", body=body)
                return
            if path.startswith("/lnurlp/"):
                self.payments.append(url)
                if self.mode == "refused":
                    route.fulfill(status=503, body="provider error")
                else:
                    route.fulfill(content_type="application/json", body=json.dumps({"pr": INVOICE, "routes": []}))
                return
            if path == "/site-config.js" and self.enabled:
                route.fulfill(content_type="text/javascript", body="window.LightningGoatsSite = {paymentsEnabled: true};")
                return
            file = ROOT / ("index.html" if path == "/" else path.lstrip("/"))
            if file.is_file() and file.resolve().is_relative_to(ROOT):
                route.fulfill(content_type=mimetypes.guess_type(file)[0] or "application/octet-stream", body=file.read_bytes())
                return
        if "nostr-tools@" in url:
            route.fulfill(content_type="text/javascript", body=NOSTR)
        elif route.request.resource_type == "script":
            route.fulfill(content_type="text/javascript", body="")
        else:
            route.fulfill(content_type="text/html", body="")

    def open(self, enabled=False):
        self.enabled = enabled
        self.page.close()
        self.page = self.context.new_page()
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        self.page.goto(ORIGIN, wait_until="domcontentloaded", timeout=120000)

    def submit(self):
        self.page.evaluate("document.getElementById('feedForm').dispatchEvent(new Event('submit', {cancelable: true}))")

    def wait_finished(self):
        self.page.wait_for_function("!document.getElementById('createInvoice').disabled")
        self.assertEqual(self.errors, [])

    def test_default_has_no_payment_or_legacy_requests(self):
        self.open()
        self.submit()  # A synthetic event cannot bypass the default HOLD.
        self.assertTrue(self.page.locator("#feedLink").is_disabled())
        self.assertEqual(self.payments, [])
        self.assertEqual(self.errors, [])
        self.assertEqual(self.page.locator("iframe").count(), 1)
        self.assertIn("youtube.com/embed", self.page.locator("iframe").get_attribute("src"))
        for url in self.requests:
            self.assertFalse(any(value in url.lower() for value in ["lnbits", "nip05", "cyberherd", "leaderboard"]))
        self.assertEqual(self.page.locator("#contactNpub").input_value(),
                         "npub1v60thnx0gz0wq3n6xdnq46y069l9x70xgmjp6lprdl6fv0eux6mqgjj4rp")
        self.assertEqual(self.page.locator("a[href^='mailto:']").count(), 0)

    def test_feed_overlay_covers_qr_above_player_on_desktop_and_mobile(self):
        self.open()
        # A player layer must not cover the feed target. The iframe remains a
        # harmless intercepted document; no video, payment or relay is contacted.
        self.page.locator("iframe").evaluate("frame => frame.style.zIndex = '9999'")
        for width in (1440, 390):
            self.page.set_viewport_size({"width": width, "height": 900})
            result = self.page.evaluate("""() => {
              const button = document.getElementById('feedLink');
              const video = document.querySelector('.iframe-container').getBoundingClientRect();
              const rect = button.getBoundingClientRect();
              const x = video.right - 50, y = video.top + 50;
              return {above: button.contains(document.elementFromPoint(x, y)),
                contained: rect.left >= video.left && rect.right <= video.right &&
                  rect.top >= video.top && rect.bottom <= video.bottom,
                opacity: getComputedStyle(button).opacity, disabled: button.disabled};
            }""")
            self.assertTrue(result["above"], (width, result))
            self.assertTrue(result["contained"], (width, result))
            self.assertEqual(result["opacity"], "1")
            self.assertTrue(result["disabled"])
        self.assertEqual(self.payments, [])

    def test_all_six_addresses_use_native_routes_without_automatic_payment(self):
        for user in ["herd", "dexter", "rowan", "cosmo", "newton", "nova"]:
            with self.subTest(user=user):
                self.payments.clear()
                self.open(enabled=True)
                self.page.select_option("#feedAddress", user, force=True)
                self.page.evaluate("""() => {
                  const form = document.getElementById("feedForm");
                  form.dispatchEvent(new Event("submit", {cancelable: true}));
                  form.dispatchEvent(new Event("submit", {cancelable: true}));
                }""")  # Same-turn double submission must make one invoice.
                self.wait_finished()
                self.assertEqual(self.payments, [f"{ORIGIN}/.well-known/lnurlp/{user}",
                                                f"{ORIGIN}/lnurlp/{user}/callback?amount=1000000"])
                self.assertEqual(self.page.locator("#invoiceText").input_value(), INVOICE)
                self.assertEqual(self.page.locator("#invoiceWallet").get_attribute("href"), "lightning:" + INVOICE)
                self.assertEqual(self.page.url, ORIGIN + "/")
                self.page.eval_on_selector("#feedAmount", "e => { e.value = '2000'; e.dispatchEvent(new Event('input')); }")
                self.assertEqual(self.page.locator("#invoiceText").input_value(), "")
                self.assertIsNone(self.page.locator("#invoiceWallet").get_attribute("href"))

    def test_unsafe_discovery_is_rejected_before_invoice_creation(self):
        for mode in ["cross_origin", "credentials", "oversize", "redirect"]:
            with self.subTest(mode=mode):
                self.mode = mode
                self.payments.clear()
                self.open(enabled=True)
                self.submit()
                self.wait_finished()
                self.assertEqual(len(self.payments), 1)
                self.assertEqual(self.page.locator("#invoiceText").input_value(), "")
                self.assertIn("No automatic retry", self.page.locator("#paymentStatus").inner_text())

    def test_refusal_does_not_retry_or_claim_payment(self):
        self.mode = "refused"
        self.open(enabled=True)
        self.submit()
        self.wait_finished()
        self.assertEqual(len(self.payments), 2)
        self.assertEqual(self.page.locator("#invoiceText").input_value(), "")
        self.assertIn("No automatic retry", self.page.locator("#paymentStatus").inner_text())

    def test_live_chat_renders_untrusted_text_without_html(self):
        self.open()
        self.page.evaluate("""() => {
          const pubkey = '669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6';
          const now = Math.floor(Date.now()/1000);
          mockSubscriptions[0].callbacks.onevent({mockVerified: true, kind: 30311,
            pubkey, created_at: now, tags: [['d','fixture'],['status','live']], content: ''});
          mockSubscriptions.find(s => s.filter.kinds[0] === 1311).callbacks.onevent({
            mockVerified: true, kind: 1311, id: 'test', pubkey, created_at: now,
            tags: [['a', `30311:${pubkey}:fixture`]], content: '<img src=x onerror=alert(1)>'});
        }""")
        self.assertEqual(self.page.locator(".chat-content").inner_text(), "<img src=x onerror=alert(1)>")
        self.assertEqual(self.page.locator(".chat-content img").count(), 0)
        self.assertEqual(self.errors, [])


if __name__ == "__main__":
    unittest.main()
