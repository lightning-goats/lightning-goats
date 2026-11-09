#!/usr/bin/env python3
"""Run on the household host: append one public client, retaining existing clients."""
import argparse
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import time


ENVIRONMENT = Path('/etc/nostr-bunker/earthship-operator.env')
UNIT = 'nostr-bunker@earthship-operator.service'


def updated_environment(text, client):
    if not re.fullmatch(r'[0-9a-f]{64}', client):
        raise ValueError('client public key must be 64 lowercase hex characters')
    matches = list(re.finditer(r'^BUNKER_AUTHORIZED_CLIENTS="([0-9a-fA-F ]+)"$',
                              text, re.MULTILINE))
    if len(matches) != 1:
        raise ValueError('expected exactly one quoted public client allowlist')
    match = matches[0]
    keys = match.group(1).split()
    if not keys or any(not re.fullmatch(r'[0-9a-fA-F]{64}', key) for key in keys):
        raise ValueError('existing client allowlist is invalid')
    if client in [key.lower() for key in keys]:
        return text
    return text[:match.start(1)] + ' '.join([*keys, client]) + text[match.end(1):]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('client_public_key')
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError('run with sudo on the household host')
    os.umask(0o077)
    metadata = ENVIRONMENT.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
        raise ValueError('unexpected bunker environment ownership/type/permissions')
    subprocess.run(['systemctl', 'is-active', '--quiet', UNIT], check=True)
    before = ENVIRONMENT.read_text()
    after = updated_environment(before, args.client_public_key)
    if after != before:
        backup = ENVIRONMENT.with_name(ENVIRONMENT.name + time.strftime('.lg-%Y%m%dT%H%M%SZ', time.gmtime()))
        if backup.exists():
            raise ValueError('backup already exists; inspect before retrying')
        shutil.copy2(ENVIRONMENT, backup)
        pending = ENVIRONMENT.with_name(ENVIRONMENT.name + '.lg-pending')
        with pending.open('x') as output:
            output.write(after)
            output.flush()
            os.fsync(output.fileno())
        pending.chmod(stat.S_IMODE(metadata.st_mode))
        os.chown(pending, metadata.st_uid, metadata.st_gid)
        if ENVIRONMENT.read_text() != before:
            pending.unlink()
            raise ValueError('environment changed concurrently; not replaced')
        os.replace(pending, ENVIRONMENT)
        subprocess.run(['systemctl', 'restart', UNIT], check=True)
        print('Added Lightning Goats client; existing clients retained. Backup:', backup)
    else:
        print('Lightning Goats client already present; no restart performed.')
    # This file is defined by the Earthship deployment as public-only settings.
    for line in after.splitlines():
        if line.startswith(('BUNKER_AUTHORIZED_CLIENTS=', 'BUNKER_RELAYS=')):
            print(line)
    subprocess.run(['systemctl', 'show', UNIT, '-p', 'ActiveState', '-p', 'SubState',
                    '-p', 'NRestarts'], check=True)
    print('No signing credential changed and no note published. VPS round-trip still required.')


if __name__ == '__main__':
    main()
