"""Fund a disposable mint and redeem a direct operator transfer with an offline wallet."""
import hashlib
import json
import os
import subprocess
import socketserver
import sqlite3
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
import threading
from uuid import uuid4

import bolt11
from cashu.core.base import TokenV3
from coincurve import PrivateKey
import httpx
from mnemonic import Mnemonic
from mint_acceptance import Fixture, PROJECT_TOKEN, encode_outputs, expect, new_outputs, proof, start_mint

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "collector"))
from claim_store import ClaimStore, encode
from export_operator_token import export
from operator_ecash import MINT, transfer
from payout_server import dispatch_payout
from invoice_server import handle_request


def verify_legacy_upgrade():
    migrations = Path(__file__).resolve().parents[3] / "payout-migrations"
    with sqlite3.connect(":memory:") as db:
        db.execute("PRAGMA foreign_keys=ON")
        for migration in sorted(migrations.glob("*.sql")):
            if migration.name.startswith("0014_"):
                break
            db.executescript(migration.read_text())
        for sequence in (1, 2):
            db.execute("INSERT INTO payout_intake_receipts VALUES(?,?,?,NULL)",
                       (sequence, "aa"*32, str(uuid4())))
            db.execute("INSERT INTO payout_operator_allocations VALUES(?,?,32,14,18)",
                       (sequence, "ab"*32))
        legacy = ("sat@lightning-goats.com", "original-invoice", "01"*32)
        db.execute("INSERT INTO payout_operator_invoices VALUES(1,?,?,?,17,1)", legacy)
        db.executescript((migrations/"0014_operator_methods.sql").read_text())
        assert db.execute("SELECT method,destination FROM payout_operator_methods").fetchone() == ("lightning",legacy[0])
        assert db.execute("SELECT address,invoice,payment_hash FROM payout_operator_invoices").fetchone() == legacy
        db.execute("INSERT INTO payout_operator_methods VALUES(2,'p2pk','frozen-wallet')")
        try:
            db.execute("INSERT INTO payout_operator_invoices VALUES(2,?,?,?,17,1)", (legacy[0],"new-invoice","02"*32))
        except sqlite3.IntegrityError:
            pass
        else:
            raise AssertionError("legacy writer could spend a direct allocation")


def exercise(ppk, failed_members=False, ecash_members=False):
    with TemporaryDirectory(prefix="lg-direct-") as raw:
        root = Path(raw)
        (root / "mint-seed").write_text(Mnemonic("english").generate(strength=128))
        (root / "project-quote-token").write_text(PROJECT_TOKEN)
        with Fixture(root / "reserve.sock") as reserve:
            threading.Thread(target=reserve.serve_forever, daemon=True).start()
            process, mint = start_mint(root, root / "mint.sock", root / "reserve.sock", root, input_fee_ppk=ppk)
            try:
                item = expect(mint.get("/v1/keys"))["keysets"][0]
                keyset, keys = item["id"], item["keys"]
                q = expect(mint.post("/v1/mint/quote/bolt11", json={"amount":32,"unit":"sat"}))
                payment_hash = bolt11.decode(q["request"]).payment_hash
                reserve.paid.add(payment_hash)
                outputs, material = new_outputs(keyset,[32])
                sig, = expect(mint.post("/v1/mint/bolt11",json={"quote":q["quote"],"outputs":encode_outputs(outputs)}))["signatures"]
                original=[proof(sig,material[0],keys)]
                claim,feed=str(uuid4()),str(uuid4())
                store=ClaimStore(root)
                store.prepare(claim,{"attribution":{"credit_pool":"herd"}},list)
                store.record_invoice(claim,q["request"],payment_hash)
                store.record_proofs(claim,payment_hash,original)
                wallet=PrivateKey()
                evidence=hashlib.sha256(encode(original).encode()).hexdigest()
                request={"feed":feed,"mint":MINT,"recipient":wallet.public_key.format().hex(),"gross":18,"member":14,
                         "plan_hash":"aa"*32,"fee_limit":18,"lots":[{"ledger_id":1,"consumed_sats":32,
                         "anchor":{"source":"project_invoice","claim_id":claim,"evidence_digest":evidence}}]}
                paths=[]
                class MintOnly:
                    def get(self,path):
                        assert path in ("/v1/info","/v1/keysets")
                        paths.append(path)
                        return mint.get(path)
                    def post(self,path,**kwargs):
                        assert path in ("/v1/swap","/v1/restore","/v1/checkstate")
                        paths.append(path)
                        response=mint.post(path,**kwargs)
                        if path=="/v1/swap":
                            response.raise_for_status()
                            raise httpx.ReadTimeout("accepted swap reply lost")
                        return response
                class Unsupported(MintOnly):
                    def get(self,path):
                        assert path=="/v1/info"
                        return httpx.Response(200,request=httpx.Request("GET","http://fixture"),json={"nuts":{}})
                assert transfer(store,Unsupported(),request,keyset,{keyset:keys})=={"state":"held","reason":"nut11_unsupported"}
                if ppk:
                    for gross,limit,reason in ((1,1,"no_payable_amount"),(18,0,"fee_limit")):
                        held=transfer(store,MintOnly(),{**request,"gross":gross,"member":32-gross,"fee_limit":limit},keyset,{keyset:keys})
                        assert held=={"state":"held","reason":reason}
                    assert store.db.execute("SELECT COUNT(*) FROM disposition_input").fetchone()==(0,)
                try: transfer(store,MintOnly(),{**request,"recipient":"03"+"ff"*32},keyset,{keyset:keys})
                except ValueError: pass
                else: raise AssertionError("invalid receiving key accepted")
                fee=1 if ppk else 0
                if binary := os.environ.get("LG_DIRECT_WORKER_TEST"):
                    (root/"worker.json").write_text(json.dumps({"claim":claim,"recipient":request["recipient"],
                                                              "payment_hash":payment_hash,"fee":fee}))
                    class Handler(socketserver.BaseRequestHandler):
                        def handle(self):
                            local=ClaimStore(root)
                            try:
                                def dispatch(message):
                                    if message=={"claim_status":claim}:
                                        return {"request_id":claim,"payment_hash":payment_hash,"amount_sat":32,
                                                "proof_digest":evidence}
                                    return dispatch_payout(local,MintOnly(),message,keyset,{keyset:keys})
                                handle_request(self.request,os.getuid(),dispatch,65536)
                            finally:
                                local.db.close()
                    store.db.close()
                    with socketserver.UnixStreamServer(str(root/"collector.sock"),Handler) as server:
                        threading.Thread(target=server.serve_forever,daemon=True).start()
                        try:
                            subprocess.run([binary,"direct_operator_worker_fixture","--ignored","--nocapture"],
                                           env={**os.environ,"LG_DIRECT_FIXTURE":str(root)},check=True,timeout=120)
                        finally:
                            server.shutdown()
                    feed=(root/"feed").read_text()
                else:
                    try: transfer(store,MintOnly(),request,keyset,{keyset:keys})
                    except httpx.ReadTimeout: pass
                    else: raise AssertionError("lost reply was not simulated")
                    class Unrestored(MintOnly):
                        def post(self,path,**kwargs):
                            if path=="/v1/restore":
                                return httpx.Response(200, request=httpx.Request("POST","http://fixture"),
                                                      json={"outputs":[],"signatures":[]})
                            return super().post(path,**kwargs)
                    for client, terms in ((Unrestored(),request),(MintOnly(),{**request,"recipient":PrivateKey().public_key.format().hex()})):
                        try: transfer(store,client,terms,keyset,{keyset:keys})
                        except ValueError: pass
                        else: raise AssertionError("unresolved or changed payout accepted")
                    store.db.close()
                    def retry(_):
                        s=ClaimStore(root)
                        try: return transfer(s,MintOnly(),request,keyset,{keyset:keys})
                        finally: s.db.close()
                    with ThreadPoolExecutor(2) as pool: results=list(pool.map(retry,range(2)))
                    fee=1 if ppk else 0
                    assert results==[{"state":"ready","amount_sat":18-fee,"fee_sat":fee}]*2
                assert paths.count("/v1/swap")==1 and reserve.pay_attempts==0
                store=ClaimStore(root)
                assert store.db.execute("SELECT COUNT(*) FROM proof_disposition").fetchone()==(1,)
                if failed_members:
                    from member_reclaim import reclaim, receipt
                    failures = [dict(feed=feed, member=m*64, gross=7, fee_limit=7, plan=request["plan_hash"],
                                     recipient=request["recipient"], reserved=14) for m in ("1","2")]
                    try:
                        reclaim(store,MintOnly(),failures,keyset,{keyset:keys})
                    except httpx.ReadTimeout:
                        pass
                    else:
                        raise AssertionError("lost reply fixture did not swap")
                    store.db.close()
                    store=ClaimStore(root)
                    result=reclaim(store,MintOnly(),list(reversed(failures)),keyset,{keyset:keys})
                    assert result==[{"state":"ready","amount_sat":14-fee,"fee_sat":fee}]
                    assert reclaim(store,MintOnly(),failures,keyset,{keyset:keys})==[]
                    for row in failures:
                        assert receipt(store.db,feed,request["plan_hash"],row["member"])==7
                        assert receipt(store.db,feed,"cc"*32,row["member"]) is None
                    assert paths.count("/v1/swap")==2 and reserve.pay_attempts==0
                    assert store.db.execute("SELECT SUM(reserved_sat) FROM payout_claim_allocation WHERE feed_id=?",
                                            (feed,)).fetchone()==(32,)
                    print("Verified failed member batch, original funding, lost response and duplicate recovery", flush=True)
                    store.db.close()
                    return
                if ecash_members:
                    grant = str(uuid4())
                    sender_key = PrivateKey()
                    for method in ("private_ecash","public_nutzap"):
                        receiver, identity = PrivateKey(), PrivateKey()
                        member_key = identity.public_key.format()[1:].hex()
                        lock = receiver.public_key.format().hex()
                        if method == "public_nutzap" and not lock.startswith("02"):
                            receiver = PrivateKey.from_int(
                                0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
                                - int.from_bytes(receiver.secret, "big"))
                            lock = receiver.public_key.format().hex()
                        destination={"method":method,"mint":MINT,"receiving_key":lock}
                        if method=="public_nutzap":
                            destination.update(mint=MINT+"/",member=member_key,advertisement="ab"*32,
                                token_relays=["wss://relay.example.com"],read_relays=["wss://read.example.com"])
                        else:
                            destination.update(request_digest="cd"*32,payment_id="reward",
                                transport_key=member_key,relays=["wss://relay.example.com"])
                        assignment = {"feed":feed,"grant":grant,"owner":sender_key.public_key.format()[1:].hex(),"member_key":member_key,"plan_hash":request["plan_hash"],
                            "destination":destination,"gross":7,"reserved":14,"lots":request["lots"],
                            "fee_limit":2,"minimum_net":5,"maximum_net":7}
                        def send(terms):
                            return dispatch_payout(store,MintOnly(),{"member_ecash":terms},keyset,{keyset:keys})
                        try:
                            send(assignment)
                        except httpx.ReadTimeout:
                            pass
                        else:
                            raise AssertionError("expected a lost member swap reply")
                        store.db.close()
                        store = ClaimStore(root)
                        try:
                            send({**assignment,"destination":{**destination,"receiving_key":PrivateKey().public_key.format().hex()}})
                        except ValueError:
                            pass
                        else:
                            raise AssertionError("changed member destination accepted")
                        assert send(assignment) == send(assignment) == {"state":"ready","fee_sat":fee,"amount_sat":7-fee}
                        row = store.db.execute("SELECT token FROM member_ecash WHERE feed=?",
                            (f"member-ecash:{feed}:{member_key}",)).fetchone()
                        proofs = [p.model_dump(exclude_none=True) for p in TokenV3.deserialize(row[0]).proofs]
                        if method == "public_nutzap":
                            from operator_delivery import public_reward_tags
                            tags = public_reward_tags(row[0],7-fee,assignment)
                            assert tags[:3] == [["p",member_key],["u",MINT+"/"],["unit","sat"]]
                            proofs = [json.loads(value) for kind,value in tags if kind == "proof"]
                            from member_publication import publication
                            from operator_delivery import deliver_public_members
                            import sqlite3
                            digest = hashlib.sha256(json.dumps(assignment,ensure_ascii=False,sort_keys=True,separators=(",",":")).encode()).hexdigest()
                            lookup = {"feed":feed,"member":member_key,"terms_hash":digest}
                            draft = publication(store,lookup)
                            assert not draft["signed"] and draft["unsigned"]["tags"] == tags
                            event = dict(draft["unsigned"],pubkey=assignment["owner"])
                            serialized = json.dumps([0,event["pubkey"],event["created_at"],9321,tags,""],separators=(",",":")).encode()
                            event["id"] = hashlib.sha256(serialized).hexdigest()
                            event["sig"] = sender_key.sign_schnorr(bytes.fromhex(event["id"])).hex()
                            try:
                                publication(store,{**lookup,"event":{**event,"pubkey":member_key}})
                            except ValueError:
                                pass
                            else:
                                raise AssertionError("accepted changed sender")
                            assert publication(store,{**lookup,"event":event})["signed"]
                            assert publication(store,lookup)["signed"]
                            seen = {}
                            def relay(url,signed):
                                assert signed == event
                                seen[url] = seen.get(url,0)+1
                                return url in destination["read_relays"] or seen[url]==2
                            for attempt in range(3):
                                journal = sqlite3.connect(root/"public-outbox.sqlite")
                                deliver_public_members(store.db,journal,relay)
                                ack = journal.execute("SELECT ack_at FROM delivery").fetchone()[0]
                                assert (ack is not None) == (attempt>0)
                                journal.close()
                            assert seen == {"wss://relay.example.com":2,"wss://read.example.com":1}
                        assert sum(p["amount"] for p in proofs)==7-fee
                        for p in proofs:
                            assert p["dleq"] and json.loads(p["secret"])[1]["data"]==lock
                            p["witness"]=json.dumps({"signatures":[receiver.sign_schnorr(hashlib.sha256(p["secret"].encode()).digest()).hex()]})
                        net=7-2*fee
                        outs,_=new_outputs(keyset,[1<<i for i in range(net.bit_length()) if net & (1<<i)])
                        redemption={"inputs":proofs,"outputs":encode_outputs(outs)}
                        expect(mint.post("/v1/swap",json=redemption))
                        # An exact NUT-19 replay may return the original signatures.
                        # Fresh outputs would constitute a second economic redemption.
                        retry,_=new_outputs(keyset,[o.amount for o in outs])
                        assert mint.post("/v1/swap",json={**redemption,"outputs":encode_outputs(retry)}).status_code>=400
                    assert paths.count("/v1/swap")==3 and reserve.pay_attempts==0
                    assert store.db.execute("SELECT COUNT(*) FROM operator_ecash").fetchone()==(1,)
                    assert store.db.execute("SELECT COUNT(*) FROM member_ecash").fetchone()==(2,)
                    store.db.close()
                    print("Verified two member swaps, offline recovery and independent wallet redemption",flush=True)
                    return
                member=store.reserve_payout(str(uuid4()),feed,"bb"*32,14,0,{claim:{"consumed_sats":32,"evidence_digest":evidence}})
                assert sum(p["amount"] for p in member)==14
                store.db.close()
                for name in ("first","again"):
                    export(root/"project-claims.sqlite",feed,root/name)
                encoded=(root/"first").read_text().strip()
                assert encoded==(root/"again").read_text().strip()
                if sdk := os.environ.get("LG_CASHU_TS_INSTALLATION"):
                    checker = Path(__file__).resolve().parents[3]/"tools/cashu-client-compat/decode-operator-token.mjs"
                    subprocess.run(["node",str(checker),sdk],input=json.dumps({"token":encoded,"keyset":keyset,
                        "amount":18-fee,"recipient":request["recipient"]}),text=True,check=True)
                token=TokenV3.deserialize(encoded)
                spend=[p.model_dump(exclude_none=True) for p in token.proofs]
                assert sum(p["amount"] for p in spend)==18-fee
                assert len({json.loads(p["secret"])[1]["nonce"] for p in spend})==len(spend)
                for p in spend:
                    kind,data=json.loads(p["secret"])
                    assert kind=="P2PK" and set(data)=={"nonce","data"} and data["data"]==request["recipient"]
                redeemed=18-fee-(1 if ppk else 0)
                outs,_=new_outputs(keyset,[1<<i for i in range(redeemed.bit_length()) if redeemed & (1<<i)])
                def redeem(key):
                    inputs=[dict(p) for p in spend]
                    if key:
                        for p in inputs:
                            sig=key.sign_schnorr(hashlib.sha256(p["secret"].encode()).digest())
                            p["witness"]=json.dumps({"signatures":[sig.hex()]})
                    return mint.post("/v1/swap",json={"inputs":inputs,"outputs":encode_outputs(outs)})
                assert redeem(None).status_code>=400
                assert redeem(PrivateKey()).status_code>=400
                expect(redeem(wallet))
                assert redeem(wallet).status_code>=400
                print("direct payout verified",ppk,"ppk",18-fee,"operator sats, 14 member sats",flush=True)
            finally:
                mint.close()
                process.terminate()
                process.wait(timeout=15)

if __name__=="__main__":
    verify_legacy_upgrade()
    for rate in (0,100): exercise(rate)
    exercise(100, failed_members=True)
    exercise(100, ecash_members=True)
