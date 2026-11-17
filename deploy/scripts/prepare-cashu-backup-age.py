#!/usr/bin/env python3
"""Prepare the exact official age test dependency; never install system software."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys
import tarfile
import urllib.request

VERSION='v1.3.2'
URL='https://github.com/FiloSottile/age/releases/download/v1.3.2/age-v1.3.2-linux-amd64.tar.gz'
BYTES=19405817
SHA256='cbe24006683f8eb669266162894b9a522a1af52f2665fbc63a4bb032ed26ac10'
EXECUTABLES={'age':'eb7dd1b518f0a307c99cd97782623c5321da049154b04acd2d98d21aa7bc9b2c',
             'age-keygen':'0a0009db842259d6717f7eeb30acb6b90d2a2eb924c6acd0a0db0ca1f1537899'}


def prepare(destination,archive=None):
    if sys.platform!='linux' or platform.machine()!='x86_64':
        raise ValueError('qualified Linux amd64 artifact required')
    destination.mkdir(mode=0o700)
    (destination/'INCOMPLETE').write_bytes(b'AGE_TEST_DEPENDENCY_V1\n')
    if archive is None:
        with urllib.request.urlopen(URL,timeout=30) as source:raw=source.read(BYTES+1)
    else:
        fd=os.open(archive,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
        with os.fdopen(fd,'rb') as source:
            info=os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size!=BYTES:raise ValueError('invalid cached release')
            raw=source.read(BYTES+1)
    if len(raw)!=BYTES or hashlib.sha256(raw).hexdigest()!=SHA256:
        raise ValueError('official pinned release checksum differs')
    records={}
    with tarfile.open(fileobj=io.BytesIO(raw),mode='r:gz') as bundle:
        for name in [*EXECUTABLES,'LICENSE']:
            entries=[m for m in bundle.getmembers() if m.name=='age/'+name]
            if len(entries)!=1 or not entries[0].isfile() or not 0<entries[0].size<=16*1024**2:
                raise ValueError('missing/aliased official release member')
            with bundle.extractfile(entries[0]) as source:data=source.read(entries[0].size+1)
            checksum=hashlib.sha256(data).hexdigest()
            if len(data)!=entries[0].size or (name in EXECUTABLES and checksum!=EXECUTABLES[name]):
                raise ValueError('selected official executable differs')
            with (destination/name).open('xb') as output:
                os.fchmod(output.fileno(),0o500 if name in EXECUTABLES else 0o400)
                output.write(data)
            records[name]={'sha256':checksum,'bytes':len(data)}
    version=subprocess.run([str((destination/'age').resolve()),'--version'],stdin=subprocess.DEVNULL,
        capture_output=True,timeout=10,env={'PATH':'/usr/bin:/bin'})
    if version.returncode or version.stdout.strip()!=VERSION.encode():raise ValueError('selected age version differs')
    result={'version':VERSION,'release_url':URL,'archive_sha256':SHA256,'archive_bytes':BYTES,
            'files':records,'system_installation':False}
    (destination/'selection.json').write_text(json.dumps(result,sort_keys=True)+'\n')
    (destination/'INCOMPLETE').unlink()
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination',type=Path);parser.add_argument('--archive',type=Path)
    args=parser.parse_args()
    print(json.dumps(prepare(args.destination,args.archive),sort_keys=True))


if __name__=='__main__':main()
