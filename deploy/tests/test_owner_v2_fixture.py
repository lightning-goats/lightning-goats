import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('fixture',Path(__file__).resolve().parents[1] / 'scripts/prepare-owner-v2-fixture.py')
fixture=importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class OwnerFixtureTests(unittest.TestCase):
    def test_source_fixture_has_only_unlinked_test_item_bindings_and_distinct_cache(self):
        definitions=fixture.definitions()
        self.assertEqual(len(definitions),3)
        source=definitions[0]['actions'][0]['configuration']['script']
        for forbidden in ('Goat_Plugs_', 'GoatFeedings', 'GoatFeeder_', 'earthship.feeder-owner'):
            self.assertNotIn(forbidden,source)
        for key in ('Request','Result','Ledger','Actuator','Counter'):
            self.assertIn(fixture.NAMES[key],source)
        self.assertNotEqual(fixture.NAMES['Request'],fixture.NAMES['Ledger'])
        self.assertIn("persistence.persist('jdbc')",definitions[1]['actions'][0]['configuration']['script'])

    def test_resume_unknown_uuid_never_sends_command(self):
        from unittest.mock import patch
        spec=importlib.util.spec_from_file_location('checker',Path(__file__).resolve().parents[1] / 'scripts/check-owner-v2-fixture.py')
        checker=importlib.util.module_from_spec(spec);spec.loader.exec_module(checker)
        commands=[]
        def request(path, method='GET', body=None):
            if method != 'GET': commands.append((path,method))
            return {'Counter':'0','Deliveries':'0','Ledger':'{"version":"feeder-request-ledger/v2","entries":[]}',
                    'Actuator':'OFF'}[path.split('/')[-2].removeprefix(fixture.PREFIX)]
        with patch.object(checker.fixture,'client',return_value=request), patch.object(checker,'inspect',return_value='inspected'):
            with self.assertRaisesRegex(ValueError,'existing complete receipt'):
                checker.check(Path('/unused'),True,'unknown-request-0001')
        self.assertEqual(commands,[])

    def test_changed_fixture_rule_rejected_before_any_command(self):
        from unittest.mock import patch
        spec=importlib.util.spec_from_file_location('checker',Path(__file__).resolve().parents[1] / 'scripts/check-owner-v2-fixture.py')
        checker=importlib.util.module_from_spec(spec);spec.loader.exec_module(checker)
        changed=fixture.definitions()[0]
        changed['actions'][0]['configuration']['script'] += '\nitems.getItem("outside_fixture").sendCommand("ON");'
        commands=[]
        def request(path, method='GET', body=None):
            if method != 'GET':commands.append((path,method))
            return changed
        with patch.object(checker.fixture,'client',return_value=request):
            with self.assertRaises(ValueError):checker.check(Path('/unused'),True)
        self.assertEqual(commands,[])
