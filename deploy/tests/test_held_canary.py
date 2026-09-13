import importlib.util
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
