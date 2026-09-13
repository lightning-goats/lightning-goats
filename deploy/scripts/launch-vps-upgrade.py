#!/usr/bin/env python3
"""Root-installed bootstrap; authenticate this file externally BEFORE interpreting it.

Invoke with /usr/bin/python3 -I -S -B and an independently reviewed TOOL.json
SHA256. See inactive-upgrade-transaction.md. This is not a self-authentication
mechanism for an untrusted checkout or a grant of administrative exclusivity.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import stat
import sys

FILES = {
    'launch-vps-upgrade.py', 'upgrade-vps-canary.py',
    'inactive_file_transaction.py', 'preflight-release.py',
    'prepare-vps-canary.py', 'smoke-release.py',
}


def trusted(path, directory=False):
    info = path.lstat()
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    if not kind(info.st_mode) or info.st_uid != 0 or info.st_gid != 0 or info.st_mode & 0o022:
        raise ValueError('untrusted tool path')
    if not directory and info.st_nlink != 1:
        raise ValueError('aliased tool file')
    if any(n.startswith('system.posix_acl') or n == 'security.capability' for n in os.listxattr(path)):
        raise ValueError('unreviewed tool ACL/capability')


def bounded(path):
    trusted(path)
    with path.open('rb') as stream:
        data = stream.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError('tool file exceeds budget')
    return data


def verify(root, expected):
    if not re.fullmatch('[0-9a-f]{64}', expected):
        raise ValueError('expected independent tool manifest SHA256')
    if not root.is_absolute():
        raise ValueError('absolute tool path required')
    for parent in [*reversed(root.parents), root]:
        trusted(parent, directory=True)
    if {p.name for p in root.iterdir()} != FILES | {'TOOL.json'}:
        raise ValueError('unexpected tool bundle contents')
    data = bounded(root / 'TOOL.json')
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('tool manifest digest mismatch')
    manifest = json.loads(data)
    if (manifest.get('version') != 1 or not re.fullmatch('[0-9a-f]{40}', manifest.get('source_commit', ''))
            or set(manifest.get('files', {})) != FILES):
        raise ValueError('unsupported tool manifest')
    for name, digest in manifest['files'].items():
        if hashlib.sha256(bounded(root / name)).hexdigest() != digest:
            raise ValueError('tool payload drift')
    return {'source_commit': manifest['source_commit'], 'manifest_sha256': expected}


def main():
    if os.geteuid() != 0 or not sys.flags.isolated or not sys.flags.no_site or not sys.dont_write_bytecode:
        raise ValueError('root and isolated Python -I -S -B required')
    root = Path(__file__).absolute().parent
    expected = sys.argv[1]
    verify(root, expected)  # No local code has been imported before this point.
    # Only authenticated local files can now participate in imports. -I/-S
    # excluded the caller directory, PYTHONPATH, user site and site customizers.
    sys.path.insert(0, str(root))
    sys.argv = [str(root / 'upgrade-vps-canary.py'), *sys.argv[2:]]
    runpy.run_path(sys.argv[0], run_name='__main__', init_globals={
        'verified_tool_identity': lambda: verify(root, expected),
    })


if __name__ == '__main__':
    main()
