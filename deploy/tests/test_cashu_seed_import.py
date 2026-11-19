"""Dedicated seed preparation: public BIP39 vectors and real terminal/file boundaries."""
import errno
import importlib.util
import os
from pathlib import Path
import pty
import select
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/add-cashu-seed.py'
SPEC = importlib.util.spec_from_file_location('cashu_seed_import', SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
# Public vector: github.com/trezor/python-mnemonic/blob/master/vectors.json
PHRASE = 'abandon ' * 11 + 'about'
EXPECTED = 'c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04'


class SeedImportTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.output = self.root / 'private/wallet-seed.bin'

    def test_public_bip39_vector_and_passphrase_normalization(self):
        self.assertEqual(MODULE.derive_seed(PHRASE, 'TREZOR').hex(), EXPECTED)
        self.assertEqual(MODULE.derive_seed(PHRASE, 'é'), MODULE.derive_seed(PHRASE, 'e\u0301'))
        self.assertNotEqual(MODULE.derive_seed(PHRASE), MODULE.derive_seed(PHRASE, 'TREZOR'))

    def test_invalid_words_count_and_checksum_never_echo_input(self):
        for value in ['abandon ' * 12, 'abandon ' * 11, 'abandon ' * 11 + 'private-invalid-marker']:
            with self.assertRaises(ValueError) as raised:
                MODULE.derive_seed(value)
            self.assertNotIn('private-invalid-marker', str(raised.exception))

    def test_private_raw_seed_and_refusal_to_replace(self):
        seed = MODULE.derive_seed(PHRASE)
        MODULE.write_seed(self.output, seed)
        self.assertEqual(self.output.read_bytes(), seed)
        self.assertEqual(stat.S_IMODE(self.output.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(self.output.parent.stat().st_mode), 0o700)
        self.assertEqual(self.output.stat().st_nlink, 1)
        with self.assertRaises(FileExistsError):
            MODULE.write_seed(self.output, b'X' * 64)
        self.assertEqual(self.output.read_bytes(), seed)
        self.assertEqual(list(self.output.parent.iterdir()), [self.output])

    def test_existing_symlink_and_symlink_ancestor_refuse(self):
        self.output.parent.mkdir(mode=0o700)
        target = self.root / 'untouched'
        target.write_bytes(b'original')
        self.output.symlink_to(target)
        with self.assertRaises(FileExistsError):
            MODULE.write_seed(self.output, b'X' * 64)
        self.assertEqual(target.read_bytes(), b'original')
        alias = self.root / 'alias'
        alias.symlink_to(self.output.parent, target_is_directory=True)
        with self.assertRaises(OSError):
            MODULE.write_seed(alias / 'other', b'X' * 64)

    def test_nonprivate_directory_and_bad_length_refuse(self):
        self.output.parent.mkdir(mode=0o755)
        with self.assertRaises(ValueError):
            MODULE.write_seed(self.output, b'X' * 64)
        with self.assertRaises(ValueError):
            MODULE.write_seed(self.root / 'bad-length', b'X' * 32)
        self.assertFalse(self.output.exists())

    def test_write_failure_publishes_no_partial_seed(self):
        with patch.object(MODULE.os, 'fsync', side_effect=OSError('synthetic disk failure')):
            with self.assertRaises(OSError):
                MODULE.write_seed(self.output, b'X' * 64)
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.output.parent.iterdir()), [])

    def test_redirected_input_and_accidental_arguments_are_redacted(self):
        for extra in [[], ['private-argument-marker']]:
            result = subprocess.run([sys.executable, str(SCRIPT), '--output', str(self.output), *extra],
                                    input=PHRASE, text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(PHRASE, result.stdout + result.stderr)
            self.assertNotIn('private-argument-marker', result.stdout + result.stderr)
        self.assertFalse(self.output.exists())

    @unittest.skipIf(os.geteuid() == 0, 'interactive script deliberately refuses root')
    def test_real_terminal_hides_both_entries_and_writes_expected_bytes(self):
        pid, master = pty.fork()
        if pid == 0:
            os.execv(sys.executable, [sys.executable, str(SCRIPT), '--output', str(self.output)])
        transcript = b''
        prompts = [b'12 seed words: ', b'Enter the same 12 words again: ']
        next_prompt = 0
        deadline = time.monotonic() + 10
        try:
            while time.monotonic() < deadline:
                ready, _, _ = select.select([master], [], [], 0.1)
                if not ready:
                    continue
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not chunk:
                    break
                transcript += chunk
                if next_prompt < len(prompts) and prompts[next_prompt] in transcript:
                    os.write(master, PHRASE.encode() + b'\n')
                    next_prompt += 1
            else:
                os.kill(pid, 9)
                self.fail('terminal import timed out')
            _, status = os.waitpid(pid, 0)
        finally:
            os.close(master)
        self.assertEqual(os.waitstatus_to_exitcode(status), 0, transcript.decode(errors='replace'))
        self.assertEqual(next_prompt, 2)
        self.assertNotIn(PHRASE.encode(), transcript)
        self.assertEqual(self.output.read_bytes(), MODULE.derive_seed(PHRASE))


if __name__ == '__main__':
    unittest.main()
