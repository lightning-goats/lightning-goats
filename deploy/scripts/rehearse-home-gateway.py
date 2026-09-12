#!/usr/bin/env python3
"""Read-only HTTP rehearsal of installed gateway with a SYNTHETIC invalid token.

Default plan; --apply creates a separate ephemeral system unit, then stops/removes
only that unit/config/ciphertext. Preserves its SQLite evidence. Never POSTs to a
gateway or OpenHAB. Does not start installed production/canary units.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import urllib.error
import urllib.request

NAME = 'lightning-goats-gateway-validation-v2'
RUN = Path('/run') / NAME
STATE = Path('/var/lib') / NAME
UNIT = Path('/run/systemd/system') / (NAME + '.service')


def run():
    if os.geteuid() != 0:
        raise ValueError('--apply requires root')
    for path in (RUN, STATE, UNIT):
        if path.exists() or path.is_symlink():
            raise ValueError(f'existing validation evidence/resource: {path}')
    config = Path('/etc/lightning-goats-gateway-canary/config.toml').read_text()
    import tomllib
    parsed = tomllib.loads(config)
    if parsed['service']['listen'] != '127.0.0.1:8790' or parsed['openhab']['protocol'] != 'uuid_canary':
        raise ValueError('unexpected canary configuration')
    config = config.replace('127.0.0.1:8790', '127.0.0.1:18790').replace(
        '/var/lib/lightning-goats-gateway-canary/', str(STATE) + '/')
    unit = Path('/etc/systemd/system/lightning-goats-gateway-canary.service').read_text()
    unit = unit.replace('/etc/lightning-goats-gateway-canary/config.toml', str(RUN / 'config.toml'))
    unit = unit.replace('/etc/credstore.encrypted/lightning-goats-gateway-canary-openhab', str(RUN / 'synthetic.cred'))
    unit = unit.replace('StateDirectory=lightning-goats-gateway-canary', 'StateDirectory=' + NAME)
    unit = unit.replace('RuntimeDirectory=lightning-goats-gateway-canary', 'RuntimeDirectory=' + NAME + '-service')
    RUN.mkdir(mode=0o755)
    (RUN / 'config.toml').write_text(config)
    (RUN / 'config.toml').chmod(0o644)
    result = {}
    try:
        subprocess.run(['systemd-creds', 'encrypt', '--with-key=host', '--name=openhab-token',
                        '-', str(RUN / 'synthetic.cred')], input=b'invalid-synthetic-home-validation-token',
                       check=True, stdout=subprocess.DEVNULL)
        UNIT.write_text(unit)
        subprocess.run(['systemctl', 'daemon-reload'], check=True)
        subprocess.run(['systemctl', 'start', UNIT.name], check=True)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        for attempt in range(30):
            try:
                opener.open('http://127.0.0.1:18790/healthz', timeout=1).close()
                break
            except OSError:
                time.sleep(0.2)
        for route in ('/healthz', '/v1/feeder/override', '/v1/temperature', '/v1/weather'):
            try:
                response = opener.open('http://127.0.0.1:18790' + route, timeout=8)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                result[route] = {'http': response.code, 'body': json.loads(response.read(65536))}
        if result['/healthz']['http'] != 200 or result['/v1/feeder/override']['http'] != 502:
            raise ValueError('liveness or invalid-credential fail-closed rehearsal failed')
        result['service_properties'] = subprocess.check_output(
            ['systemctl', 'show', UNIT.name, '-p', 'User', '-p', 'Group', '-p', 'NoNewPrivileges',
             '-p', 'CapabilityBoundingSet', '-p', 'ProtectSystem'], text=True).splitlines()
    finally:
        if UNIT.exists():
            subprocess.run(['systemctl', 'stop', UNIT.name], check=True)
            UNIT.unlink()
            subprocess.run(['systemctl', 'daemon-reload'], check=True)
        for name in ('config.toml', 'synthetic.cred'):
            (RUN / name).unlink(missing_ok=True)
        RUN.rmdir()
    original = STATE / 'gateway.db'
    backup = STATE / 'gateway.backup.db'
    restored = STATE / 'gateway.restored.db'
    with sqlite3.connect(original.as_uri() + '?mode=ro', uri=True) as source:
        result['sqlite_integrity'] = source.execute('PRAGMA integrity_check').fetchone()[0]
        result['sqlite_journal'] = source.execute('PRAGMA journal_mode').fetchone()[0]
        result['request_count'] = source.execute('SELECT count(*) FROM feeder_requests').fetchone()[0]
        with sqlite3.connect(backup) as target:
            source.backup(target)
            with sqlite3.connect(restored) as destination:
                target.backup(destination)
                assert list(source.iterdump()) == list(destination.iterdump())
    for path in (backup, restored):
        path.chmod(0o600)
    result['restore_exact_sql_dump'] = True
    result['synthetic_credential_only'] = True
    result['no_command_requests'] = True
    result['binary_sha256'] = hashlib.sha256(Path('/usr/local/bin/lightning-goats-gateway').read_bytes()).hexdigest()
    result['state_preserved_at'] = str(STATE)
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    if parser.parse_args().apply:
        run()
    else:
        print(json.dumps({'unit': UNIT.name, 'listen': '127.0.0.1:18790', 'apply': False,
                          'credential': 'synthetic invalid only', 'http_methods': ['GET']}))
