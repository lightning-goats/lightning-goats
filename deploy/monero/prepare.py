#!/usr/bin/env python3
"""Offline-only bridge release staging and SQLite backup/restore preparation.
No package installation, wallet/network access, service activation or live restore.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import urllib.parse
import uuid

SCHEMA = "monero_bridge_meta"

def digest_file(path):
    with Path(path).open("rb") as file:
        return hashlib.file_digest(file,"sha256").hexdigest()


def regular(path):
    p = Path(path)
    if p.is_symlink() or not p.is_file(): raise ValueError("regular source file required")
    return p

def backup(source, output, restore=False):
    source = regular(source).resolve()
    output = Path(output)
    for target in [output, Path(str(output)+"-wal"), Path(str(output)+"-shm"), Path(str(output)+"-journal")]:
        if target.exists() or target.is_symlink():
            raise ValueError("never overwrite an existing backup, state file or SQLite sidecar")
    info=output.parent.stat()
    if info.st_uid != os.geteuid() or info.st_mode & 0o022: raise ValueError("private output directory required")
    # Caller explicitly confirms all writers stopped. Read-only SQLite preserves WAL.
    uri = "file:" + urllib.parse.quote(str(source), safe="/") + "?mode=ro"
    with closing(sqlite3.connect(uri, uri=True)) as db:
        if db.execute("PRAGMA integrity_check").fetchall() != [("ok",)]: raise ValueError("source integrity")
        row = db.execute("SELECT generation FROM monero_bridge_meta WHERE id=1").fetchone()
        if not row: raise ValueError("not a bridge database")
        fd = os.open(output, os.O_CREAT|os.O_EXCL|os.O_WRONLY, 0o600);os.close(fd)
        try:
            with closing(sqlite3.connect(output)) as target:
                db.backup(target)
                if restore:
                    # A prepared standalone restore cannot silently become eligible.
                    target.execute("UPDATE monero_bridge_meta SET generation=? WHERE id=1", (str(uuid.uuid4()),))
                    target.execute("UPDATE monero_bridge_intents SET available=0,lease='',lease_until=0,hold=COALESCE(hold,'restore_review_required')")
                target.commit()
                target.execute("PRAGMA wal_checkpoint(TRUNCATE)")
                if target.execute("PRAGMA integrity_check").fetchall() != [("ok",)]: raise ValueError("backup integrity")
                if target.execute("PRAGMA foreign_key_check").fetchall(): raise ValueError("backup foreign keys")
            with output.open("rb") as f: os.fsync(f.fileno())
        except Exception:
            output.unlink(missing_ok=True);raise
    return {"sha256":digest_file(output), "prepared_restore":restore, "activated":False}

def stage(binary, digest, output):
    binary = regular(binary)
    if binary.stat().st_size > 256*1024*1024: raise ValueError("binary too large")
    blob=binary.read_bytes()
    if hashlib.sha256(blob).hexdigest() != digest: raise ValueError("binary digest mismatch")
    output = Path(output)
    output.mkdir(mode=0o700)  # refuses an existing output; no updates to live paths
    (output/"lightning-goats-monero-bridge").write_bytes(blob)
    os.chmod(output/"lightning-goats-monero-bridge",0o755)
    source = Path(__file__).resolve().parent
    names=["config.toml.example","sync.json.example","sync-probe.py","lightning-goats-monero-bridge.service","lightning-goats-monero-sync.service","lightning-goats-monero-sync.timer","nginx-mtls.conf.example","monero-egress.nft.example","render-config.py","install-inactive.sh"]
    for name in names: shutil.copyfile(regular(source/name),output/name)
    manifest={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in output.iterdir() if p.is_file()}
    (output/"SHA256SUMS").write_text("".join(f"{v}  {k}\n" for k,v in sorted(manifest.items())))
    return {"staged_files":len(manifest), "activated":False, "configuration_required":True}

def main():
    p=argparse.ArgumentParser(description=__doc__);sub=p.add_subparsers(dest="command",required=True)
    for cmd in ["backup","prepare-restore"]:
        s=sub.add_parser(cmd);s.add_argument("source");s.add_argument("output");s.add_argument("--ack-quiesced",action="store_true",required=True)
    s=sub.add_parser("stage");s.add_argument("binary");s.add_argument("sha256");s.add_argument("output")
    a=p.parse_args()
    try:
        if a.command=="stage":r=stage(a.binary,a.sha256,a.output)
        else:r=backup(a.source,a.output,a.command=="prepare-restore")
        print(json.dumps(r,sort_keys=True))
    except Exception:
        p.exit(1,"Bridge preparation failed; no service was activated.\n")
if __name__=="__main__":main()
