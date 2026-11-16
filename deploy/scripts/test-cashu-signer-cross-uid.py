#!/usr/bin/env python3
"""Qualify synthetic signing over real distinct UIDs; no services, external network or real keys."""
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
import time
import uuid

PREFIX = 'publication_broker::cross_uid_fixture::'
SERVER = PREFIX + 'isolated_signer_child'
CLIENT = PREFIX + 'isolated_wallet_child'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wallet-test', required=True, type=Path)
    args = parser.parse_args()
    if os.geteuid() != 0 or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('requires root in a fresh network namespace')
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link']))
    if [link['ifname'] for link in links] != ['lo']:
        raise ValueError('fixture requires loopback only')
    used = {entry.pw_uid for entry in pwd.getpwall()} | {entry.gr_gid for entry in grp.getgrall()}
    used |= {int(value) for value in subprocess.check_output(['ps', '-e', '-o', 'uid='], text=True).split()}
    available = [value for value in range(60000, 65000) if value not in used]
    if len(available) < 4:
        raise ValueError('no unused numeric fixture identities')
    signer, wallet, rogue, group = available[:4]
    source = args.wallet_test.resolve(strict=True)
    if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
        raise ValueError('invalid explicitly selected native test artifact')
    with tempfile.TemporaryDirectory(prefix='lg-cashu-signer-ipc-', dir='/tmp') as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        for name, uid, mode in [('signer', signer, 0o700), ('wallet', wallet, 0o700), ('run', signer, 0o710)]:
            directory = root / name
            directory.mkdir(mode=mode)
            os.chown(directory, uid, group)
        scope = {'signer_uid': signer, 'wallet_uid': wallet, 'ipc_gid': group}
        scope.update({key: str(uuid.uuid4()) for key in ('wallet_instance', 'wallet_epoch', 'broker_instance', 'broker_epoch')})
        (root / 'scope.json').write_text(json.dumps(scope))
        (root / 'scope.json').chmod(0o644)
        binary = root / 'wallet-test'
        shutil.copyfile(source, binary)
        binary.chmod(0o555)
        with binary.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        environment = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LG_CASHU_SIGNER_FIXTURE': str(root),
                       'LG_CASHU_HOST_NETNS': os.readlink('/proc/1/ns/net')}

        def command(uid, *tail):
            return ['/usr/bin/setpriv', f'--reuid={uid}', f'--regid={group}', '--clear-groups', '--no-new-privs', '--', *map(str, tail)]

        listed = subprocess.run(command(wallet, binary, '--list'), env=environment, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=15, check=True)
        if any(f'{name}: test' not in listed.stdout.splitlines() for name in (SERVER, CLIENT)):
            raise ValueError('required current native fixtures are missing')
        socket = root / 'run/sign.sock'
        for phase in ('first', 'restart'):
            environment['LG_CASHU_SIGNER_PHASE'] = phase
            (root / 'stop').unlink(missing_ok=True)
            socket.unlink(missing_ok=True)
            server = subprocess.Popen(command(signer, binary, SERVER, '--exact', '--ignored', '--nocapture'),
                                      env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, text=True, start_new_session=True)
            try:
                deadline = time.monotonic() + 15
                while not socket.exists():
                    if server.poll() is not None or time.monotonic() >= deadline:
                        raise ValueError('isolated signer did not become ready')
                    time.sleep(0.05)
                # Filesystem traversal is allowed, but the kernel peer UID is not.
                probe = 'import socket,sys; s=socket.socket(socket.AF_UNIX); s.settimeout(3); s.connect(sys.argv[1]); assert s.recv(1)==b""; print("REJECTED_BEFORE_PAYLOAD")'
                rejected = subprocess.run(command(rogue, '/usr/bin/python3', '-c', probe, socket),
                                          env=environment, stdin=subprocess.DEVNULL, capture_output=True,
                                          text=True, timeout=5)
                if rejected.returncode or 'REJECTED_BEFORE_PAYLOAD' not in rejected.stdout:
                    raise ValueError('unauthorized UID was not rejected before payload I/O')
                client = subprocess.run(command(wallet, binary, CLIENT, '--exact', '--ignored', '--nocapture'),
                                        env=environment, stdin=subprocess.DEVNULL, capture_output=True,
                                        text=True, timeout=30)
                if client.returncode or 'CASHU_WALLET_SIGNER_CROSS_UID_LOST_REPLY_VERIFIED' not in client.stdout:
                    raise ValueError('wallet fixture failed: ' + (client.stdout + client.stderr)[-8192:])
                (root / 'stop').touch()
                output, error = server.communicate(timeout=5)
                if server.returncode or 'CASHU_SIGNER_CROSS_UID_DURABLE_REPLAY_VERIFIED' not in output:
                    raise ValueError('signer fixture failed: ' + (output + error)[-8192:])
            finally:
                try:
                    os.killpg(server.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                server.communicate(timeout=5)
        print(json.dumps({'result': 'CASHU_SIGNER_CROSS_UID_VERIFIED', 'signer_uid': signer,
                          'wallet_uid': wallet, 'unauthorized_uid': rogue, 'wallet_test_sha256': digest,
                          'retained_operations': 2, 'crypto_sign_calls': 2, 'restart_crypto_calls': 0,
                          'lost_reply_replayed': True, 'financial_state_unchanged': True,
                          'external_network': False, 'synthetic_crypto': True}, sort_keys=True))


if __name__ == '__main__':
    main()
