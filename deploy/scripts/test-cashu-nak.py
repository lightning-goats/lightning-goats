#!/usr/bin/env python3
"""Run the pinned Cashu NIP-46/NIP-44 fixture as an unused UID in a fresh net namespace."""
import argparse
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import shutil
import signal
import subprocess
import tempfile

TEST = 'publication_nak::tests::isolated_real_nak_bunker_roundtrip'
MARKER = 'CASHU_REAL_NIP46_NIP44_AND_KINDS_VERIFIED'
NAK_SHA = 'b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9'


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wallet-test', required=True, type=Path)
    parser.add_argument('--nak-helper', required=True, type=Path)
    parser.add_argument('--nak-sha256', required=True)
    args = parser.parse_args()
    if os.geteuid() != 0 or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('requires root inside a fresh network namespace')
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link']))
    if [link['ifname'] for link in links] != ['lo']:
        raise ValueError('fixture namespace must contain only loopback')
    subprocess.run(['/usr/sbin/ip', 'link', 'set', 'lo', 'up'], check=True)
    nak = Path('/usr/local/bin/nak')
    if digest(nak) != NAK_SHA:
        raise ValueError('requires pinned nak v0.20.6')
    helper_source=args.nak_helper.resolve(strict=True)
    if digest(helper_source)!=args.nak_sha256 or helper_source.stat().st_size>100_000_000:
        raise ValueError('patched helper digest or size mismatch')
    source = args.wallet_test.resolve(strict=True)
    if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
        raise ValueError('invalid explicitly selected native test artifact')
    used = {item.pw_uid for item in pwd.getpwall()} | {item.gr_gid for item in grp.getgrall()}
    used |= {int(value) for value in subprocess.check_output(['ps', '-e', '-o', 'uid='], text=True).split()}
    fixture_uid = next(value for value in range(60000, 65000) if value not in used)
    with tempfile.TemporaryDirectory(prefix='lg-cashu-nak-', dir='/tmp') as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        scratch = root / 'scratch'
        scratch.mkdir(mode=0o700)
        os.chown(scratch, fixture_uid, fixture_uid)
        binary = root / 'wallet-test'
        shutil.copyfile(source, binary)
        binary.chmod(0o555)
        binary_sha = digest(binary)
        helper=root / 'nak-cashu-stdin'
        shutil.copyfile(helper_source,helper)
        helper.chmod(0o555)
        environment = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'TMPDIR': str(scratch),
                       'LG_CASHU_HOST_NETNS': os.readlink('/proc/1/ns/net'),
                       'LG_CASHU_NAK_HELPER': str(helper),'LG_CASHU_NAK_SHA256': args.nak_sha256}
        command = ['/usr/bin/setpriv', f'--reuid={fixture_uid}', f'--regid={fixture_uid}',
                   '--clear-groups', '--no-new-privs', '--', str(binary)]
        listed = subprocess.run(command + ['--list'], env=environment, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=15, check=True)
        if f'{TEST}: test' not in listed.stdout.splitlines():
            raise ValueError('required current native fixture is missing')
        child = subprocess.Popen(command + [TEST, '--exact', '--ignored', '--nocapture'],
                                 env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, text=True, start_new_session=True)
        try:
            output, error = child.communicate(timeout=240)
            if child.returncode != 0 or MARKER not in output:
                # All credentials are public fixture scalars; diagnostics remain bounded.
                raise RuntimeError('isolated Cashu fixture failed: ' + (output + error)[-8192:])
        finally:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)
        print(json.dumps({'accepted': True, 'wallet_test_sha256': binary_sha,
                          'nak_bunker_sha256': NAK_SHA,'nak_client_sha256': args.nak_sha256, 'fixture_uid': fixture_uid,
                          'kinds': [10019, 7376], 'nip44_roundtrip': True, 'tampered_mac_rejected': True,
                          'retained_result_without_bunker': True,
                          'external_network': False, 'production_credentials_read': False}, sort_keys=True))


if __name__ == '__main__':
    main()
