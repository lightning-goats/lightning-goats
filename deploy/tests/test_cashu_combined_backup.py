"""Cross-inventory identity checks; native/root recovery qualification is separate."""
import copy
import hashlib
import os
from pathlib import Path
import runpy
import stat
import tempfile
import unittest
import uuid

MODULE = runpy.run_path(str(Path(__file__).resolve().parents[1]
                           / 'scripts/verify-cashu-combined-backup.py'))


class CombinedBackupTests(unittest.TestCase):
    def setUp(self):
        checkpoint, epoch = str(uuid.uuid4()), str(uuid.uuid4())
        self.envelope = {'checkpoint_id': checkpoint, 'cashu_inventory_sha256': 'ab' * 32,
                         'maintenance_policy_sha256': 'cd' * 32}
        self.inventory = {'checkpoint_id': checkpoint, 'registrations': {'core_epoch': epoch},
                          'components': [{'role': 'core_ledger', 'sha256': 'ef' * 32}]}
        self.native = {'id': checkpoint, 'version': 2,
                       'purpose': 'cashu_four_journal_correlation_review',
                       'cashu_inventory_sha256': 'ab' * 32, 'source_policy_sha256': 'cd' * 32,
                       'policy': {'epoch': epoch}, 'images': [{'name': 'core.db', 'sha256': 'ef' * 32}]}

    def test_matching_checkpoint_and_identical_core_bind_two_reviews(self):
        self.assertEqual(MODULE['validate_links'](self.envelope, self.inventory, self.native), 'ef' * 32)

    def test_separately_valid_reviews_cannot_be_spliced_across_checkpoints_or_core_images(self):
        for case in range(8):
            native = copy.deepcopy(self.native)
            with self.subTest(case=case):
                if case == 0:
                    native['id'] = str(uuid.uuid4())
                elif case == 1:
                    native['cashu_inventory_sha256'] = 'aa' * 32
                elif case == 2:
                    native['source_policy_sha256'] = 'aa' * 32
                elif case == 3:
                    native['policy']['epoch'] = str(uuid.uuid4())
                elif case == 4:
                    native['images'][0]['sha256'] = 'aa' * 32
                elif case == 5:
                    native['images'].append(copy.deepcopy(native['images'][0]))
                elif case == 6:
                    native['purpose'] = 'ordinary_restore'
                else:
                    native['version'] = 1
                with self.assertRaises(ValueError):
                    MODULE['validate_links'](self.envelope, self.inventory, native)

    def test_selected_executable_is_frozen_private_and_requires_exact_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, frozen = root / 'source', root / 'frozen'
            data = b'not executed; fixture executable bytes'
            source.write_bytes(data)
            MODULE['freeze_executable'](source, frozen, hashlib.sha256(data).hexdigest())
            self.assertEqual(frozen.read_bytes(), data)
            self.assertEqual(stat.S_IMODE(frozen.stat().st_mode), 0o500)
            with self.assertRaises(ValueError):
                MODULE['freeze_executable'](source, root / 'wrong', 'aa' * 32)
            with self.assertRaises(FileExistsError):
                MODULE['freeze_executable'](source, frozen, hashlib.sha256(data).hexdigest())

    def test_executable_alias_and_special_file_cannot_become_root_program(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / 'target'
            target.write_bytes(b'fixture')
            alias = root / 'alias'
            alias.symlink_to(target)
            with self.assertRaises(OSError):
                MODULE['freeze_executable'](alias, root / 'copy', 'aa' * 32)
            fifo = root / 'fifo'
            os.mkfifo(fifo, 0o600)
            with self.assertRaises(ValueError):
                MODULE['freeze_executable'](fifo, root / 'copy', 'aa' * 32)

    def test_unprivileged_verifier_has_no_file_or_execution_authority(self):
        if os.geteuid() == 0:
            self.skipTest('negative unprivileged authority check')
        with self.assertRaisesRegex(ValueError, 'local root operator'):
            MODULE['verify'](Path('/nonexistent'), Path('/nonexistent'), 'aa' * 32)


if __name__ == '__main__':
    unittest.main()
