"""Check the inactive Minibits rollout configuration and cross-role bindings."""
import json
from pathlib import Path
import runpy
import tomllib
import unittest
import uuid


SOURCE = Path(__file__).resolve().parents[1]
TOOL = runpy.run_path(str(SOURCE / 'scripts/render-cashu-pilot.py'))
ONE = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
TWO = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
THREE = '2f8bde4d1a07209355b4a7250a5c5128e88b84bddc619ab7cba8d569b240efe4'


class PilotConfig(unittest.TestCase):
    def setUp(self):
        template = (SOURCE / 'config.canary.toml.example').read_text()
        self.core = template.replace('0' * 64, ONE).replace(
            '# bunker_relays = ["wss://signer-relay.example"]',
            'bunker_relays = ["wss://signer-relay.example"]',
        ).encode()
        self.input = dict(zip(TOOL['EPOCHS'], [str(uuid.UUID(int=n)) for n in range(1, 6)], strict=True))
        self.input.update(version=1, core_uid=1010, wallet_uid=1020, wallet_gid=1021,
                          signer_uid=1030, signer_gid=1031, receiving_key='02' + TWO,
                          client_pubkey=THREE, helper_sha256='ab' * 32,
                          strike_account_id=str(uuid.UUID(int=6)), strike_credential_fingerprint='ac' * 32,
                          daily_credit_sats=12345, minimum_deposit_sats=1, relay_since_unix=1_800_000_000,
                          mint_keysets=[{'id': '00deadbeef123456', 'keys': {'1': '02' + ONE}}])

    def test_render_preserves_core_and_matches_all_role_bindings(self):
        files = TOOL['render'](self.core, self.input)
        self.assertEqual(set(files), {'core.toml', 'wallet.json', 'signer.json'})
        self.assertTrue(files['core.toml'].startswith(self.core))
        core = tomllib.loads(files['core.toml'].decode())
        wallet, signer = (json.loads(files[name]) for name in ('wallet.json', 'signer.json'))
        intake, receipts, treasury = (core.pop(key) for key in ('cashu', 'cashu_receipts', 'cashu_treasury'))
        self.assertEqual(core, tomllib.loads(self.core.decode()))
        for role in (intake, receipts, signer['policy']):
            self.assertEqual(role['wallet_uid'], wallet['runtime_uid'])
            self.assertEqual(role['wallet_instance'], wallet['wallet_instance'])
            self.assertEqual(role['wallet_epoch'], wallet['wallet_epoch'])
        self.assertEqual(intake['socket'], wallet['intake_socket'])
        self.assertEqual(receipts['socket'], wallet['receipt_delivery']['socket'])
        self.assertEqual(treasury['socket'], wallet['treasury_intake']['socket'])
        self.assertEqual(signer['socket'], wallet['publication_signing']['socket'])
        self.assertEqual(signer['runtime_uid'], wallet['publication_signing']['signer_uid'])
        self.assertEqual(receipts['core_epoch'], wallet['treasury_intake']['core_epoch'])
        self.assertEqual(treasury['policy']['destination'], wallet['treasury_intake']['destination'])
        self.assertEqual(signer['policy']['receiving_key'], wallet['receiving_key'][2:])
        self.assertEqual(signer['nak']['relays'], ['wss://signer-relay.example'])
        self.assertEqual(wallet['relay_intake']['relays'], ['wss://relay.example'])
        self.assertEqual(wallet['mints'][0]['advertised_url'], 'https://mint.minibits.cash/Bitcoin')
        self.assertEqual(wallet['keysets'][0]['mint_id'], 'minibits')
        self.assertEqual(receipts['maximum_custody_sats'], 999)
        self.assertEqual(receipts['credit_sats_per_24h'], self.input['daily_credit_sats'])
        self.assertTrue(receipts['unlimited_receipts_per_24h'])
        for limits in (treasury['policy']['limits'], wallet['treasury_execution']):
            self.assertEqual(limits['maximum_fee_sats'], 5)
            self.assertEqual(limits['fee_sats_per_24h'], 25)
            self.assertTrue(limits['unlimited_operations_per_24h'])
            self.assertTrue(limits['unlimited_input_sats_per_24h'])
        self.assertEqual(treasury['controls']['sweep']['maximum_holding_age_seconds'], 1800)
        self.assertFalse(any((
            intake['receive_cashu'], intake['receive_nutzap'], receipts['import_enabled'],
            treasury['controls']['plan'], treasury['controls']['issue'], treasury['controls']['deliver'],
            treasury['controls']['inspect'], treasury['controls']['sweep']['enabled'], treasury['policy']['enabled'],
            wallet['receive_cashu'], wallet['receive_nutzap'], wallet['receipt_delivery']['enabled'],
            wallet['relay_intake']['enabled'], wallet['treasury_intake']['enabled'],
            wallet['treasury_intake']['accept_new'], wallet['treasury_execution']['execute'],
        )))

    def test_reviewed_nutzap_relays_bind_wallet_signer_and_publication_only(self):
        base = self.core.replace(b'relays = ["wss://relay.example"]',
                                 b'relays = ["wss://relay.example", "wss://other.example"]')
        approved = ['wss://relay.example:443/']
        files = TOOL['render'](base, dict(self.input, nutzap_relays=approved))
        wallet, signer = (json.loads(files[name]) for name in ('wallet.json', 'signer.json'))
        self.assertTrue(files['core.toml'].startswith(base))
        self.assertEqual(wallet['relay_intake']['relays'], approved)
        self.assertEqual(signer['policy']['relays'], approved)
        self.assertEqual(signer['nak']['relays'], ['wss://signer-relay.example'])
        self.assertEqual(wallet['publication_delivery']['base_relays'], approved)
        self.assertEqual(wallet['publication_delivery']['discovery_relays'], approved)
        self.assertFalse(wallet['publication_delivery']['announcements'])
        self.assertFalse(wallet['publication_delivery']['redemptions'])
        fallback = TOOL['render'](base, self.input)
        self.assertEqual(files['core.toml'], fallback['core.toml'])
        self.assertNotIn('publication_delivery', json.loads(fallback['wallet.json']))

    def test_nutzap_subset_refuses_unknown_or_canonical_duplicate_relays(self):
        for selected in ([], ['wss://unreviewed.example'], ['wss://relay.example/', 'wss://relay.example:443'],
                         ['wss://relay.example/?credential=example'], ['wss://relay.example/?'],
                         ['wss://relay.example/other']):
            with self.subTest(selected=selected), self.assertRaises(ValueError):
                TOOL['render'](self.core, dict(self.input, nutzap_relays=selected))

    def test_rejects_misbound_keys_identities_and_existing_cashu(self):
        for key, value in (
            ('signer_uid', self.input['wallet_uid']), ('client_pubkey', TWO),
            ('wallet_epoch', self.input['core_epoch']), ('daily_credit_sats', 0),
            ('minimum_deposit_sats', 999), ('mint_keysets', []), ('relay_since_unix', 0),
        ):
            with self.subTest(key=key), self.assertRaises(ValueError):
                TOOL['render'](self.core, dict(self.input, **{key: value}))
        with self.assertRaises(ValueError):
            TOOL['render'](self.core + b'\n[cashu]\nreceive_cashu = false\n', self.input)

    def test_rolling_credit_budget_can_import_largest_allowed_receipt(self):
        original = TOOL['render'](self.core, self.input)
        maximum_receipt = json.loads(original['wallet.json'])['receipt_sats']
        with self.assertRaisesRegex(ValueError, 'rolling credit budget'):
            TOOL['render'](self.core, dict(self.input, daily_credit_sats=maximum_receipt - 1))
        at_limit = TOOL['render'](self.core, dict(self.input, daily_credit_sats=maximum_receipt))
        receipts = tomllib.loads(at_limit['core.toml'].decode())['cashu_receipts']
        self.assertEqual(receipts['credit_sats_per_24h'], maximum_receipt)
        self.assertTrue(receipts['unlimited_receipts_per_24h'])

    def test_request_selection_keeps_receiving_disabled(self):
        endpoint = 'https://goats.example/api/v1/cashu'
        for encoding, request in (('creqA', 'creqAAb_CD'), ('creqB', 'CREQB1QPZR')):
            preview = dict(version=1, id='lg1-' + 'ab' * 16, request=request, encoding=encoding,
                           status='prepared', service_started=False, descriptor_registered=False)
            with self.subTest(encoding=encoding):
                files = TOOL['render'](self.core, self.input, (endpoint, preview))
                wallet = json.loads(files['wallet.json'])
                self.assertEqual(wallet['descriptors'], [preview['id']])
                self.assertEqual(wallet['public_request'], {'endpoint': endpoint, 'encoding': encoding})
                self.assertFalse(wallet['receive_cashu'])
                self.assertFalse(wallet['receive_nutzap'])
                self.assertFalse(tomllib.loads(files['core.toml'].decode())['cashu']['receive_cashu'])
                self.assertNotIn(request.encode(), files['wallet.json'])

    def test_request_selection_refuses_invalid_or_live_previews(self):
        endpoint = 'https://goats.example/api/v1/cashu'
        preview = dict(version=1, id='lg1-' + 'ab' * 16, request='creqAAb_CD', encoding='creqA',
                       status='prepared', service_started=False, descriptor_registered=False)
        for changes in ({'service_started': True}, {'descriptor_registered': True},
                        {'id': 'invalid'}, {'request': 'cashuAAb_CD'},
                        {'encoding': 'creqB'}, {'version': True}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                TOOL['render'](self.core, self.input, (endpoint, dict(preview, **changes)))
        for invalid in ('http://goats.example', 'https://user@goats.example',
                        endpoint + '?query=1', endpoint + '#fragment'):
            with self.subTest(endpoint=invalid), self.assertRaises(ValueError):
                TOOL['render'](self.core, self.input, (invalid, preview))
