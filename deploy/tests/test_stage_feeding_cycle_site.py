import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "stage_feeding_cycle_site", ROOT / "deploy/scripts/stage-feeding-cycle-site.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class StageFeedingCycleSiteTests(unittest.TestCase):
    def test_stage_preserves_live_payment_assets_and_qr(self):
        with TemporaryDirectory() as scratch:
            live = Path(scratch) / "live"
            live.mkdir()
            files = {
                "index.html": '<script src="/homepage-payments-abcdef012345.js"></script>'
                              '<script src="/contact-fee-abcdef012346.js"></script>',
                "overlay.html": '<a id="qr-code" href="lightning:LNURL1TEST"></a>',
                "homepage-payments-abcdef012345.js": "live home payment",
                "contact-fee-abcdef012346.js": "live contact payment",
            }
            for name, content in files.items():
                (live / name).write_text(content)
            output = Path(scratch) / "staged"
            MODULE.stage(ROOT / "web", live, output)

            index = (output / "index.html").read_text()
            overlay = (output / "overlay.html").read_text()
            self.assertIn("/homepage-payments-abcdef012345.js", index)
            self.assertIn("/contact-fee-abcdef012346.js", index)
            self.assertIn('href="lightning:LNURL1TEST"', overlay)
            for prefix, document in (("feeding-checkout", index), ("overlay", overlay)):
                assets = list(output.glob(f"{prefix}-*"))
                self.assertEqual(len(assets), 1 if prefix == "feeding-checkout" else 2)
                for asset in assets:
                    self.assertIn(f'"/{asset.name}"', document)
            self.assertEqual({name: (live / name).read_text() for name in files}, files)
