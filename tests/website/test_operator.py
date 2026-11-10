"""Nostr HTTP auth / operator browser behavior; all network traffic is mocked."""
import base64
import hashlib
import json
import mimetypes
import os
from pathlib import Path
import unittest
from urllib.parse import urlsplit
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2] / "web"
ORIGIN = "https://lightning-goats.com"

class OperatorBrowserTests(unittest.TestCase):
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
        self.page.add_init_script("""
            window.signedRequests=[];
            const fixtureKey=new Uint8Array(32);fixtureKey[31]=3;
            window.nostr={getPublicKey:async()=>window.NostrTools.getPublicKey(fixtureKey),signEvent:async event=>{
                window.signedRequests.push(structuredClone(event));return window.NostrTools.finalizeEvent(event,fixtureKey);
            }};
        """)
        self.errors=[]
        self.page.on("pageerror",lambda error:self.errors.append(str(error)))
        self.enabled=False
        self.operator=True
        self.requests=[]
        self.commands=[]
        self.drop_first=False
        self.state={"owner":"ab"*32,"cycle":1,"revision":0,"settings":{"minimum_sats":"10","max_members":10,"member_percent":10},"members":[],"next_offset":None,"categories":[{"category":"member_added","enabled":True}]}
        self.page.route("**/*",self.route)
    def route(self,route):
        request=route.request
        url=urlsplit(request.url)
        self.requests.append((request.method,url.path))
        if url.path=="/site-config.js":
            route.fulfill(content_type="text/javascript",body="window.LightningGoatsSite="+json.dumps({"cyberherdEnabled":self.enabled,"cyberherdAuthEnabled":self.enabled}))
        elif url.path.startswith("/api/"):
            authorization=request.headers.get("authorization","")
            self.assertTrue(authorization.startswith("Nostr "))
            event=json.loads(base64.b64decode(authorization[6:]))
            tags={t[0]:t[1] for t in event["tags"]}
            self.assertEqual(event["kind"],27235)
            self.assertEqual(tags["u"],request.url)
            self.assertEqual(tags["method"],request.method)
            if url.path=="/api/v1/cyberherd/me":
                route.fulfill(content_type="application/json",body=json.dumps({"pubkey":event["pubkey"],"operator":self.operator,"member":{"active":False,"amount_sats":"0"}}))
            elif url.path=="/api/v1/cyberherd/operator/state":
                route.fulfill(content_type="application/json",body=json.dumps(self.state))
            elif url.path=="/api/v1/cyberherd/operator/command":
                body=request.post_data
                self.assertEqual(tags["payload"],hashlib.sha256(body.encode()).hexdigest())
                self.commands.append((body,event["id"]))
                command=json.loads(body)
                self.state["revision"]=1
                self.state["settings"].update({"minimum_sats":"20","member_percent":0})
                if self.drop_first and len(self.commands)==1:
                    route.abort()
                else:
                    route.fulfill(content_type="application/json",body=json.dumps({"command_id":command["id"],"revision":1,"cycle":1,"changed":True,"event_seq":1}))
            else:
                route.abort()
        else:
            file=ROOT/url.path.lstrip("/")
            if url.netloc=="lightning-goats.com" and file.is_file() and file.resolve().is_relative_to(ROOT):
                route.fulfill(content_type=mimetypes.guess_type(file)[0] or "text/plain",body=file.read_bytes())
            else:
                route.abort()
    def test_disabled_operator_page_does_not_sign_or_contact_an_api(self):
        self.page.goto(ORIGIN+"/cyberherd-operator.html")
        self.assertTrue(self.page.locator("#connect").is_hidden())
        self.assertTrue(self.page.locator("#operator").is_hidden())
        self.assertFalse(any(path.startswith("/api/") for _,path in self.requests))
        self.assertEqual(self.page.evaluate("window.signedRequests"),[])
        self.assertEqual(self.page.locator('input[type=password],input[name=nsec]').count(),0)
        self.assertEqual(self.errors,[])
    def test_nonoperator_signer_cannot_open_controls(self):
        self.enabled=True
        self.operator=False
        self.page.goto(ORIGIN+"/cyberherd-operator.html")
        self.page.locator("#connect").click()
        self.page.wait_for_function("document.querySelector('#message').textContent.includes('not an authorized operator')")
        self.assertTrue(self.page.locator("#operator").is_hidden())
        self.assertFalse(any("/operator/" in path for _,path in self.requests))
        self.assertEqual(self.errors,[])
    def test_lost_response_retains_exact_command_but_signs_a_fresh_authorization_for_retry(self):
        self.enabled=True
        self.drop_first=True
        self.page.goto(ORIGIN+"/cyberherd-operator.html")
        self.page.locator("#connect").click()
        self.page.wait_for_function("document.querySelector('#revision').textContent.includes('revision 0')")
        self.page.locator("#minimum").fill("20")
        self.page.locator("#percent").fill("0")
        self.page.locator("#settings button").click()
        self.page.wait_for_function("!document.querySelector('#retry').hidden && !document.querySelector('#retry').disabled")
        self.assertEqual(len(self.commands),1)
        self.assertTrue(self.page.evaluate("Boolean(sessionStorage.getItem('lg-cyberherd-pending-command-v1'))"))
        self.page.locator("#retry").click()
        self.page.wait_for_function("document.querySelector('#message').textContent==='Change saved.'")
        self.assertEqual(len(self.commands),2)
        self.assertEqual(self.commands[0][0],self.commands[1][0])
        self.assertNotEqual(self.commands[0][1],self.commands[1][1])
        self.assertEqual(json.loads(self.commands[0][0])["expected_revision"],0)
        self.assertEqual(self.page.locator("#percent").input_value(),"0")
        self.assertFalse(self.page.evaluate("Boolean(sessionStorage.getItem('lg-cyberherd-pending-command-v1'))"))
        self.assertTrue(all(path.startswith('/api/v1/cyberherd/') for method,path in self.requests if method=="POST"))
        self.assertEqual(self.errors,[])
