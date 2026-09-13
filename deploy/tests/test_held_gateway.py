import importlib.util
from pathlib import Path
import unittest
import tomllib
spec=importlib.util.spec_from_file_location('held_install',Path(__file__).resolve().parents[1]/'scripts/prepare-held-gateway.py')
held=importlib.util.module_from_spec(spec);spec.loader.exec_module(held)
class HeldGatewayTests(unittest.TestCase):
    def test_targets_and_identity_do_not_reuse_original_canary_state_or_binary(self):
        files=held.files(b'fixture')
        self.assertEqual(len(files),3)
        self.assertTrue(all(held.STEM in path for path in files))
        config=tomllib.loads(files[f'/etc/{held.STEM}/config.toml'][0].decode())
        self.assertEqual(config['service']['listen'],'127.0.0.1:8791')
        self.assertEqual(config['openhab']['protocol'],'uuid_held_canary')
        self.assertNotIn('temperature_item',config['openhab'])
        unit=files[f'/etc/systemd/system/{held.STEM}.service'][0].decode()
        self.assertIn('User='+held.STEM+'\n',unit)
        self.assertIn('StateDirectory='+held.STEM+'\n',unit)
        self.assertNotIn('IPAddressAllow=10.8.',unit)
        self.assertIn('lightning-goats-gateway-canary-openhab',unit)
