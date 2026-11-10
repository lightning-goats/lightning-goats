#!/usr/bin/env python3
"""Encrypt a separate payout key; no webhook, service, API or payment operation.

Run through the authenticated root-owned launcher. The key is read from a private
root-owned file, never from argv/environment. Existing different credentials are
never rotated or replaced. A public fingerprint binds later disabled policy.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import hashlib
import hmac
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess

SPEC = importlib.util.spec_from_file_location('payout_credential_support', Path(__file__).with_name('upgrade-cyberherd-core.py'))
C = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(C)
C.verified_tool_identity = lambda: verified_tool_identity()
STORE = Path('/etc/credstore.encrypted')
TARGET = STORE / 'cyberherd-strike-payout'
HOST_KEY = Path('/var/lib/systemd/credential.secret')


def token(raw):
    if len(raw) > 8192:
        raise ValueError('payout key file exceeds the input bound')
    key = raw.strip()
    if re.fullmatch(rb'[!-~]{1,4096}', key) is None:
        raise ValueError('payout key must be one nonempty ASCII token without internal whitespace/control characters')
    return key


def crypt(operation, raw):
    args = ['/usr/bin/systemd-creds', operation, '--name=strike-payout-api-key']
    if operation == 'encrypt':
        args.append('--with-key=host')
    result = subprocess.run([*args, '-', '-'], input=raw,
                            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}, capture_output=True, timeout=30)
    if result.returncode or len(result.stdout) > 65536:
        raise ValueError('payout credential subprocess failed; output withheld')
    return result.stdout


def prepare(source, approved):
    C.root()
    if not approved:
        raise ValueError('explicit payout-key import flag required')
    C.protected(STORE, directory=True, private=True)
    C.protected(HOST_KEY, private=True)  # Never replace/initialize the host key.
    C.protected(source, private=True)
    if source.stat().st_size > 8192:
        raise ValueError('payout key file exceeds the input bound')
    key = token(source.read_bytes())
    fingerprint = hashlib.sha256(key).hexdigest()
    created = not TARGET.exists() and not TARGET.is_symlink()
    if created:
        encrypted = crypt('encrypt', key)
        if not encrypted or not hmac.compare_digest(crypt('decrypt', encrypted), key):
            raise ValueError('payout encryption round-trip mismatch; nothing installed')
        # Exclusive creation cannot replace another key/financial binding.
        with TARGET.open('xb') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(encrypted)
            stream.flush()
            os.fsync(stream.fileno())
        C.TX.sync_directory(STORE)
    else:
        C.protected(TARGET, private=True)
        if TARGET.stat().st_size > 65536 or not hmac.compare_digest(crypt('decrypt', TARGET.read_bytes()), key):
            raise ValueError('different payout credential already exists; reviewed rotation required, nothing replaced')
    return {'created': created, 'credential_fingerprint': fingerprint,
            'credential': str(TARGET), 'permissions_checked': False,
            'services_changed': False, 'api_calls': 0, 'payments_sent': 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--key-file', type=Path, required=True)
    parser.add_argument('--import-key', action='store_true', required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(args.key_file, args.import_key), sort_keys=True))


if __name__ == '__main__':
    main()
