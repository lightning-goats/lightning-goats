"""Private contact crypto/UI against synthetic keys and isolated inbox relays."""
import json
import mimetypes
import os
from pathlib import Path
import unittest
from urllib.parse import urlsplit

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2] / "web"
ORIGIN = "https://lightning-goats.com"
PROJECT = "669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6"
FIXTURE = """
window.NativeNostrTools=NostrTools;
window.recipientKey=window.fixtureReuseRecipient?new Uint8Array(fixtureReuseRecipient):NostrTools.generateSecretKey();
window.recipient=NostrTools.getPublicKey(recipientKey);
window.senderKey=window.fixtureReuseSender?new Uint8Array(fixtureReuseSender):NostrTools.generateSecretKey();
window.sender=NostrTools.getPublicKey(senderKey);
window.published=[]; window.failRelays=true; window.signCalls=[];
const inbox=NostrTools.finalizeEvent({kind:10050,created_at:Math.floor(Date.now()/1000),
  tags:[['relay','wss://nos.lol'],['relay','wss://relay.damus.io']],content:''},recipientKey);
window.NostrTools={...NostrTools,SimplePool:class{
  subscribe(){return{close(){}};}
  get(){return Promise.resolve(null);}
  querySync(relays,filter){return Promise.resolve(filter.kinds[0]===10050?[inbox]:[]);}
  publish(relays,event){published.push({relays,event});return relays.map(()=>failRelays?Promise.reject(Error('lost')):Promise.resolve());}
  close(){}
}};
window.nostr={
  async getPublicKey(){return sender;},
  nip44:{async encrypt(pubkey,plaintext){return NativeNostrTools.nip44.encrypt(plaintext,NativeNostrTools.nip44.getConversationKey(senderKey,pubkey));}},
  async signEvent(template){signCalls.push(template.kind);return NativeNostrTools.finalizeEvent(template,senderKey);}
};
"""


class ContactBrowserTests(unittest.TestCase):
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
        self.provider_requests = []
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))

    def route(self, route):
        parsed = urlsplit(route.request.url)
        if parsed.netloc != "lightning-goats.com":
            route.fulfill(body="")
            return
        path = parsed.path
        if path == "/site-config.js":
            route.fulfill(content_type="text/javascript", body="window.LightningGoatsSite={contactPaymentsEnabled:false}")
        elif path == "/vendor/nostr.bundle.js":
            route.fulfill(content_type="text/javascript", body=(ROOT / path.lstrip("/")).read_text() + FIXTURE)
        elif path == "/vendor/nostr-login.js":
            route.fulfill(content_type="text/javascript", body="")
        elif path == "/contact.js":
            source = (ROOT / "contact.js").read_text()
            # Only the recipient is replaced so this isolated test can decrypt
            # envelopes with its own generated key, never the project credential.
            self.assertIn(f'const RECIPIENT = "{PROJECT}";', source)
            source = source.replace(f'const RECIPIENT = "{PROJECT}";', "const RECIPIENT = window.recipient;")
            route.fulfill(content_type="text/javascript", body=source)
        elif path.startswith("/api/") or path.startswith("/.well-known/") or path.startswith("/lnurlp/"):
            if "contact" in path or "lnurlp" in path:
                self.provider_requests.append(route.request.url)
            route.fulfill(status=503, content_type="application/json", body="{}")
        else:
            file = ROOT / ("index.html" if path == "/" else path.lstrip("/"))
            if file.is_file() and file.resolve().is_relative_to(ROOT):
                route.fulfill(content_type=mimetypes.guess_type(file)[0] or "text/plain", body=file.read_bytes())
            else:
                route.fulfill(body="")

    def open(self):
        self.page.goto(ORIGIN + "/", wait_until="domcontentloaded", timeout=120000)

    def test_identity_bound_nip17_roundtrip_and_anonymous_email_validation(self):
        self.open()
        result = self.page.evaluate("""async()=>{
          const c=LightningGoatsContact;
          const wrapped=await c.signedEnvelope(nostr,sender,'Private contact fixture');
          const rumor=NativeNostrTools.nip59.unwrapEvent(wrapped,recipientKey);
          const result={kind:wrapped.kind,rumorKind:rumor.kind,sender:rumor.pubkey===sender,
            recipient:rumor.tags[0][1]===recipient,content:rumor.content,
            noPlaintext:!JSON.stringify(wrapped).includes('Private contact fixture')};
          try{c.anonymousEnvelope('message','');result.missingRejected=false;}catch(_){result.missingRejected=true;}
          try{c.anonymousEnvelope('message','bad\\naddress@example.org');result.invalidRejected=false;}catch(_){result.invalidRejected=true;}
          const anonymous=c.anonymousEnvelope('Email contact fixture','visitor@example.org');
          const anon=NativeNostrTools.nip59.unwrapEvent(anonymous,recipientKey);
          result.anonContent=anon.content;result.anonDifferent=anon.pubkey!==sender;
          const changedSigner={...nostr,async signEvent(t){return NativeNostrTools.finalizeEvent({...t,kind:1},senderKey);}};
          try{await c.signedEnvelope(changedSigner,sender,'secret');result.changedRejected=false;}catch(_){result.changedRejected=true;}
          try{await c.signedEnvelope({...nostr,nip44:{async encrypt(){return 'plaintext'}}},sender,'secret');result.plainRejected=false;}catch(_){result.plainRejected=true;}
          result.publications=published.length;return result;
        }""")
        self.assertEqual(result["kind"], 1059)
        self.assertEqual(result["rumorKind"], 14)
        for name in ["sender", "recipient", "noPlaintext", "missingRejected", "invalidRejected", "anonDifferent", "changedRejected", "plainRejected"]:
            self.assertTrue(result[name], name)
        self.assertIn("Private contact fixture", result["content"])
        self.assertIn("Reply email: visitor@example.org", result["anonContent"])
        self.assertEqual(result["publications"], 0)
        self.assertEqual(self.provider_requests, [])
        self.assertEqual(self.errors, [])

    def test_free_contact_retains_ciphertext_and_retries_exact_event_after_reload(self):
        self.open()
        self.page.locator("#contactLink").click()
        self.page.locator("#contactModal").wait_for(state="visible")
        self.assertTrue(self.page.locator("#contactEmail").evaluate("e=>e.required"))
        self.assertTrue(self.page.locator("#contactSend").is_disabled())
        self.page.locator("#contactConnect").click()
        self.page.wait_for_function("!document.getElementById('contactEmail').required")
        self.page.locator("#contactMessage").fill("Confidential fixture contact")
        self.page.locator("#contactSend").click()
        self.page.wait_for_function("document.getElementById('contactSend').textContent==='Retry encrypted message'")
        saved = self.page.evaluate("JSON.parse(localStorage.getItem('lightning-goats-contact-pending-v1'))")
        self.assertNotIn("Confidential fixture contact", json.dumps(saved))
        self.assertEqual(self.page.evaluate("signCalls"), [13])
        self.assertEqual(self.page.evaluate("published[0].event.id"), saved["event"]["id"])
        # Simulate reload while preserving the same visitor signer and browser
        # storage. Production stores no private key; this fixture injects its key.
        keys = self.page.evaluate("({sender:Array.from(senderKey),recipient:Array.from(recipientKey)})")
        self.context.add_init_script("window.fixtureReuseSender=" + json.dumps(keys["sender"]) + ";window.fixtureReuseRecipient=" + json.dumps(keys["recipient"]))
        self.page.reload(wait_until="domcontentloaded")
        self.page.locator("#contactLink").click()
        self.page.locator("#contactModal").wait_for(state="visible")
        self.page.locator("#contactConnect").click()
        self.page.wait_for_function("!document.getElementById('contactEmail').required")
        self.page.evaluate("window.failRelays=false")
        self.page.locator("#contactSend").click()
        self.page.wait_for_function("document.getElementById('contactStatus').textContent.includes('accepted your encrypted message')")
        self.assertEqual(self.page.evaluate("published[0].event.id"), saved["event"]["id"])
        self.assertEqual(self.page.evaluate("signCalls"), [])
        self.assertIsNone(self.page.evaluate("localStorage.getItem('lightning-goats-contact-pending-v1')"))
        self.assertEqual(self.provider_requests, [])
        self.assertEqual(self.errors, [])


if __name__ == "__main__":
    unittest.main()
