"""Actual index UI with isolated wallets/relays; no payment or publication.

Zap fixtures use the shipped native Schnorr implementation for both signatures.
Their BOLT11 strings exercise the receipt display parser, not invoice settlement.
"""
import json
import mimetypes
import os
from pathlib import Path
import unittest
from urllib.parse import urlsplit

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2] / "web"
ORIGIN = "https://lightning-goats.com"
LG = "669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6"
INVOICE = "lnbc10u1" + "q" * 120
POOL = """
window.mockSubscriptions = [];
class FixturePool {
  subscribe(relays, filter, callbacks) {
    const item = {relays, filter, callbacks, closed:false};
    mockSubscriptions.push(item);
    return {close() {item.closed = true;}};
  }
  querySync(relays, filter) {return Promise.resolve((window.fixtureProfiles || []).filter(e=>filter.authors?.includes(e.pubkey)));}
  get(relays, filter) {return Promise.resolve(window.fixtureEvents?.find(e=>filter.ids?.includes(e.id)) || null);}
  publish() {window.publications = (window.publications || 0)+1; return [Promise.resolve()];}
  close() {}
}
"""
MOCK = POOL + """
window.NostrTools = {SimplePool:FixturePool, verifyEvent:e=>e.mockVerified === true,
  nip19:{npubEncode:key=>'npub-test-'+key}, nip57:{}};
"""
NATIVE = (ROOT / "vendor/nostr.bundle.js").read_text() + POOL + """
window.NativeNostrTools = NostrTools;
window.fixtureIssuerKey = NostrTools.generateSecretKey();
window.fixtureIssuer = NostrTools.getPublicKey(fixtureIssuerKey);
window.fixtureSenderKey = NostrTools.generateSecretKey();
window.fixtureSender = NostrTools.getPublicKey(fixtureSenderKey);
window.NostrTools = {...NostrTools, SimplePool:FixturePool, verifyEvent:e=>
  ([30311,9041].includes(e.kind) && e.mockVerified === true) || NativeNostrTools.verifyEvent(e)};
window.fixtureProfiles = [NostrTools.finalizeEvent({kind:0, created_at:Math.floor(Date.now()/1000),
  tags:[], content:JSON.stringify({display_name:'Sat',picture:'javascript:alert(1)'})},fixtureSenderKey)];
window.makeReceipt = (changes={}) => {
  const now = Math.floor(Date.now()/1000);
  const tags = [['p','669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6'],
    ['amount','21000'],['relays','wss://relay.damus.io'],['e','e'.repeat(64)]];
  const template = {kind:9734,created_at:now,content:'<img src=x onerror=alert(1)>',tags,...changes.request};
  const request = NostrTools.finalizeEvent(template, fixtureSenderKey);
  const receipt = {kind:9735,created_at:now,content:'',tags:[
    ['p',tags[0][1]],['P',fixtureSender],['e','e'.repeat(64)],
    ['description',JSON.stringify(request)],['bolt11','lnbc210n1'+'q'.repeat(120)]],...changes.receipt};
  return NostrTools.finalizeEvent(receipt, changes.wrongIssuer ? fixtureSenderKey : fixtureIssuerKey);
};
"""


class IndexBrowserTests(unittest.TestCase):
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
        self.context.route("**/*", self.route)
        self.context.route_web_socket("**/*", lambda socket: socket.close())
        self.page = self.context.new_page()
        self.errors = []
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        self.invoices = []
        self.native = False

    def route(self, route):
        url = urlsplit(route.request.url)
        if url.netloc != "lightning-goats.com":
            route.fulfill(body="")
            return
        path = url.path
        if path == "/site-config.js":
            route.fulfill(content_type="text/javascript", body="window.LightningGoatsSite={paymentsEnabled:true,cyberherdEnabled:true}")
        elif path == "/vendor/nostr.bundle.js":
            route.fulfill(content_type="text/javascript", body=NATIVE if self.native else MOCK)
        elif path == "/vendor/nostr-login.js":
            route.fulfill(content_type="text/javascript", body="")
        elif path == "/.well-known/lnurlp/herd":
            issuer = self.page.evaluate("window.fixtureIssuer || 'a'.repeat(64)")
            route.fulfill(content_type="application/json", body=json.dumps({
                "tag": "payRequest", "callback": ORIGIN + "/lnurlp/herd/callback",
                "minSendable": 1000, "maxSendable": 1000000000,
                "allowsNostr": True, "nostrPubkey": issuer}))
        elif path == "/lnurlp/herd/callback":
            self.invoices.append(route.request.url)
            route.fulfill(content_type="application/json", body=json.dumps({"pr": INVOICE}))
        elif path == "/api/v1/cyberherd":
            route.fulfill(status=503, content_type="application/json", body="{}")
        else:
            file = ROOT / ("index.html" if path == "/" else path.lstrip("/"))
            if file.is_file() and file.resolve().is_relative_to(ROOT):
                route.fulfill(content_type=mimetypes.guess_type(file)[0] or "text/plain", body=file.read_bytes())
            else:
                route.fulfill(body="")

    def open(self):
        self.page.goto(ORIGIN + "/", wait_until="domcontentloaded", timeout=120000)

    def pay(self):
        self.page.locator("#zapCreateButton").click()
        self.page.wait_for_function("!document.getElementById('zapCreateButton').disabled")

    def test_invisible_qr_opens_one_action_webln_and_reuses_uncertain_invoice(self):
        self.context.add_init_script("""
          window.walletCalls=[]; window.failPayment=true;
          window.alby={webln:{async enable(){walletCalls.push('enable');},async sendPayment(invoice){
            walletCalls.push(invoice); if(failPayment) throw Error('cancelled'); return {preimage:'fixture'};
          }}};
        """)
        self.open()
        self.page.locator(".qr-code-overlay").click()
        self.assertTrue(self.page.locator("#zapModal").is_visible())
        self.assertEqual(self.page.locator("#zapCreateButton").inner_text(), "Pay")
        self.assertEqual(self.invoices, [])
        self.pay()
        self.assertEqual(len(self.invoices), 1)
        self.assertEqual(self.page.evaluate("walletCalls"), ["enable", INVOICE])
        self.assertIn("cancelled", self.page.locator("#zapStatus").inner_text())
        self.assertEqual(self.page.locator("#zapInvoice").input_value(), INVOICE)
        self.page.evaluate("window.failPayment=false")
        self.pay()
        self.assertEqual(len(self.invoices), 1)
        self.assertEqual(self.page.evaluate("walletCalls"), ["enable", INVOICE, "enable", INVOICE])
        self.assertIn("Waiting for confirmation", self.page.locator("#zapStatus").inner_text())
        self.assertEqual(self.page.evaluate("window.publications || 0"), 0)
        self.assertEqual(self.errors, [])

    def test_permission_refusal_has_no_invoice_then_external_wallet_fallback(self):
        self.context.add_init_script("window.webln={async enable(){throw Error('refused')},async sendPayment(){throw Error('must not run')}}")
        self.open()
        self.page.locator(".qr-code-overlay").click()
        self.pay()
        self.assertEqual(self.invoices, [])
        self.page.evaluate("delete window.webln")
        self.pay()
        self.assertEqual(len(self.invoices), 1)
        self.assertEqual(self.page.locator("#zapWalletButton").get_attribute("href"), "lightning:" + INVOICE)
        self.assertTrue(self.page.locator("#zapQr").is_visible())
        self.assertEqual(self.errors, [])

    def test_real_signed_herd_zaps_are_safe_deduplicated_and_forgery_refused(self):
        self.native = True
        self.open()
        self.page.evaluate("""lg=>{
          const callback=mockSubscriptions.find(s=>s.filter.kinds[0]===30311).callbacks.onevent;
          // A foreign author merely tagging the project cannot select the room.
          callback({mockVerified:true,kind:30311,id:'b'.repeat(64),pubkey:'a'.repeat(64),
            created_at:Math.floor(Date.now()/1000),tags:[['d','fake'],['status','live'],['p',lg,'','host']],content:''});
        }""", LG)
        self.assertEqual(self.page.locator("#chatStatusDot.live").count(), 0)
        self.page.evaluate("""lg=>{
          mockSubscriptions.find(s=>s.filter.kinds[0]===30311).callbacks.onevent({mockVerified:true,
            kind:30311,id:'c'.repeat(64),pubkey:lg,created_at:Math.floor(Date.now()/1000),
            tags:[['d','fixture'],['status','live']],content:''});
        }""", LG)
        self.page.wait_for_function("mockSubscriptions.some(s=>s.filter.kinds[0]===9735 && s.filter['#p'])")
        self.page.evaluate("""() => {
          window.deliver = event=>mockSubscriptions.find(s=>s.filter.kinds[0]===9735 && s.filter['#p']).callbacks.onevent(event);
          window.goodReceipt=makeReceipt(); deliver(goodReceipt); deliver(goodReceipt);
          const {id,sig,...replay}=goodReceipt;
          deliver(NativeNostrTools.finalizeEvent({...replay,created_at:replay.created_at+1},fixtureIssuerKey));
          deliver(makeReceipt({wrongIssuer:true}));
          deliver(makeReceipt({request:{kind:1}}));
          deliver(makeReceipt({request:{tags:[['p','a'.repeat(64)],['amount','21000'],['relays','wss://relay.damus.io']]}}));
          deliver(makeReceipt({request:{tags:[['p',goodReceipt.tags[0][1]],['amount','22000'],['relays','wss://relay.damus.io']]}}));
          deliver(makeReceipt({receipt:{created_at:Math.floor(Date.now()/1000)+301}}));
          deliver(makeReceipt({receipt:{tags:goodReceipt.tags.concat([['p',goodReceipt.tags[0][1]]])}}));
          const changed = JSON.parse(JSON.stringify(goodReceipt)); changed.content='forged'; deliver(changed);
        }""")
        self.page.wait_for_function("document.querySelector('.zap-line .chat-author')?.textContent==='Sat'")
        self.assertEqual(self.page.locator(".zap-message").count(), 1)
        self.assertEqual(self.page.locator(".zap-action").inner_text(), "has zapped")
        self.assertEqual(self.page.locator(".zap-amount").inner_text(), "21 sats.")
        self.assertIn("<img src=x", self.page.locator(".zap-comment").inner_text())
        self.assertEqual(self.page.locator(".zap-comment img").count(), 0)
        self.assertEqual(self.page.locator(".chat-avatar img").count(), 0)
        self.assertEqual(self.invoices, [])
        self.assertEqual(self.page.evaluate("window.publications || 0"), 0)
        self.assertEqual(self.errors, [])


if __name__ == "__main__":
    unittest.main()
