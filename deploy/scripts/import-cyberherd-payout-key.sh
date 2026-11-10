#!/bin/sh
# Installed root-owned as /usr/local/bin/install-cyberherd-payout-key.
# The argument, if supplied, is a private file path, never an API key.
set -eu
exec /usr/bin/python3 -I -S -B - "$@" <<'PY'
import getpass
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import warnings

LAUNCHER = '/usr/local/libexec/lightning-goats-upgrade-2e3b289/launch-vps-upgrade.py'
MANIFEST = '737ddc9bf91efc3d3d958affa38986b3c9d38fbb2083aee79066ed4eb703ae00'
STORE = Path('/etc/credstore.encrypted')

def main():
    if os.geteuid() != 0:
        raise ValueError('run with sudo')
    if len(sys.argv) > 2:
        raise ValueError('usage: install-cyberherd-payout-key [PRIVATE_KEY_FILE]')
    os.umask(0o077)
    temporary = None
    try:
        if len(sys.argv) == 2:
            source = Path(sys.argv[1])
        else:
            for path in [*reversed(STORE.parents), STORE]:
                info = path.lstat()
                if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_gid != 0 or info.st_mode & 0o022:
                    raise ValueError('protected credential store required')
            if STORE.stat().st_mode & 0o077:
                raise ValueError('private credential store required')
            warnings.simplefilter('error', getpass.GetPassWarning)
            key = getpass.getpass('CyberHerd Strike payout API key (hidden): ')
            if not key or len(key) > 8192:
                raise ValueError('empty or oversized key input')
            with tempfile.NamedTemporaryFile(prefix='.cyberherd-payout-key-input-', dir=STORE, delete=False) as output:
                temporary = Path(output.name)
                os.fchmod(output.fileno(), 0o600)
                output.write(key.encode('ascii'))
                output.flush()
                os.fsync(output.fileno())
            del key
            source = temporary
        return subprocess.run([
            '/usr/bin/python3', '-I', '-S', '-B', LAUNCHER, MANIFEST,
            '--cyberherd-payout-key', '--key-file', str(source), '--import-key',
        ], env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}, check=False).returncode
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)

try:
    raise SystemExit(main())
except (ValueError, OSError, UnicodeError, getpass.GetPassWarning):
    print('Payout key import failed; key input withheld. Use a private root-owned file if the terminal cannot hide input.', file=sys.stderr)
    raise SystemExit(1)
except (KeyboardInterrupt, EOFError):
    print('\nPayout key import cancelled.', file=sys.stderr)
    raise SystemExit(130)
PY
