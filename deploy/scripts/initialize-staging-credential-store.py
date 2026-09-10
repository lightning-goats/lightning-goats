#!/usr/bin/env python3
"""Initialize an empty new-VPS systemd credential store; never replace a key."""
import json
import os
from pathlib import Path
import stat
import subprocess


def inspect_key(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_gid != 0 or info.st_mode & 0o077:
        raise ValueError("existing systemd credential key is not a protected root-owned regular file")
    return {"path":str(path),"uid":info.st_uid,"gid":info.st_gid,"mode":oct(stat.S_IMODE(info.st_mode))}


def initialize():
    if os.geteuid() != 0:
        raise ValueError("credential-store initialization requires root")
    key = Path("/var/lib/systemd/credential.secret")
    if key.exists() or key.is_symlink():
        return {"created":False,**inspect_key(key)}
    for location in ["/etc/credstore.encrypted", "/run/credstore.encrypted"]:
        directory = Path(location)
        if directory.exists() and any(directory.iterdir()):
            raise ValueError("encrypted credentials exist without the host key; recover the original key before proceeding")
    subprocess.run(["systemd-creds","setup"],check=True,env={"PATH":os.defpath},timeout=30)
    return {"created":True,**inspect_key(key)}


if __name__ == "__main__":
    print(json.dumps(initialize(),sort_keys=True))
