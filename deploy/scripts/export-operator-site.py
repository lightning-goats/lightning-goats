#!/usr/bin/env python3
"""Export exact committed operator-site bytes/pins as unprivileged data."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

FILES = {'operator.conf': 'deploy/cyberherd/operator-only-nginx.conf.example',
         'cyberherd-operator.html': 'web/cyberherd-operator.html',
         'cyberherd-operator.js': 'web/cyberherd-operator.js'}
BROWSER = ('cyberherd.html', 'cyberherd.js', 'cyberherd-auth.js',
           'vendor/nostr.bundle.js', 'vendor/nostr-login.js')


def export(repository, source, destination):
    if os.geteuid() == 0 or re.fullmatch('[0-9a-f]{40}', source) is None:
        raise ValueError('unprivileged export with an exact commit required')

    def git(*arguments):
        return subprocess.check_output(['/usr/bin/git', '--no-replace-objects', '-C', str(repository), *arguments],
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'GIT_CONFIG_NOSYSTEM': '1',
                 'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_TERMINAL_PROMPT': '0'}, timeout=30)

    if git('cat-file', '-t', source).strip() != b'commit':
        raise ValueError('exact commit object required')

    def blob(path):
        row = git('ls-tree', source, '--', path).decode().strip()
        header, actual = row.split('\t')
        mode, kind, identifier = header.split()
        if actual != path or mode not in ('100644', '100755') or kind != 'blob':
            raise ValueError('committed regular source file required')
        if int(git('cat-file', '-s', identifier)) > 512 * 1024:
            raise ValueError('site source exceeds bound')
        return git('cat-file', 'blob', identifier)

    payload = {name: blob(path) for name, path in FILES.items()}
    manifest = {'version': 1, 'source_commit': source,
        'files': {name: hashlib.sha256(data).hexdigest() for name, data in payload.items()},
        'browser_files': {name: hashlib.sha256(blob('web/' + name)).hexdigest() for name in BROWSER}}
    payload['SOURCE.json'] = (json.dumps(manifest, indent=2, sort_keys=True) + '\n').encode()
    destination = Path(destination)
    destination.mkdir(mode=0o700)
    for name, data in payload.items():
        descriptor = os.open(destination / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(data)
    return {'source_commit': source, 'manifest_sha256': hashlib.sha256(payload['SOURCE.json']).hexdigest(),
            'installed': False, 'executed': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('repository', 'source', 'destination'):
        parser.add_argument(name)
    args = parser.parse_args()
    print(json.dumps(export(args.repository, args.source, args.destination), sort_keys=True))
