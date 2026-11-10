#!/usr/bin/env python3
"""Read-only household inspection; print only status and public-key comparisons.

Never open a signing credential file, process environment or journal. Process
arguments are filtered in memory; no raw argument or pairing URI is emitted.
Never restart a unit or modify a file.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


UNIT = 'nostr-bunker@earthship-operator.service'
ENVIRONMENT = Path('/etc/nostr-bunker/earthship-operator.env')
PROC = Path('/proc')
CLIENTS = {
    'earthship': '5302cd2bfe2dcc76c5a9abcba74e6c5f1a07444b35c25e0cd74bac5b453fb5f6',
    'receive': 'd7a195216dec31cbdd3a98ab5d99b2865a7e74c7ad6d83144570f98b681c8256',
    'membership': 'df7ffa14b600519ddb726817e7ebc41559cc141b0233a3214161cf3d01d2e2cd',
    'payout_signer': '94e5cffdc73bec1c0e4578434a7be30cfd055fc07c5a4c5038280b3e3e21dfda',
}


def public_clients(arguments):
    keys, malformed = set(), 0
    for index, argument in enumerate(arguments):
        value = None
        if argument in ('--authorized-keys', '-k'):
            value = arguments[index + 1] if index + 1 < len(arguments) else ''
        elif argument.startswith('--authorized-keys='):
            value = argument.split('=', 1)[1]
        if value is not None:
            if re.fullmatch('[0-9a-fA-F]{64}', value):
                keys.add(value.lower())
            else:
                malformed += 1
    return keys, malformed


def status(unit):
    result = subprocess.run(['/usr/bin/systemctl', 'show', unit, '-p', 'Id', '-p', 'MainPID',
                             '-p', 'ActiveState', '-p', 'SubState', '-p', 'NRestarts', '-p', 'ExecMainStatus'],
                            capture_output=True, timeout=10, check=True)
    if len(result.stdout) > 8192:
        raise ValueError('service status exceeds bound')
    values = dict(line.split('=', 1) for line in result.stdout.decode().splitlines())
    if (values.get('Id') != unit or set(values) != {'Id', 'MainPID', 'ActiveState', 'SubState', 'NRestarts', 'ExecMainStatus'}
            or not all(re.fullmatch('[0-9]{1,12}', values[n]) for n in ('MainPID', 'NRestarts', 'ExecMainStatus'))):
        raise ValueError('service status is incomplete')
    return values


def bounded(path):
    with path.open('rb') as stream:
        raw = stream.read(65537)
    if len(raw) > 65536:
        raise ValueError('inspection input exceeds bound')
    return raw


def inspect():
    if os.geteuid() != 0:
        raise ValueError('run with sudo on the household host')
    before = status(UNIT)
    report = {'service_active': before['ActiveState'] == 'active' and before['SubState'] == 'running',
              'automatic_restarts': int(before['NRestarts']), 'main_exit_status': int(before['ExecMainStatus']),
              'changes_made': False, 'credential_files_read': False,
              'process_environment_read': False, 'raw_arguments_emitted': False}
    text = bounded(ENVIRONMENT).decode()
    matches = re.findall(r'^BUNKER_AUTHORIZED_CLIENTS="([0-9a-fA-F ]+)"$', text, re.MULTILINE)
    configured = set(matches[0].lower().split()) if len(matches) == 1 else set()
    report['configured_clients'] = {name: key in configured for name, key in CLIENTS.items()}
    if not report['service_active'] or int(before['MainPID']) == 0:
        report['running_client_inspection'] = 'service is not running'
        return report
    process = PROC / before['MainPID']
    raw = bounded(process / 'cmdline')
    if not raw.endswith(b'\0'):
        raise ValueError('process arguments are incomplete')
    keys, malformed = public_clients(raw.decode().split('\0')[:-1])
    report['running_clients'] = {name: key in keys for name, key in CLIENTS.items()}
    report['running_public_client_count'] = len(keys)
    report['malformed_public_client_arguments'] = malformed
    digest = hashlib.sha256()
    with (process / 'exe').open('rb') as stream:
        for block in iter(lambda: stream.read(65536), b''):
            digest.update(block)
    report['running_program_sha256'] = digest.hexdigest()
    expected = re.findall(r'^NAK_SHA256="?([0-9a-f]{64})"?$', text, re.MULTILINE)
    report['running_program_matches_configured_digest'] = len(expected) == 1 and expected[0] == digest.hexdigest()
    after = status(UNIT)
    if after != before:
        raise ValueError('service changed during inspection; retry once stable')
    report['running_client_inspection'] = 'stable process inspected'
    return report


def main():
    try:
        print(json.dumps(inspect(), sort_keys=True))
    except Exception:
        # Unexpected decoding/process errors must never expose input bytes.
        raise SystemExit('Read-only inspection incomplete; no private input emitted.') from None


if __name__ == '__main__':
    main()
