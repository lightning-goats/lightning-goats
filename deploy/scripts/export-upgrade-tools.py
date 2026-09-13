#!/usr/bin/env python3
"""Export a pinned commit's upgrade tools as data, without installation/execution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

# Explicit closure, deliberately not imported from the code being exported.
FILES = (
    'launch-vps-upgrade.py', 'upgrade-vps-canary.py',
    'inactive_file_transaction.py', 'preflight-release.py',
    'prepare-vps-canary.py', 'smoke-release.py',
)


def export(repository, source, destination):
    if os.geteuid() == 0:
        raise ValueError('export must run unprivileged; this is not a trusted installer')
    if not re.fullmatch('[0-9a-f]{40}', source):
        raise ValueError('full source commit required')
    def git(*args):
        return subprocess.check_output(
            ['/usr/bin/git', '--no-replace-objects', '-C', str(repository), *args],
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'GIT_CONFIG_NOSYSTEM': '1',
                 'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_TERMINAL_PROMPT': '0'}, timeout=30)
    if git('cat-file', '-t', source).strip() != b'commit':
        raise ValueError('source must name a commit object')
    payload = {}
    for name in FILES:
        member = 'deploy/scripts/' + name
        row = git('ls-tree', source, '--', member).decode().strip()
        header, path = row.split('\t')
        mode, kind, blob = header.split()
        if path != member or mode not in ('100644', '100755') or kind != 'blob':
            raise ValueError('tool must be a regular committed file')
        if int(git('cat-file', '-s', blob)) > 1024 * 1024:
            raise ValueError('tool file exceeds budget')
        payload[name] = git('cat-file', 'blob', blob)
    manifest = {'version': 1, 'source_commit': source, 'files': {
        name: hashlib.sha256(data).hexdigest() for name, data in payload.items()}}
    payload['TOOL.json'] = (json.dumps(manifest, sort_keys=True, indent=2) + '\n').encode()
    destination = Path(destination)
    destination.mkdir(mode=0o700)  # Refuse reuse, including a dangling symlink.
    for name, data in payload.items():
        descriptor = os.open(destination / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as output:
            output.write(data)
    return {'source_commit': source,
            'manifest_sha256': hashlib.sha256(payload['TOOL.json']).hexdigest(),
            'launcher_sha256': manifest['files']['launch-vps-upgrade.py'],
            'installed': False, 'executed': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['repository', 'source', 'destination']:
        parser.add_argument(name)
    args = parser.parse_args()
    print(json.dumps(export(args.repository, args.source, args.destination), sort_keys=True, indent=2))
