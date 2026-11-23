#!/usr/bin/env python3
"""Verify read-only Cashu/four-journal correlations in an isolated root fixture.

Requires a selected native test executable with an explicit SHA256. Never starts
host services, contacts a provider, sends money or authorizes recovery resumption.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

TEST = ('state_maintenance::cashu_correlations::tests::'
        'frozen_cashu_journals_preserve_native_correlations_without_restore_authority')
MARKER = 'CASHU_FOUR_JOURNAL_CORRELATIONS_VERIFIED'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core-test', type=Path, required=True)
    parser.add_argument('--core-test-sha256', required=True)
    args = parser.parse_args()
    if os.geteuid() != 0 or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('requires root in a fresh network namespace')
    source = args.core_test.resolve(strict=True)
    if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
        raise ValueError('invalid selected test executable')
    environment = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    with tempfile.TemporaryDirectory(prefix='lg-cashu-correlations-', dir='/run') as temporary:
        program = Path(temporary) / 'core-test'
        data = source.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != args.core_test_sha256:
            raise ValueError('selected test executable checksum differs')
        program.write_bytes(data)
        program.chmod(0o500)
        listing = subprocess.run([str(program), '--list'], env=environment,
                                 capture_output=True, text=True, check=True, timeout=30)
        if f'{TEST}: test' not in listing.stdout.splitlines():
            raise ValueError('selected executable lacks required native review test')
        result = subprocess.run([str(program), TEST, '--exact', '--ignored', '--nocapture'],
                                env=environment, capture_output=True, text=True, timeout=120)
        if (result.returncode or '1 passed; 0 failed' not in result.stdout
                or MARKER not in result.stdout):
            raise ValueError('correlation review fixture failed: '
                             + (result.stdout + result.stderr)[-8192:])
        print(json.dumps({'result': MARKER, 'core_test_sha256': digest,
                          'test': TEST, 'current_schema_cashu_receipts': True,
                          'native_four_journal_checks': True,
                          'ordinary_restore_refused': True,
                          'cashu_wallet_verified': False,
                          'combined_restore_authorized': False,
                          'resume_authorized': False,
                          'services_changed': False}, sort_keys=True))


if __name__ == '__main__':
    main()
