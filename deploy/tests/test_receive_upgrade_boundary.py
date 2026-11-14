"""The old receive-only helper cannot apply a CyberHerd financial archive."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/upgrade-strike-receive.py"
SPEC = importlib.util.spec_from_file_location("receive_upgrade_boundary", SCRIPT)
UPGRADE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UPGRADE)


class ReceiveUpgradeBoundary(unittest.TestCase):
    def test_cyberherd_schema_is_refused_without_any_service_command(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "migrations").mkdir()
            (root / "migrations/0013_feeding_presentation.sql").write_text("legacy")
            with patch.object(UPGRADE, "run", side_effect=AssertionError("service command reached")):
                UPGRADE.require_legacy_upgrade_payload(root)
                for name in ["0016_reward_funding.sql", "0023_failed_payout_costs.sql"]:
                    path = root / "migrations" / name
                    path.write_text("new financial schema")
                    with self.assertRaisesRegex(ValueError, "no service changed"):
                        UPGRADE.require_legacy_upgrade_payload(root)
                    path.unlink()
