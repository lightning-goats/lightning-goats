"""Review guards only: no live SSH configuration, root commands or key access."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "ssh_review", Path(__file__).resolve().parents[1] / "scripts/review-sshd-policy.py"
)
REVIEW = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REVIEW)


class SshReviewTests(unittest.TestCase):
    def files(self):
        return {
            REVIEW.MAIN: b"Include /etc/ssh/sshd_config.d/*.conf\n",
            REVIEW.SNIPPETS / REVIEW.TARGET: b"PermitRootLogin prohibit-password\n",
            REVIEW.SNIPPETS / "40-crypto.conf":
                b"Include /etc/crypto-policies/back-ends/opensshserver.config\n",
            REVIEW.CRYPTO: b"Ciphers aes256-gcm@openssh.com\n",
            REVIEW.OPTIONS: b'OPTIONS=""\n',
        }

    def test_unprivileged_review_stops_before_host_or_candidate_reads(self):
        with patch.object(REVIEW.os, "geteuid", return_value=1000), patch.object(REVIEW, "bounded_read") as read:
            with self.assertRaisesRegex(ValueError, "privileges"):
                REVIEW.review(Path("missing"))
            read.assert_not_called()

    def test_candidate_cannot_add_commands_keys_includes_or_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory) / "candidate"
            for line in ["AuthorizedKeysCommand /tmp/command", "AuthorizedKeysFile /tmp/key",
                         "Include /tmp/other", "Match User root"]:
                candidate.write_text(line + "\n")
                with self.subTest(line=line), patch.object(REVIEW.os, "geteuid", return_value=0), patch.object(REVIEW, "snapshot") as snapshot:
                    with self.assertRaisesRegex(ValueError, "outside the reviewed scope"):
                        REVIEW.review(candidate)
                    snapshot.assert_not_called()

    def test_unreviewed_host_layout_is_rejected_before_service_inspection(self):
        cases = [
            (REVIEW.MAIN, b"Include /tmp/other/*.conf\n"),
            (REVIEW.MAIN, b"Include /etc/ssh/sshd_config.d/*.conf\nMatch Address 192.0.2.0/24\nPasswordAuthentication yes\n"),
            (REVIEW.SNIPPETS / "40-crypto.conf", b"Include /tmp/other\n"),
            (REVIEW.CRYPTO, b"Include /tmp/crypto-override\n"),
            (REVIEW.OPTIONS, b'OPTIONS="-o PasswordAuthentication=yes"\n'),
        ]
        for path, data in cases:
            files = self.files()
            files[path] = data
            with self.subTest(path=path, data=data), patch.object(REVIEW, "command") as command:
                with self.assertRaises(ValueError):
                    REVIEW.verify_layout(files)
                command.assert_not_called()

    def test_service_override_is_rejected_and_observed_empty_options_are_supported(self):
        invocation = "{ argv[]=/usr/sbin/sshd -D $OPTIONS ; }"
        environment = "/etc/sysconfig/sshd (ignore_errors=yes)\n"
        with patch.object(REVIEW, "command", side_effect=[invocation, environment]):
            self.assertTrue(REVIEW.verify_layout(self.files())["options_empty"])
        for responses in [
            ["{ argv[]=/usr/sbin/sshd -D -o PasswordAuthentication=yes ; }"],
            [invocation, "/tmp/other-env (ignore_errors=yes)\n"],
        ]:
            with self.subTest(responses=responses), patch.object(REVIEW, "command", side_effect=responses):
                with self.assertRaises(ValueError):
                    REVIEW.verify_layout(self.files())


if __name__ == "__main__":
    unittest.main()
