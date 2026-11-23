"""Verify private NUT-18 delivery, offline retry and recipient-only decryption."""
import base64
import json
from pathlib import Path
from unittest.mock import Mock, patch
import shutil
import sqlite3
import subprocess
import sys
import tempfile

import bech32
import cbor2
from coincurve import PrivateKey

repo = Path(__file__).resolve().parents[3]
collector = repo/"deploy/breez-reserve/collector"
sys.path.insert(0, str(collector))
from operator_delivery import MINT, deliver, publish


def run():
    key = PrivateKey()
    lock = key.public_key.format().hex()
    url = b"wss://relay.example"
    profile = bytes([0,32])+bytes.fromhex(lock[2:])+bytes([1,len(url)])+url
    address = bech32.bech32_encode("nprofile",bech32.convertbits(profile,8,5,True))
    req = {"i":"fixture","u":"sat","nut10":{"k":"P2PK","d":lock},
           "t":[{"t":"nostr","a":address,"g":[["n","17"]]}]}
    encoded = "creqA"+base64.urlsafe_b64encode(cbor2.dumps(req)).decode()
    proofs = [{"amount":8,"secret":json.dumps(["P2PK",{"nonce":"fixture-private-nonce","data":lock}])}]
    raw = json.dumps({"unit":"sat","token":[{"mint":MINT,"proofs":proofs}]})
    source = sqlite3.connect(":memory:")
    source.executescript("create table operator_ecash(feed,token,amount_sat);create table proof_disposition(id,binding);")
    source.execute("insert into operator_ecash values('earlier','invalid',8)")
    source.execute("insert into proof_disposition values('earlier','{}')")
    source.execute("insert into operator_ecash values('feed',?,8)",("cashuA"+base64.urlsafe_b64encode(raw.encode()).decode(),))
    source.execute("insert into proof_disposition values('feed',?)",(json.dumps({"recipient":lock}),))
    with tempfile.TemporaryDirectory() as tmp:
        root=Path(tmp)
        wrapper=root/"operator_wrap.cjs"
        shutil.copyfile(collector/wrapper.name, wrapper)
        shutil.copyfile(repo/"web/vendor/nostr.bundle.js",root/"nostr.bundle.js")
        proof_secret = "fixture-private-nonce"
        observed=[]
        def sender(url,event):
            assert url == "wss://relay.example"
            assert proof_secret not in json.dumps(event)
            observed.append(event)
            return len(observed)==2
        for _ in range(3):
            journal=sqlite3.connect(root/"outbox.sqlite")
            deliver(source,journal,encoded,wrapper,sender)
            journal.close()
        assert len(observed)==2 and observed[0]==observed[1]
        # A relay acknowledging another event cannot complete this token's delivery.
        relay = Mock()
        relay.recv.side_effect = [json.dumps(["OK", "00"*32, True, ""]), ConnectionError()]
        with patch("operator_delivery.websocket.create_connection", return_value=relay):
            assert not publish("wss://relay.example", observed[0])
        relay.close.assert_called_once()
        program="""const fs=require('fs'), sdk=require('./operator_wrap.cjs').sdk();
const {key,event}=JSON.parse(fs.readFileSync(0,'utf8'));
const rumor=sdk.nip59.unwrapEvent(event,Uint8Array.from(Buffer.from(key,'hex')));
if(rumor.kind!==14) process.exit(1);
process.stdout.write(rumor.content);"""
        result=subprocess.run(["node","-e",program],cwd=root,input=json.dumps({"key":key.secret.hex(),"event":observed[0]}),
                              capture_output=True,text=True,check=True)
        assert json.loads(result.stdout)=={"mint":MINT,"unit":"sat","proofs":proofs,"id":"fixture"}
        print("Private wallet delivery passed: decrypted NUT-18 payload, identical retry and one acknowledged delivery.")


if __name__=="__main__":
    run()
