#!/usr/bin/env python3
"""Run a selected fixture with disk-backed /run in private mount/net namespaces.

The parent must unshare both namespaces. No host mount or service is changed;
all temporary storage is removed after the child. Useful on small /run tmpfs.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command and command[0] == '--':
        command = command[1:]
    if not command or os.geteuid() != 0:
        raise ValueError('requires root and an explicit fixture command')
    for namespace in ['mnt', 'net']:
        if os.readlink('/proc/self/ns/' + namespace) == os.readlink('/proc/1/ns/' + namespace):
            raise ValueError('requires fresh mount and network namespaces')
    subprocess.run(['/usr/bin/mount', '--make-rprivate', '/'], check=True)
    with tempfile.TemporaryDirectory(prefix='lg-cashu-private-run-', dir='/var/tmp') as temporary:
        # Like /run: traversable to distinct non-admin fixture identities, with
        # root as the only directory writer. Children protect their own state.
        Path(temporary).chmod(0o755)
        subprocess.run(['/usr/bin/mount', '--bind', temporary, '/run'], check=True)
        try:
            result = subprocess.run(command, stdin=subprocess.DEVNULL,
                                    env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
        finally:
            subprocess.run(['/usr/bin/umount', '/run'], check=True)
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
