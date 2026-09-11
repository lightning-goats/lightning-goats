"""Prevent a rehearsal from silently weakening or omitting shipped directives."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("systemd_rehearsal", Path(__file__).resolve().parents[1] / "scripts/rehearse-systemd.py")
REHEARSAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REHEARSAL)


class SystemdRehearsalTests(unittest.TestCase):
    def test_repeated_address_and_credential_properties_are_preserved(self):
        gateway = REHEARSAL.service_properties(REHEARSAL.TEMPLATES / "lightning-goats-gateway-canary.service")
        self.assertEqual([v for k,v in gateway if k == "IPAddressAllow"], ["localhost","10.8.0.0/24"])
        self.assertIn(("CapabilityBoundingSet",""), gateway)
        self.assertIn(("ProtectSystem","strict"), gateway)
        daemon = REHEARSAL.service_properties(REHEARSAL.TEMPLATES / "lightning-goats-canary.service")
        self.assertEqual([v.split(":",1)[0] for k,v in daemon if k == "LoadCredentialEncrypted"], ["strike-api-key","strike-webhook-secret"])

    def test_unsupported_continuation_does_not_drop_a_sandbox_constraint(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.service"
            path.write_text("[Service]\nRestrictAddressFamilies=AF_UNIX \\\n AF_INET\n")
            with self.assertRaisesRegex(ValueError,"unsupported"):
                REHEARSAL.service_properties(path)

    def test_install_directives_are_not_started_or_enabled(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.service"
            path.write_text("[Unit]\nWants=production.service\n[Service]\nNoNewPrivileges=yes\n[Install]\nWantedBy=multi-user.target\n")
            self.assertEqual(REHEARSAL.service_properties(path), [("NoNewPrivileges","yes")])

    def test_absent_other_service_does_not_prove_credential_isolation(self):
        launcher = REHEARSAL.SystemdLauncher.__new__(REHEARSAL.SystemdLauncher)
        launcher.starts = [
            {"role":"gateway", "other_service_active_during_credential_probe":False},
            {"role":"daemon", "other_service_active_during_credential_probe":True},
        ]
        with self.assertRaisesRegex(ValueError,"both directions"):
            launcher.evidence()
