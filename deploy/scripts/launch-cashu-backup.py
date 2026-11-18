#!/usr/bin/env python3
"""Authenticate this root-owned bootstrap externally before interpreting it.

Run with python3 -I -S -B TOOL_SHA256 POLICY POLICY_SHA256 DESTINATION
or python3 -I -S -B TOOL_SHA256 stage ARCHIVE ARCHIVE_SHA256 RECEIPT IDENTITY DESTINATION OPTIONS.
The review subcommand reads a staged checkpoint and preserves every fence.
The trusted bootstrap verifies every local module before importing one.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import stat
import sys

FILES={'launch-cashu-backup.py','collect-cashu-backup.py','stage-cashu-restore.py','review-cashu-restore.py','reconcile-cashu-restore.py',
       'verify-cashu-backup.py','verify-cashu-combined-backup.py','apply-cashu-mint-policy.py','inactive_file_transaction.py'}


def unique(pairs):
    result={}
    for k,v in pairs:
        if k in result: raise ValueError('duplicate tool manifest field')
        result[k]=v
    return result


def trusted(path,directory=False):
    info=path.lstat()
    if (not (stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode))
            or info.st_uid!=0 or info.st_gid!=0 or info.st_mode&0o022
            or (not directory and info.st_nlink!=1)
            or any(n.startswith('system.posix_acl') or n=='security.capability' for n in os.listxattr(path))):
        raise ValueError('untrusted backup tool path')


def bounded(path):
    trusted(path)
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
    with os.fdopen(fd,'rb') as source:
        before=os.fstat(source.fileno());raw=source.read(1024*1024+1)
        after=os.fstat(source.fileno());current=path.lstat()
    def identity(info):
        return (info.st_dev,info.st_ino,info.st_mode,info.st_uid,info.st_gid,
                info.st_nlink,info.st_size,info.st_mtime_ns,info.st_ctime_ns)
    if len(raw)>1024*1024 or identity(before)!=identity(after) or identity(after)!=identity(current):
        raise ValueError('backup tool file changed or exceeded bound')
    return raw


def verify(root,expected):
    if not re.fullmatch('[0-9a-f]{64}',expected) or not root.is_absolute() or root.resolve(strict=True)!=root:
        raise ValueError('external tool identity required')
    for parent in [*reversed(root.parents),root]: trusted(parent,directory=True)
    if {p.name for p in root.iterdir()}!=FILES|{'TOOL.json'}:
        raise ValueError('unexpected backup tool contents')
    raw=bounded(root/'TOOL.json')
    if hashlib.sha256(raw).hexdigest()!=expected: raise ValueError('backup tool manifest differs')
    manifest=json.loads(raw,object_pairs_hook=unique)
    if (set(manifest)!={'version','source_commit','files'} or type(manifest['version']) is not int
            or manifest['version']!=1 or not re.fullmatch('[0-9a-f]{40}',manifest['source_commit'])
            or set(manifest['files'])!=FILES): raise ValueError('unsupported backup tool manifest')
    for name,checksum in manifest['files'].items():
        if hashlib.sha256(bounded(root/name)).hexdigest()!=checksum:
            raise ValueError('backup tool payload differs')
    return {'source_commit':manifest['source_commit'],'manifest_sha256':expected}


def main():
    if os.geteuid()!=0 or not sys.flags.isolated or not sys.flags.no_site or not sys.dont_write_bytecode:
        raise ValueError('root and isolated Python -I -S -B required')
    commands = {'stage': 'stage-cashu-restore.py', 'review': 'review-cashu-restore.py',
                'reconcile': 'reconcile-cashu-restore.py', 'apply-mint-policy': 'apply-cashu-mint-policy.py'}
    subcommand = commands.get(sys.argv[2]) if len(sys.argv) >= 3 else None
    if subcommand is None and len(sys.argv) != 5:
        raise ValueError('tool/policy hashes and new destination required')
    root=Path(__file__).absolute().parent
    expected=sys.argv[1]
    verify(root,expected)
    sys.path.insert(0,str(root))
    script = root / (subcommand or 'collect-cashu-backup.py')
    arguments = sys.argv[3:] if subcommand else sys.argv[2:]
    sys.argv = [str(script), *arguments]
    runpy.run_path(str(script),run_name='__main__',
        init_globals={'verified_tool_identity':lambda:verify(root,expected)})


if __name__=='__main__':
    try: main()
    except (ValueError,OSError,KeyError,TypeError):
        raise SystemExit('Backup tool authentication refused; no authority granted.')
