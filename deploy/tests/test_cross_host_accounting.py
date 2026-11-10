import copy
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('verify_cross', ROOT/'deploy/scripts/verify-cross-host-accounting.py')
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class AccountingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.database = Path(self.temp.name)/'daemon.db'
        self.run_id = str(uuid.uuid4())
        self.ids = [str(uuid.uuid4()), str(uuid.uuid4())]
        self.connection = sqlite3.connect(self.database)
        self.addCleanup(self.connection.close)
        # Explicit timestamps avoid reliance on the host Python SQLite's
        # optional unixepoch() support; schema and application SQL stay unchanged.
        for name in ['0001_initial.sql','0003_backend_neutral_payments.sql','0004_strike_receive_requests.sql']:
            self.connection.executescript((ROOT/'migrations'/name).read_text())
        self.connection.execute("INSERT INTO settled_payments(source,source_id,address_user,credit_pool,amount_msat,received_at) VALUES('synthetic-cross-host',?,'herd','herd',2340000,0)", (self.run_id,))
        self.connection.execute("INSERT INTO ledger_entries(entry_type,source_key,delta_sats,payment_source,payment_source_id,created_at) VALUES('HERD_RECEIPT',?,2340,'synthetic-cross-host',?,0)", ('payment:synthetic-cross-host:'+self.run_id,self.run_id))
        payment = dict(source='synthetic-cross-host',source_id=self.run_id,address_user='herd',credit_pool='herd',amount_sats=2340,feed_credit_sats=2340)
        self.connection.execute("INSERT INTO event_log(event_type,payload_json,created_at) VALUES('payment_received',?,0)",(json.dumps(payment),))
        for ident, remaining in zip(self.ids, [1340,340]):
            self.connection.execute("INSERT INTO feed_attempts(id,status,threshold_sats,created_at) VALUES(?,'confirmed',1000,0)",(ident,))
            self.connection.execute("INSERT INTO ledger_entries(entry_type,source_key,delta_sats,feed_attempt_id,created_at) VALUES('FEED_DEBIT',?,-1000,?,0)",('feed:'+ident,ident))
            event = dict(feed_attempt_id=ident,threshold_sats=1000,feed_credit_sats=remaining)
            self.connection.execute("INSERT INTO event_log(event_type,payload_json,created_at) VALUES('feeder_confirmed',?,0)",(json.dumps(event),))
        self.connection.commit()
        old_id = str(uuid.uuid4())
        def row(sequence, ident):
            return dict(sequence=sequence,requestId=ident,status='released',receivedAt='2026-09-13T00:00:00Z',releasedAt='2026-09-13T00:00:06Z')
        self.before = dict(source_sha256=check.OWNER_SHA,count=1,hold='ON',remote_enabled='OFF',ack=old_id,deliveries=[row(1,old_id)])
        self.after = copy.deepcopy(self.before)
        self.after.update(count=3,ack=self.ids[-1])
        self.after['deliveries'].extend(row(i+2,ident) for i,ident in enumerate(self.ids))

    def verify(self):
        return check.verify(self.database,self.before,self.after,self.run_id)

    def test_two_correlated_commands_confirmations_debits_and_340_read_only(self):
        before = self.database.read_bytes()
        report = self.verify()
        self.assertEqual(report['remaining_sats'],340)
        self.assertEqual(report['confirmed_request_ids'],self.ids)
        self.assertEqual(self.database.read_bytes(),before)

    def test_every_command_counts_even_if_financial_ledger_is_correct(self):
        for mutation in ['extra','same_uuid','unrelated','gap','baseline_changed','held','wrong_source','remote_on']:
            with self.subTest(mutation=mutation):
                saved = copy.deepcopy(self.after)
                if mutation == 'extra':
                    extra = copy.deepcopy(self.after['deliveries'][-1]);extra['sequence']=4
                    self.after['deliveries'].append(extra);self.after['count']=4
                elif mutation == 'same_uuid':self.after['deliveries'][-1]['requestId']=self.ids[0]
                elif mutation == 'unrelated':self.after['deliveries'][1]['requestId']=str(uuid.uuid4())
                elif mutation == 'gap':self.after['count']=4
                elif mutation == 'baseline_changed':self.after['deliveries'][0]['receivedAt']='different'
                elif mutation == 'held':self.after['deliveries'][-1]['status']='held'
                elif mutation == 'wrong_source':self.after['source_sha256']='0'*64
                elif mutation == 'remote_on':self.after['remote_enabled']='ON'
                with self.assertRaises(ValueError):self.verify()
                self.after = saved

    def test_database_inconsistencies_and_public_work_rejected(self):
        mutations = [
            "UPDATE feed_attempts SET status='unknown' WHERE id='"+self.ids[-1]+"'",
            "DELETE FROM event_log WHERE seq=3",
            "UPDATE ledger_entries SET delta_sats=-999 WHERE entry_type='FEED_DEBIT'",
            "UPDATE settled_payments SET source='strike'",
            "INSERT INTO message_outbox(event_id,signed_event_json,status,created_at) VALUES('unexpected','{}','pending',0)",
            "INSERT INTO strike_receive_requests(receive_request_id,address_user,credit_pool,amount_msat,description_hash,payment_hash,invoice,created_at) VALUES('unexpected','herd','herd',1000,'"+'a'*64+"','"+'b'*64+"','invoice',0)",
        ]
        for statement in mutations:
            with self.subTest(statement=statement):
                original = self.database.read_bytes()
                self.connection.execute(statement);self.connection.commit()
                with self.assertRaises(ValueError):self.verify()
                self.connection.close()
                self.database.write_bytes(original)
                self.connection = sqlite3.connect(self.database)
                self.addCleanup(self.connection.close)

    def test_missing_database_is_not_initialized_and_json_is_bounded(self):
        missing = Path(self.temp.name)/'missing.db'
        with self.assertRaises(FileNotFoundError):check.verify(missing,self.before,self.after,self.run_id)
        self.assertFalse(missing.exists())
        capture = Path(self.temp.name)/'capture.json'
        for raw in [b'{"count":1,"count":2}', b' '*(check.MAX_INPUT+1)]:
            capture.write_bytes(raw)
            with self.assertRaises(ValueError):check.read_json(capture)
