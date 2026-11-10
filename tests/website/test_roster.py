"""Read-only CyberHerd page. Every HTTP request is fulfilled locally or blocked."""
import json
import mimetypes
import os
from pathlib import Path
import time
import unittest
from urllib.parse import urlsplit
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2] / "web"
ORIGIN = "https://lightning-goats.com"
OWNER = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"

class RosterBrowserTests(unittest.TestCase):
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
        self.page.on("pageerror",lambda error:self.errors.append(str(error)))
        self.enabled = False
        self.requests = []
        self.data = {"version":1,"generated_at":int(time.time()),"availability":"paused","targets":[{"event_id":"ab"*32,"kind":1}],
                     "roster":{"owner":OWNER,"cycle":1,"cycle_start":100,"revision":1,"capacity":3,"minimum_sats":"10","member_percent":10,"total_members":1,"next_offset":None,
                               "members":[{"rank":1,"pubkey":"cd"*32,"display_name":"Alice <img src=x>","picture":"javascript:alert(1)","amount_sats":"9007199254740993","points":"9007199254740993","share_percent":"10"}]}}
        self.page.route("**/*",self.route)
    def route(self,route):
        url = urlsplit(route.request.url)
        self.requests.append((route.request.method,url.path))
        if url.path == "/site-config.js":
            route.fulfill(content_type="text/javascript",body="window.LightningGoatsSite = " + json.dumps({"paymentsEnabled":False,"cyberherdEnabled":self.enabled}))
        elif url.path == "/api/v1/cyberherd":
            route.fulfill(content_type="application/json",body=json.dumps(self.data))
        else:
            file = ROOT / url.path.lstrip("/")
            if url.netloc == "lightning-goats.com" and file.is_file() and file.resolve().is_relative_to(ROOT):
                route.fulfill(content_type=mimetypes.guess_type(file)[0] or "text/plain",body=file.read_bytes())
            else:
                route.abort()
    def test_default_does_not_contact_membership_or_wallets(self):
        self.page.goto(ORIGIN + "/cyberherd.html")
        self.assertIn("not available yet",self.page.locator("#status").inner_text())
        self.assertFalse(any(path.startswith("/api/") for _,path in self.requests))
        self.assertEqual(self.page.locator("input").count(),0)
        self.assertEqual(self.errors,[])
    def test_enabled_view_is_exact_safe_nonmutating_and_clears_when_empty(self):
        self.enabled=True
        self.page.goto(ORIGIN + "/cyberherd.html")
        self.page.wait_for_selector("#herd",state="visible")
        self.assertIn("Alice <img src=x>",self.page.locator("#members").inner_text())
        self.assertEqual(self.page.locator("#members span img").count(),0)
        self.assertIn("9,007,199,254,740,993",self.page.locator("#members").inner_text())
        self.assertIn("paused",self.page.locator("#status").inner_text())
        self.assertTrue(self.page.locator("#targets a").get_attribute("href").startswith("nostr:nevent1"))
        self.assertTrue(all(method=="GET" for method,_ in self.requests))
        self.assertFalse(any("lnurlp" in path or "webhook" in path for _,path in self.requests))
        self.data["roster"].update({"total_members":0,"members":[],"revision":2})
        self.page.reload()
        self.page.wait_for_selector("#herd",state="visible")
        self.assertTrue(self.page.locator("#empty").is_visible())
        self.assertEqual(self.page.locator("#members tr").count(),0)
        self.assertEqual(self.errors,[])
