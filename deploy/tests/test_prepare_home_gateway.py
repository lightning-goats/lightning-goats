import importlib.util
from pathlib import Path
import tomllib
import unittest

spec = importlib.util.spec_from_file_location('home', Path(__file__).resolve().parents[1] / 'scripts/prepare-home-gateway.py')
home = importlib.util.module_from_spec(spec)
spec.loader.exec_module(home)

class HomePlanTests(unittest.TestCase):
    def test_canary_cannot_bind_physical_items_or_share_identity(self):
        files = home.generated_files(b'fixture')
        c = tomllib.loads(files['/etc/lightning-goats-gateway-canary/config.toml'][0].decode())
        self.assertEqual(c['service']['listen'], '127.0.0.1:8790')
        self.assertEqual(c['openhab']['protocol'], 'uuid_canary')
        for key in ('request_item', 'ack_item', 'override_item', 'remote_enabled_item'):
            self.assertTrue(c['openhab'][key].startswith('LightningGoatsCanary'))
        self.assertNotIn('temperature_item', c['openhab'])
        unit = files['/etc/systemd/system/lightning-goats-gateway-canary.service'][0].decode()
        self.assertIn('User=lightning-goats-gateway-canary\n', unit)
        self.assertIn('openhab-token:/etc/credstore.encrypted/lightning-goats-gateway-canary-openhab', unit)
        self.assertNotIn('IPAddressAllow=10.8.', unit)
        self.assertIn('CapabilityBoundingSet=\n', unit)

    def test_production_preserves_owner_protocol_and_caps(self):
        files = home.generated_files(b'fixture')
        c = tomllib.loads(files['/etc/lightning-goats-gateway/config.toml'][0].decode())
        self.assertEqual(c['openhab']['protocol'], 'feeder_request_v1')
        self.assertEqual(c['openhab']['override_item'], 'FeederOverride')
        self.assertEqual(c['feeder']['min_feed_interval_seconds'], 30)
        self.assertEqual(c['feeder']['max_feeds_per_hour'], 10)
        self.assertEqual(c['service']['listen'], '127.0.0.1:8789')

class CanaryReadbackTests(unittest.TestCase):
    def test_openhab_added_empty_inputs_is_harmless_but_extra_behavior_is_rejected(self):
        import copy
        spec = importlib.util.spec_from_file_location('canary', Path(__file__).resolve().parents[1] / 'scripts/prepare-home-openhab-canary.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        expected = module.rule_definition()
        actual = copy.deepcopy(expected)
        actual['actions'][0]['inputs'] = {}
        module.verify_rule(actual, expected)
        actual['actions'][0]['inputs'] = {'other': 'rule-output'}
        with self.assertRaises(ValueError):
            module.verify_rule(actual, expected)

    def test_configuration_directory_remains_root_managed(self):
        for name, (data, mode) in home.generated_files(b'fixture').items():
            if name.endswith('.service'):
                self.assertNotIn('ConfigurationDirectory=', data.decode())
