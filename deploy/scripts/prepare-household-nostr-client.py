#!/usr/bin/env python3
"""Create/reuse only the VPS NIP-46 client credential; publish nothing."""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile


NAK = Path('/usr/local/bin/nak')
DIGEST = 'b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9'
STORE = Path('/etc/credstore.encrypted')
TARGET = STORE / 'lightning-goats-nostr-client'


def protected(path, directory=False):
    metadata = path.lstat()
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    if not kind(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
        raise ValueError('unsafe ownership/type/permissions: ' + str(path))
    return metadata


def run(args, *, data=None, environment=None):
    result = subprocess.run(args, input=data, env=environment, capture_output=True)
    if result.returncode:
        # Subprocess diagnostics can contain credential data.
        raise ValueError('credential preparation subprocess failed; output withheld')
    return result.stdout


def main():
    if os.geteuid() != 0:
        raise ValueError('run as root')
    os.umask(0o077)
    protected(NAK)
    if hashlib.sha256(NAK.read_bytes()).hexdigest() != DIGEST:
        raise ValueError('pinned nak digest mismatch')
    protected(STORE, directory=True)
    with tempfile.TemporaryDirectory(prefix='.nostr-client-', dir=STORE) as temporary:
        temporary = Path(temporary)
        clean_env = {'PATH': '/usr/bin:/bin', 'NO_COLOR': '1'}
        created = not TARGET.exists() and not TARGET.is_symlink()
        if created:
            key = run([str(NAK), '--config-path', str(temporary / 'cache'),
                       'key', 'generate'], environment=clean_env).strip()
            if not re.fullmatch(rb'[0-9a-f]{64}', key):
                raise ValueError('nak generated an invalid client key')
            encrypted = temporary / 'encrypted'
            run(['systemd-creds', 'encrypt', '--with-key=host',
                 '--name=nostr-client-key', '-', str(encrypted)], data=key + b'\n')
            encrypted.chmod(0o600)
            checked = run(['systemd-creds', 'decrypt', '--name=nostr-client-key',
                           str(encrypted), '-']).strip()
            if checked != key:
                raise ValueError('encrypted credential readback mismatch')
        else:
            metadata = protected(TARGET)
            if stat.S_IMODE(metadata.st_mode) != 0o600:
                raise ValueError('existing client credential must be mode 0600')
            key = run(['systemd-creds', 'decrypt', '--name=nostr-client-key',
                       str(TARGET), '-']).strip()
        if not re.fullmatch(rb'[0-9a-f]{64}', key):
            raise ValueError('client credential is not a valid hex private key')
        public = run([str(NAK), '--config-path', str(temporary / 'cache'),
                      'key', 'public'], data=key + b'\n',
                     environment=clean_env).strip().decode()
        if not re.fullmatch(r'[0-9a-f]{64}', public):
            raise ValueError('invalid client public key')
        if created:
            # Exclusive installation: a concurrent existing key is never replaced.
            os.link(encrypted, TARGET)
            run(['restorecon', str(TARGET)])
        print(json.dumps({'client_public_key': public, 'created': created,
                          'credential': str(TARGET), 'services_changed': False,
                          'events_published': 0}))


if __name__ == '__main__':
    main()
