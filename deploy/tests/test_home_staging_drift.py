import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('drift', Path(__file__).resolve().parents[1] / 'scripts/inspect-home-staging-drift.py')
drift = importlib.util.module_from_spec(spec)
spec.loader.exec_module(drift)


class DriftTests(unittest.TestCase):
    def test_packet_counters_change_without_hiding_policy_changes(self):
        before = {'nftables': [{'metainfo': {'generation': 1}}, {'rule': {'handle': 3,
            'expr': [{'counter': {'packets': 1, 'bytes': 64}}, {'drop': None}]}}]}
        after = copy.deepcopy(before)
        after['nftables'][1]['rule']['expr'][0]['counter']['packets'] = 50
        self.assertEqual(drift.stable_nft(before), drift.stable_nft(after))
        after['nftables'][1]['rule']['expr'][1] = {'accept': None}
        self.assertNotEqual(drift.stable_nft(before), drift.stable_nft(after))
        after = copy.deepcopy(before)
        after['nftables'][1]['rule']['handle'] = 4
        self.assertNotEqual(drift.stable_nft(before), drift.stable_nft(after))

    def test_route_lifetime_ticks_but_route_changes_and_removal_are_detected(self):
        before = [{'dst': '2001:db8::/64', 'gateway': 'fe80::1', 'expires': 1800}]
        after = [dict(before[0], expires=1799)]
        self.assertEqual(drift.stable_routes(before), drift.stable_routes(after))
        self.assertNotEqual(drift.stable_routes(before), drift.stable_routes([]))
        self.assertNotEqual(drift.stable_routes(before), drift.stable_routes([dict(after[0], gateway='fe80::2')]))
