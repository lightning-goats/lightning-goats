#!/usr/bin/env python3
"""Prepare locked parser dependencies without npm or executing package scripts.

Downloads public archives only. Run offline verification separately inside an
isolated network namespace. The destination must not already exist.
"""
import argparse
import base64
import hashlib
import io
import json
from pathlib import Path
import tarfile
import urllib.request


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination',type=Path)
    args=parser.parse_args()
    lock=Path(__file__).with_name('packages.json')
    packages=json.loads(lock.read_text())
    destination=args.destination.resolve()
    destination.mkdir(mode=0o755,parents=False,exist_ok=False)
    for package in packages:
        name,version,url=package['name'],package['version'],package['url']
        if name not in ['@cashu/cashu-ts','@scure/base','@scure/bip32','@noble/curves','@noble/hashes']:
            raise ValueError('unexpected package')
        expected=f'https://registry.npmjs.org/{name}/-/{name.split("/")[-1]}-{version}.tgz'
        if url != expected or not package['integrity'].startswith('sha512-'):
            raise ValueError('invalid locked package URL/integrity')
        with urllib.request.urlopen(url,timeout=30) as response:
            if response.url != url: raise ValueError('archive redirect refused')
            data=response.read(8*1024*1024+1)
        if len(data)>8*1024*1024:
            raise ValueError('archive too large')
        actual='sha512-'+base64.b64encode(hashlib.sha512(data).digest()).decode()
        if actual != package['integrity']:
            raise ValueError('archive integrity mismatch')
        root=destination/'node_modules'/name
        root.mkdir(parents=True,exist_ok=False)
        with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as archive:
            entries=archive.getmembers()
            if len(entries)>4096 or sum(entry.size for entry in entries)>32*1024*1024:
                raise ValueError('unpacked package exceeds bound')
            seen=set()
            for entry in entries:
                path=Path(entry.name)
                if (not path.parts or path.parts[0]!='package' or '..' in path.parts
                        or path.is_absolute() or path in seen or not (entry.isdir() or entry.isfile())):
                    raise ValueError('unsafe or duplicate archive member')
                seen.add(path)
                target=root.joinpath(*path.parts[1:])
                if entry.isdir(): target.mkdir(parents=True,exist_ok=True)
                else:
                    target.parent.mkdir(parents=True,exist_ok=True)
                    with target.open('xb') as output: output.write(archive.extractfile(entry).read())
        metadata=json.loads((root/'package.json').read_text())
        if metadata['name']!=name or metadata['version']!=version:
            raise ValueError('package identity mismatch')
    (destination/'package-lock-evidence.json').write_bytes(lock.read_bytes())
    files={str(path.relative_to(destination)):hashlib.sha256(path.read_bytes()).hexdigest()
           for path in sorted((destination/'node_modules').rglob('*')) if path.is_file()}
    (destination/'package-files.json').write_text(json.dumps({
        'version':1,'lock_sha256':hashlib.sha256(lock.read_bytes()).hexdigest(),'files':files,
    },indent=2)+'\n')
    print('Locked dependencies prepared; no package scripts or wallet operations executed.')


if __name__=='__main__':
    main()
