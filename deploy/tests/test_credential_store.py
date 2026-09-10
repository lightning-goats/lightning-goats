"""The staging bootstrap must never replace a key or bypass recovery evidence."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("credential_store", Path(__file__).resolve().parents[1] / "scripts/initialize-staging-credential-store.py")
STORE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STORE)


class CredentialStoreTests(unittest.TestCase):
    def test_existing_key_is_inspected_not_replaced(self):
        with patch.object(STORE.os,"geteuid",return_value=0), patch.object(STORE.Path,"exists",return_value=True), patch.object(STORE,"inspect_key",return_value={"mode":"0o400"}), patch.object(STORE.subprocess,"run") as run:
            self.assertEqual(STORE.initialize(),{"created":False,"mode":"0o400"})
            run.assert_not_called()

    def test_missing_key_with_existing_ciphertext_requires_recovery(self):
        with patch.object(STORE.os,"geteuid",return_value=0), patch.object(STORE.Path,"exists",side_effect=[False,True]), patch.object(STORE.Path,"is_symlink",return_value=False), patch.object(STORE.Path,"iterdir",return_value=iter([Path("saved-ciphertext")])), patch.object(STORE.subprocess,"run") as run:
            with self.assertRaisesRegex(ValueError,"recover the original key"):
                STORE.initialize()
            run.assert_not_called()

    def test_insecure_or_non_regular_key_is_not_accepted(self):
        for mode,uid in [(0o100644,0),(0o120400,0),(0o100400,1000)]:
            with patch.object(STORE.Path,"lstat",return_value=SimpleNamespace(st_mode=mode,st_uid=uid,st_gid=0)):
                with self.assertRaises(ValueError):
                    STORE.inspect_key(Path("synthetic-path"))
