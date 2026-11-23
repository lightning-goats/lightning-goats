#!/usr/bin/env python3
"""Exercise protected financial-release approval checks with a selected native artifact.

Run as root in a fresh network namespace. This does not invoke the release CLI,
open a ledger, stop any service, or claim positive financial-release acceptance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

TESTS = {
    'cashu::financial_release::tests::approval_scope_time_and_review_are_required': None,
    'cashu::financial_release::tests::developer_owned_or_aliased_or_writable_approval_is_not_authority':
        'FINANCIAL_RELEASE_PROTECTED_APPROVAL_VERIFIED',
    'cashu::financial_release::tests::incomplete_active_or_unknown_service_state_cannot_authorize_release': None,
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core-test', required=True, type=Path)
    args = parser.parse_args()
    if os.geteuid() != 0 or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('requires root inside a fresh network namespace')
    source = args.core_test.resolve(strict=True)
    if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
        raise ValueError('invalid selected native test artifact')
    environment = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    # Freeze selected bytes before invoking them; the caller selects a trusted build.
    with tempfile.TemporaryDirectory(prefix='lg-cashu-release-check-', dir='/run') as temporary:
        program = Path(temporary) / 'core-test'
        data = source.read_bytes()
        program.write_bytes(data)
        program.chmod(0o500)
        digest = hashlib.sha256(data).hexdigest()
        listed = subprocess.run([str(program), '--list'], env=environment,
                                capture_output=True, text=True, check=True, timeout=30)
        for name in TESTS:
            if f'{name}: test' not in listed.stdout.splitlines():
                raise ValueError('selected artifact lacks required current approval test')
        for name, marker in TESTS.items():
            result = subprocess.run([str(program), name, '--exact', '--nocapture'],
                                    env=environment, capture_output=True, text=True, timeout=30)
            if (result.returncode or '1 passed; 0 failed' not in result.stdout
                    or (marker and marker not in result.stdout)):
                raise ValueError('approval check failed: ' + (result.stdout + result.stderr)[-4096:])
        print(json.dumps({'result': 'CASHU_FINANCIAL_RELEASE_APPROVAL_VERIFIED',
                          'core_test_sha256': digest, 'tests': list(TESTS),
                          'root_protected_file_branch_executed': True,
                          'positive_release_cli_qualified': False,
                          'ledger_opened': False, 'services_changed': False}, sort_keys=True))


if __name__ == '__main__':
    main()
