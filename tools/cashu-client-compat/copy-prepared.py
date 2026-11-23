"""Copy an explicitly pinned prepared SDK inventory into a disposable fixture."""
import hashlib
import json
import os
from pathlib import Path
import stat


def read_regular(path, limit):
    if path.resolve(strict=True) != path:
        raise ValueError('prepared package alias refused')
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > limit:
            raise ValueError('prepared package file type or size refused')
        data = stream.read(limit + 1)
    if len(data) != info.st_size:
        raise ValueError('prepared package changed size')
    return data


def copy_prepared(source, destination, expected_manifest, lock):
    if source.absolute() != source.resolve(strict=True):
        raise ValueError('prepared SDK root alias refused')
    source = source.resolve(strict=True)
    raw = read_regular(source / 'package-files.json', 1024 * 1024)
    if hashlib.sha256(raw).hexdigest() != expected_manifest:
        raise ValueError('prepared SDK inventory hash mismatch')
    inventory = json.loads(raw)
    if (inventory['version'] != 1 or inventory['lock_sha256'] != hashlib.sha256(lock).hexdigest()
            or read_regular(source / 'package-lock-evidence.json', 65536) != lock):
        raise ValueError('prepared SDK lock mismatch')
    files = inventory['files']
    if not isinstance(files, dict) or not 1 <= len(files) <= 8192:
        raise ValueError('prepared SDK inventory exceeds bound')
    actual = set()
    def failed_walk(error):
        raise error
    for directory, dirs, names in os.walk(source, onerror=failed_walk):
        for name in dirs + names:
            path = Path(directory) / name
            mode = path.lstat().st_mode
            if not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                raise ValueError('prepared SDK aliases and special entries refused')
            if stat.S_ISREG(mode):
                actual.add(str(path.relative_to(source)))
    if actual != set(files) | {'package-files.json', 'package-lock-evidence.json'}:
        raise ValueError('unlisted or missing prepared SDK file')
    total = 0
    destination.mkdir(mode=0o755)
    for relative, expected in files.items():
        path = Path(relative)
        if (not relative.startswith('node_modules/') or path.is_absolute()
                or '..' in path.parts or str(path) != relative):
            raise ValueError('invalid prepared SDK member path')
        data = read_regular(source / path, 8 * 1024 * 1024)
        total += len(data)
        if total > 64 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != expected:
            raise ValueError('prepared SDK file changed or size exceeded')
        target = destination / path
        target.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
        with target.open('xb') as stream:
            stream.write(data)
        target.chmod(0o444)
    for package in json.loads(lock):
        metadata = json.loads((destination / 'node_modules' / package['name'] / 'package.json').read_bytes())
        if (metadata['name'], metadata['version']) != (package['name'], package['version']):
            raise ValueError('prepared SDK package identity mismatch')
    return {'manifest_sha256': expected_manifest, 'files': len(files), 'bytes': total}
