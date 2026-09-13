import importlib.util
import json
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]/'scripts'
def module(name):
    spec=importlib.util.spec_from_file_location(name,ROOT/(name+'.py'))
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result
held=module('prepare-held-canary')
control=module('control-held-canary')

class HeldCanaryTests(unittest.TestCase):
    def test_fixed_unlinked_bindings_preserve_existing_canary_and_owner(self):
        rules=held.definitions()
        self.assertEqual([t['configuration']['itemName'] for t in rules[0]['triggers']],
                         ['LightningGoatsHeldCanary2Request','LightningGoatsHeldCanary2Release'])
        for source in [r['actions'][0]['configuration']['script'] for r in rules]:
            for forbidden in ['sendCommand','GoatFeeder_','Goat_Plugs_','LightningGoatsCanaryRequest']:
                self.assertNotIn(forbidden,source)
    def test_existing_item_refuses_all_provisioning_writes(self):
        calls=[]
        def request(path,method='GET',body=None,absent=False):
            calls.append(method);return {'name':'existing'}
        with patch.object(held.base,'client',return_value=request):
            with self.assertRaisesRegex(ValueError,'already exists'):
                held.prepare(Path('/unused'),True)
        self.assertEqual(calls,['GET'])
    def test_unknown_release_never_sends_a_command(self):
        calls=[]
        def request(path,method='GET',body=None):calls.append(method)
        with patch.object(control.held.base,'client',return_value=request),patch.object(control.held,'inspect',return_value='digest'),patch.object(control,'snapshot',return_value={'deliveries':[]}):
            with self.assertRaisesRegex(ValueError,'not recorded'):
                control.control(Path('/unused'),'00000000-0000-4000-8000-000000000001')
        self.assertEqual(calls,[])


check=module('check-held-canary')

class EvidenceTests(unittest.TestCase):
    def exercise(self, evidence, failure_stage=None, failure_point=None):
        calls=[]
        current={'hold':'ON','remote_enabled':'OFF','count':0,'deliveries':[],'ack':''}
        real_dump=json.dump
        real_sync=check.sync_parent
        synced=[]
        def sync(path):
            real_sync(path);synced.append(path)
        def request(path, method='GET', body=None):
            if method=='POST':
                # Identity must already be recoverable before any dispatch.
                self.assertIn(evidence,synced)
                intent=json.loads(evidence.read_text())
                self.assertEqual(intent['request_id'],body.decode())
                calls.append(body.decode())
                current['count']+=1
                current['deliveries']=[{'requestId':body.decode(),'status':'held'}]
        def dump(report,out,**kwargs):
            if report['stage']==failure_stage:
                if failure_point=='during':out.write('{"partial":')
                raise OSError('injected write failure')
            return real_dump(report,out,**kwargs)
        def snapshot(_):return json.loads(json.dumps(current))
        with patch.object(check.control.held.base,'client',return_value=request), patch.object(check.control.held,'inspect',return_value='digest'), patch.object(check.control,'snapshot',side_effect=snapshot), patch.object(check.control,'control',return_value={'count':1}), patch.object(check.time,'sleep'), patch.object(check.json,'dump',side_effect=dump), patch.object(check,'sync_parent',side_effect=sync):
            if failure_stage:
                with self.assertRaisesRegex(OSError,'injected'):check.check(Path('/unused'),evidence)
            else:check.check(Path('/unused'),evidence)
        return calls

    def test_stage_write_failure_preserves_identity_and_previous_snapshot(self):
        for stage,previous in [('held','prepared'),('released','held'),('passed','released')]:
            for point in ['before','during']:
                with self.subTest(stage=stage,point=point), tempfile.TemporaryDirectory() as directory:
                    evidence=Path(directory)/'evidence.json'
                    calls=self.exercise(evidence,stage,point)
                    intent=json.loads(evidence.read_text())
                    self.assertEqual(intent['stage'],'prepared')
                    self.assertEqual(calls,[intent['request_id']])
                    progress=json.loads(Path(str(evidence)+'.progress').read_text())
                    self.assertEqual(progress['stage'],previous)
                    self.assertEqual(progress['request_id'],intent['request_id'])

    def test_success_preserves_immutable_intent_and_final_progress(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence=Path(directory)/'evidence.json'
            calls=self.exercise(evidence)
            intent=json.loads(evidence.read_text())
            self.assertEqual(intent['stage'],'prepared')
            self.assertEqual(calls,[intent['request_id']])
            progress=json.loads(Path(str(evidence)+'.progress').read_text())
            self.assertEqual(progress['stage'],'passed')
            self.assertEqual(progress['request_id'],intent['request_id'])
            self.assertEqual(evidence.stat().st_mode & 0o777,0o600)
            self.assertEqual(Path(str(evidence)+'.progress').stat().st_mode & 0o777,0o600)

    def test_failed_pre_dispatch_directory_sync_never_dispatches(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence=Path(directory)/'evidence.json'
            with patch.object(check.control.held.base,'client') as client, patch.object(check.control.held,'inspect',return_value='digest'), patch.object(check.control,'snapshot',return_value={'hold':'ON','remote_enabled':'OFF','deliveries':[]}), patch.object(check,'sync_parent',side_effect=OSError('directory sync failed')):
                with self.assertRaisesRegex(OSError,'directory sync failed'):
                    check.check(Path('/unused'),evidence)
                client.return_value.assert_not_called()
            self.assertEqual(json.loads(evidence.read_text())['stage'],'prepared')

    def test_existing_evidence_is_preserved_without_dispatch(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence=Path(directory)/'evidence.json'
            evidence.write_text('existing interrupted evidence')
            with patch.object(check.control.held.base,'client') as client, patch.object(check.control.held,'inspect',return_value='digest'), patch.object(check.control,'snapshot',return_value={'hold':'ON','remote_enabled':'OFF','deliveries':[]}):
                with self.assertRaises(FileExistsError):check.check(Path('/unused'),evidence)
                client.return_value.assert_not_called()
            self.assertEqual(evidence.read_text(),'existing interrupted evidence')
