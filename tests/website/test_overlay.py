"""Browser-source tests with synthetic WebSocket messages and no external traffic."""
import json
import mimetypes
from datetime import datetime, timezone
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
        self.display_payload = {"weather": None, "market": None}
        self.status_payload = {"mode":"shadow", "gateway_reachable":True, "remote_feeding_enabled":True, "feeder_override_active":False, "feed_credit_sats":999999}
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
        if url.path == "/overlay-data.json":
            route.fulfill(content_type="application/json", body=json.dumps(self.display_payload))
            return
        if url.path == "/api/v1/status":
            route.fulfill(content_type="application/json", body=json.dumps(self.status_payload))
            return
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


    def test_full_hd_transparent_layout_and_branded_canonical_qr(self):
        self.snapshot(credit=2)
        self.assertEqual(self.page.locator("main").evaluate("node => [node.offsetWidth,node.offsetHeight]"), [1920, 1080])
        self.assertEqual(self.page.locator("body").evaluate("node => getComputedStyle(node).backgroundColor"), "rgba(0, 0, 0, 0)")
        self.assertEqual(self.page.locator("#qr-code").evaluate("node => [node.offsetLeft,node.offsetTop,node.offsetWidth,node.offsetHeight]"), [1732, 3, 180, 180])
        self.assertEqual(self.page.locator("#bottom-container").evaluate("node => node.offsetTop+node.offsetHeight"), 1080)
        self.assertIn("998", self.page.locator("#sats_left").inner_text())
        self.assertEqual(self.page.locator("#qr-image svg").count(), 1)
        payload = self.page.locator("#qr-code").get_attribute("data-lnurl")
        self.assertTrue(payload.startswith("LNURL1"))
        self.assertEqual(self.page.locator("#qr-code").get_attribute("href"), "lightning:"+payload)
        chars = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
        words = [chars.index(char) for char in payload.lower().split("1", 1)[1]]
        checksum = 1
        for word in [3,3,3,3,3,0,12,14,21,18,12]+words:
            top = checksum >> 25
            checksum = ((checksum & 0x1ffffff) << 5) ^ word
            for bit, generator in enumerate([0x3b6a57b2,0x26508e6d,0x1ea119fa,0x3d4233dd,0x2a1462b3]):
                if top >> bit & 1:
                    checksum ^= generator
        self.assertEqual(checksum, 1)
        bits = "".join(f"{word:05b}" for word in words[:-6])
        decoded = bytes(int(bits[pos:pos+8],2) for pos in range(0,len(bits)-7,8)).decode()
        self.assertEqual(decoded, "https://lightning-goats.com/.well-known/lnurlp/herd")
        self.assertEqual(self.errors, [])

    def test_payment_reaching_threshold_never_fakes_a_feed(self):
        self.snapshot(credit=990)
        payment = {"type":"sats_received", "source_type":"payment_received", "seq":1,
                   "feed_credit_sats":1010, "amount_sats":20, "message":"Thanks, Dexter!",
                   "goats":[{"name":"Dexter", "imageUrl":"javascript:alert(1)"}]}
        self.send(payment)
        self.assertTrue(self.page.locator("#sats-received-overlay").is_visible())
        self.assertIn("20 sats",self.page.locator("#sats-received-overlay").inner_text())
        self.assertEqual(self.page.locator("#sats-received-overlay").get_attribute("data-tier"),"spark")
        self.assertFalse(self.page.locator("#feeder-triggered").is_visible())
        self.assertFalse(self.page.locator("main").evaluate("node=>node.classList.contains('fed')"))
        self.assertEqual(self.page.locator("#progress").get_attribute("aria-valuenow"),"100")
        self.assertTrue(self.page.locator("#lightning-img").is_visible())
        self.assertEqual(self.page.locator(".user-profile-name").inner_text(),"Dexter")
        self.send(payment)
        self.send({"type":"feeder_trigger","source_type":"feeder_confirmed","seq":2,"feed_credit_sats":10,"message":"Confirmed"})
        self.assertTrue(self.page.locator("#feeder-triggered").is_visible())
        self.assertFalse(self.page.locator("#sats-received-overlay").is_visible())
        self.page.clock.fast_forward(4000)
        self.assertEqual(self.page.locator("#credit").inner_text(),"10 sats credit")
        self.assertEqual(self.page.locator("#progress").get_attribute("aria-valuenow"),"1")
        self.assertFalse(self.page.locator("#feeder-triggered").is_visible())
        self.assertEqual(self.errors,[])

    def test_resume_suppresses_old_payment_notice_and_qr_swap(self):
        self.snapshot(seq=2,credit=2)
        self.page.evaluate("window.mockSocket.close()")
        self.page.wait_for_function("window.socketUrls.length === 2")
        self.send({"type":"resume","version":1,"stream_id":STREAM,"after":2,"through":3})
        self.send({"type":"sats_received","source_type":"payment_received","seq":3,"feed_credit_sats":502,"amount_sats":500,"message":"Historical payment"})
        self.snapshot(seq=3,credit=502)
        self.assertFalse(self.page.locator("#lightning-img").is_visible())
        self.assertFalse(self.page.locator("#sats-received-overlay").is_visible())
        self.assertEqual(self.page.locator("#message").inner_text(),"")
        self.assertEqual(self.page.locator("#credit").inner_text(),"502 sats credit")
        self.assertEqual(self.errors,[])

    def test_public_display_data_expires_and_status_cannot_overwrite_credit(self):
        self.snapshot(credit=2)
        observed = self.page.evaluate("new Date().toISOString()")
        self.display_payload = {"weather":{"observed_at":observed,"temperature_f":65.66},
            "market":{"observed_at":observed,"price_usd":75000,"change_24h_percent":-2}}
        self.page.clock.run_for(16000)
        self.assertEqual(self.page.locator("#current-temperature").inner_text(),"66 °F")
        self.assertEqual(self.page.locator("#btc-price").inner_text(),"BTC: $75,000")
        self.assertEqual(self.page.locator("#btc-change").inner_text(),"(24h: -2.00%)")
        self.assertIn("btc-change-negative",self.page.locator("#btc-change").get_attribute("class"))
        self.assertEqual(self.page.locator("#credit").inner_text(),"2 sats credit")
        self.page.clock.run_for(30000)
        self.assertIn("Paused", self.page.locator("#feeding-state").inner_text())
        self.page.clock.fast_forward(301000)
        self.assertEqual(self.page.locator("#current-temperature").inner_text(),"-- °F")
        self.assertEqual(self.page.locator("#btc-price").inner_text(),"BTC: --")
        self.assertEqual(self.errors,[])

    def test_future_roster_cards_are_bounded_sorted_safe_and_resettable(self):
        self.snapshot()
        members = [{"name":f"Member {i}","pubkey":str(i),"amount":i,"picture":"javascript:alert(1)"} for i in range(12)]
        members[-1]["name"] = "<img src=x onerror=alert(1)>"
        self.send({"type":"cyberherd_roster_updated","source_type":"cyberherd_roster_updated","seq":1,
            "members":members,"newest_pubkey":"11"})
        self.assertEqual(self.page.locator(".accordion-item").count(),10)
        self.assertEqual(self.page.locator(".user-profile-name").first.inner_text(),"<img src=x onerror=alert(1)>")
        self.assertEqual(self.page.locator(".accordion-item[data-newest=true]").count(),1)
        self.assertEqual(self.page.locator(".user-profile-name img").count(),0)
        self.assertEqual(urlsplit(self.page.locator(".accordion-content img").first.get_attribute("src")).path, "/images/lightninggoatslogo1.png")
        self.send({"type":"cyberherd_daily_reset","source_type":"cyberherd_daily_reset","seq":2})
        self.assertEqual(self.page.locator(".accordion-item").count(),0)
        self.assertEqual(self.errors,[])


    def test_actual_logo_branded_qr_decodes_at_obs_size(self):
        import io
        from PIL import Image
        import zxingcpp
        self.snapshot()
        self.page.wait_for_function("document.querySelector('#qr-brand img').naturalWidth > 0")
        image = Image.open(io.BytesIO(self.page.locator("#qr-code").screenshot()))
        decoded = zxingcpp.read_barcode(image)
        self.assertIsNotNone(decoded, "180px QR with the center logo must remain scannable")
        self.assertEqual(decoded.text, self.page.locator("#qr-code").get_attribute("data-lnurl"))

    def test_two_confirmations_preserve_absolute_remainder_during_visual_drain(self):
        self.snapshot(credit=2340)
        for sequence, credit in [(1,1340),(2,340)]:
            self.send({"type":"feeder_trigger","source_type":"feeder_confirmed","seq":sequence,
                       "feed_credit_sats":credit,"message":"Confirmed."})
        self.page.clock.run_for(3000)
        self.assertTrue(self.page.locator("#feeder-triggered").is_visible())
        self.assertEqual(self.page.locator("#credit").inner_text(),"340 sats credit")
        self.page.clock.run_for(3000)
        self.assertFalse(self.page.locator("#feeder-triggered").is_visible())
        self.assertEqual(self.page.locator("#credit").inner_text(),"340 sats credit")
        self.assertEqual(self.page.locator("#progress").get_attribute("aria-valuenow"),"34")
        self.assertEqual(self.errors,[])


    def test_live_connection_label_hidden_and_override_banner_follows_operator(self):
        self.snapshot(credit=3)
        self.assertEqual(self.page.locator("#connection").get_attribute("data-state"),"live")
        self.assertEqual(self.page.locator("#connection").inner_text(),"")
        self.status_payload.update(mode="canary", feeder_override_active=True)
        self.page.clock.run_for(11000)
        self.assertEqual(self.page.locator("#feeding-state-label").inner_text(),"⏸ Feeder Paused")
        self.assertEqual(self.page.locator("#feeding-state").get_attribute("data-state"),"paused")
        self.assertGreaterEqual(self.page.locator("#feeding-state").evaluate("node=>parseInt(getComputedStyle(node).fontSize)"),28)
        self.status_payload["feeder_override_active"] = False
        self.page.clock.run_for(11000)
        self.assertTrue(self.page.locator("#feeding-state").is_hidden())
        self.assertEqual(self.page.locator("#credit").inner_text(),"3 sats credit")
        self.assertEqual(self.errors,[])


    def test_small_credits_update_progress_but_suppress_all_payment_effects(self):
        self.snapshot(credit=3)
        self.send({"type":"sats_received","source_type":"payment_received","seq":1,
            "amount_sats":9,"feed_credit_sats":12,"message":"Too small to announce", "goats":[{"name":"Dexter"}]})
        self.assertEqual(self.page.locator("#credit").inner_text(),"12 sats credit")
        self.assertFalse(self.page.locator("#sats-received-overlay").is_visible())
        self.assertFalse(self.page.locator("#lightning-img").is_visible())
        self.assertEqual(self.page.locator("#message").inner_text(),"")
        self.assertEqual(self.page.locator(".accordion-item").count(),0)
        self.send({"type":"event_skipped","source_type":"operator_credit_added","seq":2,"feed_credit_sats":21})
        self.assertEqual(self.page.locator("#credit").inner_text(),"21 sats credit")
        self.send({"type":"sats_received","source_type":"operator_credit_added","seq":3,
            "amount_sats":10,"feed_credit_sats":31,"message":"10 sats received!","goats":[{"name":"Dexter"}]})
        self.assertTrue(self.page.locator("#sats-received-overlay").is_visible())
        self.assertEqual(self.page.locator("#sats-received-overlay").inner_text(),"⚡ 10 sats ⚡")
        self.assertTrue(self.page.locator("#lightning-img").is_visible())
        self.assertEqual(self.page.locator(".user-profile-name").inner_text(),"Dexter")
        self.assertEqual(self.errors,[])


    def test_paused_banner_counts_queued_feedings_without_debit_or_false_feed(self):
        self.snapshot(credit=2340)
        self.page.clock.run_for(11000)
        self.assertIn("2 feedings queued to run when feeding resumes", self.page.locator("#feeding-queue").inner_text())
        self.assertIn("2 feedings queued",self.page.locator("#sats_left").inner_text())
        self.assertEqual(self.page.locator("#credit").inner_text(),"2,340 sats credit")
        self.assertFalse(self.page.locator("#feeder-triggered").is_visible())
        self.status_payload.update(mode="canary",feeder_override_active=False)
        self.page.clock.run_for(11000)
        self.assertTrue(self.page.locator("#feeding-state").is_hidden())
        self.assertEqual(self.page.locator("#credit").inner_text(),"2,340 sats credit")
        self.send({"type":"feeder_trigger","source_type":"feeder_confirmed","seq":1,"feed_credit_sats":1340,"message":"Confirmed"})
        self.status_payload["feeder_override_active"]=True
        self.page.clock.run_for(11000)
        self.assertIn("1 feeding queued to run when feeding resumes",self.page.locator("#feeding-queue").inner_text())
        self.assertEqual(self.page.locator("#credit").inner_text(),"1,340 sats credit")
        self.assertEqual(self.errors,[])

    def test_pending_confirmation_is_not_promised_as_another_queued_feeding(self):
        self.snapshot(credit=2340,unresolved_feed_attempt=STREAM)
        self.status_payload.update(unresolved_feed_attempt=STREAM)
        self.page.clock.run_for(11000)
        text=self.page.locator("#feeding-queue").inner_text()
        self.assertIn("1 feeding queued",text)
        self.assertIn("earlier feeding is awaiting confirmation",text)
        self.assertEqual(self.page.locator("#credit").inner_text(),"2,340 sats credit")
        self.assertFalse(self.page.locator("#feeder-triggered").is_visible())
        self.assertEqual(self.errors,[])
