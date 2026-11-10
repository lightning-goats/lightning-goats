import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

HERE=Path(__file__).resolve().parent
ROOT=HERE.parent.parent

def load(name,path):
    spec=importlib.util.spec_from_file_location(name,HERE/path)
    m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
probe=load("sync_probe","sync-probe.py")
prepare=load("prepare","prepare.py")
renderer=load("renderer","render-config.py")

class Tools(unittest.TestCase):
    def fixture(self):
        primary="4"*95
        cfg={"network":"mainnet","provider_url":"http://127.0.0.1:18993/","account_scope":hashlib.sha256(primary.encode()).hexdigest(),"max_wallet_lag":0,"wallet_rpc_url":"http://127.0.0.1:18994/json_rpc","daemon_rpc_url":"http://127.0.0.1:18081/json_rpc"}
        node={"status":"OK","nettype":"mainnet","synchronized":True,"offline":False,"busy_syncing":False,"untrusted":False,"height":100,"target_height":0,"incoming_connections_count":0,"outgoing_connections_count":2}
        calls=[]
        def call(url,method,params,credentials):
            calls.append((url,method,params))
            if method=="get_info":return node.copy()
            if method=="get_address":return {"address":primary}
            if method=="get_height":return {"height":100}
            self.fail("spending/admin call attempted")
        return cfg,node,call,calls
    def test_sync_probe_only_reads_and_emits_no_native_wallet_data(self):
        cfg,_,call,calls=self.fixture();r=probe.inspect(cfg,call,lambda:1000)
        self.assertTrue(r["synchronized"]);self.assertEqual([c[1] for c in calls],["get_info","get_address","get_height","get_info"])
        self.assertNotIn("address",r);self.assertNotIn("balance",r)
    def test_sync_rejects_wrong_network_offline_unsynced_and_no_peers(self):
        for key,value in [("nettype","testnet"),("offline",True),("untrusted",True),("synchronized",False),("busy_syncing",True),("target_height",101),("outgoing_connections_count",0),("outgoing_connections_count",True)]:
            c,n,call,_=self.fixture();n[key]=value
            with self.assertRaises(ValueError):probe.inspect(c,call,lambda:1000)
    def test_sync_rejects_wallet_identity_or_clock_conflict(self):
        c,n,call,_=self.fixture();c["account_scope"]="a"*64
        with self.assertRaises(ValueError):probe.inspect(c,call,lambda:1000)
        c,n,call,_=self.fixture();ticks=iter([1000,999])
        with self.assertRaises(ValueError):probe.inspect(c,call,lambda:next(ticks))
        ticks=iter([1000,1021])
        with self.assertRaises(ValueError):probe.inspect(c,call,lambda:next(ticks))
    def test_no_rpc_redirect_or_spending_route(self):
        with self.assertRaises(ValueError):probe.rpc("http://127.0.0.1:18994/json_rpc","transfer",{})
        for value in ["http://evil:123/json_rpc","http://127.0.0.1:5000/json_rpc","http://a:b@127.0.0.1:12/json_rpc","http://127.0.0.1:12/json_rpc?x=y"]:
            with self.assertRaises(ValueError):probe.endpoint(value,"/json_rpc")
        with self.assertRaises(ValueError):probe.NoRedirect().redirect_request(None,None,302,"",{},"http://evil")
    def test_strict_json_and_private_config(self):
        with self.assertRaises(ValueError):probe.strict_json('{"height":1,"height":2}')
        with self.assertRaises(ValueError):probe.strict_json('{"height":NaN}')
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"cfg";p.write_text('{}');p.chmod(0o600);self.assertEqual(probe.read_private(p),{})
            p.chmod(0o644)
            with self.assertRaises(ValueError):probe.read_private(p)
            p.chmod(0o600);s=Path(d)/"link";s.symlink_to(p)
            with self.assertRaises(OSError):probe.read_private(s)
    def test_report_is_atomic_private_and_false_is_preserved(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"report";probe.write_report(p,{"synchronized":False})
            self.assertFalse(json.loads(p.read_text())["synchronized"]);self.assertEqual(p.stat().st_mode&0o777,0o640)
            self.assertEqual([i.name for i in Path(d).iterdir()],["report"])
    def test_backup_and_prepared_restore_preserve_history_but_cannot_enable_credit(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);db=sqlite3.connect(p/"source.db")
            db.executescript((ROOT/"src/monero_bridge/schema.sql").read_text())
            db.execute("INSERT INTO monero_bridge_meta VALUES(1,'private','old-generation',100)")
            db.execute("INSERT INTO monero_bridge_intents(id,request,reserved_at,callback_token,state,created,address,snapshot,available) VALUES('id','{}',100,'private','bound','{}','subaddress','[]',1)")
            db.commit();db.close()
            r=prepare.backup(p/"source.db",p/"copy.db");self.assertEqual(r["sha256"],hashlib.sha256((p/"copy.db").read_bytes()).hexdigest())
            r=prepare.backup(p/"source.db",p/"restore.db",True)
            with sqlite3.connect(p/"restore.db") as conn:
                self.assertNotEqual(conn.execute('SELECT generation FROM monero_bridge_meta').fetchone()[0],'old-generation')
                self.assertEqual(conn.execute('SELECT hold,available,address FROM monero_bridge_intents').fetchone(),('restore_review_required',0,'subaddress'))
            self.assertEqual(r["sha256"],hashlib.sha256((p/"restore.db").read_bytes()).hexdigest())
            with self.assertRaises(ValueError):prepare.backup(p/"source.db",p/"copy.db")
            (p/"unsafe.db-wal").write_bytes(b"unrelated")
            with self.assertRaises(ValueError):prepare.backup(p/"source.db",p/"unsafe.db")
            self.assertEqual((p/"unsafe.db-wal").read_bytes(),b"unrelated")
            self.assertFalse((p/"unsafe.db").exists())
    def test_stage_checks_digest_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);b=p/"binary";b.write_bytes(b"synthetic-test-not-executable")
            with self.assertRaises(ValueError):prepare.stage(b,"bad",p/"stage")
            r=prepare.stage(b,hashlib.sha256(b.read_bytes()).hexdigest(),p/"stage");self.assertFalse(r["activated"])
            self.assertTrue((p/"stage/SHA256SUMS").exists())
            with self.assertRaises(FileExistsError):prepare.stage(b,hashlib.sha256(b.read_bytes()).hexdigest(),p/"stage")
    def values(self):
        return {"account_scope":"a"*64,"network":"mainnet","bridge_uid":999,"home_ipv4":"10.8.0.6","vps_ipv4":"10.8.0.12","bridge_port":18991,"callback_port":18992,"provider_port":18993,"wallet_port":18994,"daemon_port":18081,"mtls_port":18995,"max_amount_atomic":1000000000000,"server_cert":"/etc/lg/server.crt","server_key":"/etc/lg/server.key","client_ca":"/etc/lg/client-ca.crt"}
    def test_renderer_stays_disabled_and_injectable_values_are_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);r=renderer.render(self.values(),p/"rendered");self.assertFalse(r["activated"])
            self.assertIn("create_enabled = false",(p/"rendered/config.toml").read_text())
            self.assertIn("ssl_verify_client on",(p/"rendered/nginx-mtls.conf").read_text())
            for key,value in [("server_cert","/x;evil"),("provider_port",5000),("bridge_uid",0),("vps_ipv4","0.0.0.0"),("wallet_port",18993)]:
                v=self.values();v[key]=value
                with self.assertRaises(ValueError):renderer.render(v,p/"invalid")
    def test_installer_usage_does_not_change_hosts_and_shell_syntax_is_valid(self):
        script=HERE/"install-inactive.sh"
        self.assertEqual(subprocess.run(["bash","-n",script]).returncode,0)
        self.assertEqual(subprocess.run(["bash",script,"--not-approved"],capture_output=True).returncode,2)
        text=script.read_text();self.assertNotIn("systemctl start",text);self.assertNotIn("systemctl enable",text)
        self.assertNotIn("flush ruleset",(HERE/"monero-egress.nft.example").read_text().split('table inet')[1])

if __name__=="__main__":unittest.main()
