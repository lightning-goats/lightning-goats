"""Preparation boundaries and actual private files; native/systemd mocked here.

The installed disposable rehearsal separately runs the real native checker.
These root fixtures prove protected-file preservation, not native or live finance.
"""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import types
import unittest
from unittest.mock import MagicMock, patch
import test_upgrade_contact_prepare as support

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
ROOT = SCRIPTS.parents[1]
with patch.dict(sys.modules, {'inactive_file_transaction': support.TX}):
    spec = importlib.util.spec_from_file_location('financial_stage_fixture', SCRIPTS/'prepare-cyberherd-financial-stage.py')
    F = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(F)

OWNER = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
LIMITS = {'member_percent':10,'max_sats_per_feed':100,'max_sats_per_24h':1000,
          'max_fee_sats':2,'max_net_sats':98,'max_new_per_hour':10}
IDENTITIES = {r:{'uid':10001+i,'gid':10001+i} for i,r in enumerate(('core','membership','payout','signer'))}
REPORT = {'accepted':True,'configuration_only':True,'candidate_limits_valid':True,
          'journals_opened':False,'credentials_loaded':False,'provider_operations':0,'services_changed':False}


def fixture():
    files={'core':'deploy/config.receive.toml.example','membership':'deploy/cyberherd/config.toml.example',
           'payout':'deploy/cyberherd/payout.toml.example','signer':'deploy/cyberherd/signer.toml.example'}
    values={role:tomllib.loads((ROOT/name).read_text()) for role,name in files.items()}
    core,member,payout,signer=(values[r] for r in files)
    core['nostr']['bunker_pubkey']=OWNER
    payees={name:OWNER for name in F.USERS}
    core['lnurl']['zap']={'receipt_pubkey':OWNER,'payee_pubkeys':payees,'alias_origins':['https://lnb.bolverker.com/']}
    core['core_bridge']={'socket':member['core_socket'],'peer_uid':10002,'group_gid':10005,
        'payout_execution_enabled':False,'delegated_feeder_messages':False,
        'reward_policy':{'owner':OWNER,'member_percent_ceiling':0,'max_sats_per_feed':0,'max_sats_per_24h':0}}
    member.update(enabled=True,project_pubkey=OWNER)
    member['core'].update(core_uid=10001,epoch='11111111-1111-4111-8111-111111111111',payees=payees)
    member['messaging']={'enabled':True,'start_after':5,'poll_millis':1000,'relays':['wss://relay.damus.io/'],'lifecycle':False}
    member['view']={'enabled':True,'listen':'127.0.0.1:3340','overlay':True,
                    'auth':{'public_origin':'https://example.com','operators':[]}}
    payout.update(intake_uid=10002,signer_uid=10004)
    payout['policy']['scope'].update(core_uid=10001,owner=OWNER,epoch=member['core']['epoch'])
    payout['policy']['scope']['provider'].update(account_id='22222222-2222-4222-8222-222222222222',credential_fingerprint='11'*32)
    payout['policy']['operation_addresses'].append('operator@wallet.example')
    signer['group_gid']=10007
    signer['policy'].update(owner=OWNER,peer_uid=10003,
        client_pubkey='c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5')
    signer['nostr']['bunker_pubkey']=OWNER
    return values


class FinancialBoundaries(unittest.TestCase):
    def test_explicit_limits_reject_partial_policy_bad_types_and_out_of_scope_caps(self):
        F.validate_limits(LIMITS);F.validate_limits({n:0 for n in F.LIMITS})
        F.validate_limits(dict(LIMITS,max_fee_sats=0))
        invalid=[{n:v for n,v in LIMITS.items() if n!='max_net_sats'},dict(LIMITS,extra=1)]
        invalid += [dict(LIMITS,**{n:v}) for n,v in [('member_percent',True),('max_fee_sats',1.5),
            ('max_sats_per_feed','100'),('member_percent',101),('max_new_per_hour',3601),
            ('max_net_sats',101),('max_fee_sats',101),('max_sats_per_24h',99),('max_net_sats',0),
            ('max_fee_sats',-1),('max_sats_per_24h',2**63)]]
        for value in invalid:
            with self.subTest(value=value),self.assertRaises(ValueError):F.validate_limits(value)

    def test_four_role_changes_retain_all_other_configuration_and_exclude_every_receive_alias(self):
        original=fixture();saved=copy.deepcopy(original);new=F.render(original,IDENTITIES,10006,LIMITS)
        self.assertEqual(original,saved)
        self.assertEqual(new['core']['lightning_address'],original['core']['lightning_address'])
        for section in ['nostr','lnurl','database','gateway','service']:
            if section in original['core']:self.assertEqual(new['core'][section],original['core'][section])
        for section in ['core','messaging','view']:self.assertEqual(new['membership'][section],original['membership'][section])
        self.assertEqual(new['payout']['policy']['scope'],original['payout']['policy']['scope'])
        self.assertEqual(new['signer']['nostr'],original['signer']['nostr'])
        self.assertEqual(new['core']['core_bridge']['payout_peer_uid'],10003)
        self.assertEqual(new['membership']['payout_bridge']['group_gid'],10006)
        expected={u+'@'+d for u in F.USERS for d in ['lightning-goats.com','lnb.bolverker.com']}|{'operator@wallet.example'}
        self.assertEqual(set(new['payout']['policy']['operation_addresses']),expected)
        self.assertEqual(new['membership']['rewards']['operation_addresses'],new['payout']['policy']['operation_addresses'])
        self.assertEqual(new['membership']['rewards']['minimum_fee_sats'],2)
        self.assertEqual(new['membership']['rewards']['maximum_net_sats'],98)
        self.assertEqual(new['payout']['policy']['max_net_sats'],98)
        self.assertEqual(new['signer']['policy']['max_net_sats'],98)
        for role in ['payout','signer']:self.assertTrue(new[role]['enabled'])
        self.assertEqual(new['core']['core_bridge']['reward_policy']['max_sats_per_feed'],100)

    def test_original_peer_epoch_payee_and_receipt_mismatch_refuse(self):
        for role,path,value in [('payout',['intake_uid'],10001),('signer',['policy','peer_uid'],10002),
                ('payout',['signer_uid'],10002),('membership',['core','epoch'],'changed'),
                ('membership',['view','listen'],'127.0.0.1:3341'),
                ('core',['lnurl','zap','receipt_pubkey'],'ab'*32),('core',['lnurl','zap','payee_pubkeys'],{}),
                ('payout',['policy','receipt_relays'],['wss://other.example'])]:
            original=fixture();target=original[role]
            for part in path[:-1]:target=target[part]
            target[path[-1]]=value
            with self.subTest(role=role,path=path),self.assertRaises(ValueError):F.render(original,IDENTITIES,10006,LIMITS)

    def test_direct_execution_refuses_before_substituted_helper_import(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);shutil.copyfile(SCRIPTS/'prepare-cyberherd-financial-stage.py',root/'prepare.py')
            (root/'maintain-contact-upgrade.py').write_text('raise RuntimeError("INJECTED")')
            r=subprocess.run(['/usr/bin/python3','-I','-S','-B',str(root/'prepare.py'),'--help'],capture_output=True,text=True,timeout=10)
            self.assertNotEqual(r.returncode,0);self.assertIn('externally verified root-owned tool launcher',r.stderr)
            self.assertNotIn('INJECTED',r.stderr)

    def test_native_receipt_requires_all_positive_fields_and_exact_types(self):
        self.assertEqual(F.check_report(json.dumps(REPORT)),REPORT)
        for value in [dict(REPORT,accepted=False),dict(REPORT,journals_opened=0),dict(REPORT,services_changed=True),
                      dict(REPORT,extra=True),{n:v for n,v in REPORT.items() if n!='candidate_limits_valid'}]:
            with self.assertRaises(ValueError):F.check_report(json.dumps(value))

    def test_projection_requires_fresh_stored_allocation_and_fixed_loopback_route(self):
        opener=MagicMock();response=opener.open.return_value.__enter__.return_value;response.status=200
        response.headers={'Content-Type':'application/json'}
        value={'version':1,'generated_at':100,'roster':{'owner':OWNER,'member_percent':10}}
        with patch.object(F.urllib.request,'build_opener',return_value=opener),patch.object(F.time,'time',return_value=100):
            response.read.return_value=json.dumps(value).encode();self.assertEqual(F.observed_allocation(OWNER,10)['member_percent'],10)
            self.assertEqual(opener.open.call_args.args[0].full_url,'http://127.0.0.1:3340/api/v1/cyberherd')
            for changed in [dict(value,generated_at=69),dict(value,generated_at=131),dict(value,version=2),
                    dict(value,roster={'owner':OWNER,'member_percent':0}),dict(value,roster={'owner':OWNER,'member_percent':True})]:
                response.read.return_value=json.dumps(changed).encode()
                with self.assertRaises(ValueError):F.observed_allocation(OWNER,10)


@unittest.skipUnless(os.geteuid()==0,'protected financial-candidate file cases require separate root CI')
class FinancialFiles(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='lg-financial-files-',dir='/run');self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name);self.base=self.root/'upgrades';self.base.mkdir(mode=0o700)
        self.etc=self.root/'etc';self.etc.mkdir();self.config=self.etc/'core.toml'
        self.original=fixture();self.paths={'core':self.config}
        for role in ['membership','payout','signer']:
            parent=self.etc/F.U.P.ROLE_NAMES[role];parent.mkdir();self.paths[role]=parent/'config.toml'
        for role,path in self.paths.items():self.write(path,F.S.W.toml_text(self.original[role]).encode(),0o600)
        self.before={str(p):F.TX.fingerprint(p) for p in self.paths.values()}
        self.baseline={'identities':IDENTITIES,'files':self.before,'fixture_generation':0}
        self.validator=self.root/'native-checker';self.write(self.validator,b'NEVER EXECUTE MOCK CHECKER',0o755)
        self.digest=hashlib.sha256(self.validator.read_bytes()).hexdigest();self.destination=self.base/'candidate'
        self.carriers=set();self.calls=[];self.refuse=False;self.drift=False;self.candidate_drift=False
        self.addCleanup(self.cleanup)
        for obj,name,value in [(F.C,'BASE',self.base),(F.C,'CONFIG',self.config),(F.Q,'ETC',self.etc),
            (F.C,'root',lambda:None),(F.C.grp,'getgrnam',lambda n:types.SimpleNamespace(gr_gid=10006)),
            (F.U.P,'baseline',lambda **kw:copy.deepcopy(self.baseline)),(F.C,'command',self.command),
            (F,'observed_allocation',lambda owner,percent:{'owner':owner,'member_percent':percent,'generated_at':100}),
            (F,'verified_tool_identity',lambda:{'source_commit':'a'*40,'manifest_sha256':'b'*64})]:
            context=patch.object(obj,name,value);context.start();self.addCleanup(context.stop)

    @staticmethod
    def write(path,data,mode):path.write_bytes(data);path.chmod(mode)

    def cleanup(self):
        for path in self.carriers:shutil.rmtree(path)

    def command(self,*args,**kwargs):
        self.calls.append(args);self.assertEqual(args[:3],('/usr/bin/unshare','--net','--'))
        self.assertIn('--reuid=10001',args);self.assertIn('--regid=10001',args);self.assertIn('--bounding-set=-all',args)
        for path in map(Path,args[-4:]):
            self.carriers.add(path.parent);self.assertEqual(path.stat().st_mode&0o777,0o440);self.assertEqual(path.stat().st_gid,10001)
            self.assertIn('core_bridge' if path.name=='core.toml' else 'enabled',tomllib.loads(path.read_text()))
        if self.drift:self.baseline['fixture_generation']=1
        if self.candidate_drift:(self.destination/'candidate-payout.toml').write_bytes(b'substituted after checking')
        return json.dumps(dict(REPORT,accepted=not self.refuse))

    def unchanged(self):
        for p,fp in self.before.items():self.assertEqual(F.TX.fingerprint(Path(p)),fp)

    def test_candidate_only_preserves_originals_and_records_separate_reviews(self):
        result=F.prepare(self.destination,self.validator,self.digest,LIMITS)
        self.assertTrue(result['prepared']);self.assertFalse(result['activation_authorized']);self.assertTrue(result['backlog_review_required'])
        record=json.loads((self.destination/'PREPARED.json').read_text())
        self.assertEqual(len(record['configurations']),4);self.assertTrue(record['provider_qualification_required'])
        self.assertFalse((self.destination/'INCOMPLETE').exists());self.assertEqual(len(self.calls),1);self.unchanged()
        for role in self.paths:
            self.assertEqual(tomllib.loads((self.destination/('original-'+role+'.toml')).read_text()),self.original[role])
        with self.assertRaises(FileExistsError):F.prepare(self.destination,self.validator,self.digest,LIMITS)

    def test_native_refusal_and_baseline_drift_retain_incomplete_without_live_changes(self):
        for name in ['refuse','drift','candidate_drift']:
            with self.subTest(name=name):
                self.destination=self.base/name;setattr(self,name,True)
                with self.assertRaises(ValueError):F.prepare(self.destination,self.validator,self.digest,LIMITS)
                setattr(self,name,False);self.baseline['fixture_generation']=0
                self.assertTrue((self.destination/'INCOMPLETE').exists());self.assertFalse((self.destination/'PREPARED.json').exists());self.unchanged()

    def test_untrusted_checker_or_allocation_mismatch_refuses_before_creation(self):
        with self.assertRaises(ValueError):F.prepare(self.destination,self.validator,'0'*64,LIMITS)
        self.assertFalse(self.destination.exists());self.assertEqual(self.calls,[])
        with patch.object(F,'observed_allocation',side_effect=ValueError('stored allocation mismatch')):
            with self.assertRaisesRegex(ValueError,'stored allocation'):F.prepare(self.destination,self.validator,self.digest,LIMITS)
        self.assertFalse(self.destination.exists());self.unchanged()
