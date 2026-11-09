#!/usr/bin/env python3
"""Stage the qualified household client connection, retaining canary mode.

Default prints a plan; --apply backs up quiesced state/config and restarts the
same daemon with only the client credential, Primal signing transport, and the
operator-selected Primal, Chaima, Damus and ArtX publication relays.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import stat
import subprocess
import time
import tomllib
import urllib.request

CONFIG = Path('/etc/lightning-goats/config.receive.toml')
UNIT = 'lightning-goats-receive.service'
DIRECTORY = Path('/etc/systemd/system') / (UNIT + '.d')
DROPIN = DIRECTORY / '50-household-nostr.conf'
CREDENTIAL = Path('/etc/credstore.encrypted/lightning-goats-nostr-client')
RELAY = 'wss://relay.primal.net'
PUBLICATION_RELAYS = [RELAY, 'wss://nostr.chaima.info/', 'wss://relay.damus.io/',
                      'wss://relay.artx.market/']
DROPIN_TEXT = ('[Service]\n# NIP-46 client only; signing key stays on the household host.\n'
               'LoadCredentialEncrypted=nostr-client-key:' + str(CREDENTIAL) + '\n')


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True)


def protected(path, directory=False):
    metadata = path.lstat()
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    if not kind(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
        raise ValueError('unexpected ownership/permissions/type: ' + str(path))
    return metadata


def paused():
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open('http://10.8.0.6:8789/v1/feeder/override', timeout=5) as response:
        body = response.read(4097)
        if len(body) > 4096 or json.loads(body).get('override_enabled') is not True:
            raise ValueError('retain local FeederOverride ON during this setup')


def atomic_write(path, data, mode):
    temporary = path.with_name(path.name + '.nostr-pending')
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    os.fchmod(descriptor, mode)
    with os.fdopen(descriptor, 'wb') as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())
    os.replace(temporary, path)
    run('restorecon', str(path))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError('run as root')
    os.umask(0o077)
    metadata = protected(CONFIG)
    if stat.S_IMODE(protected(CREDENTIAL).st_mode) != 0o600:
        raise ValueError('client credential must be mode 0600')
    before = CONFIG.read_bytes()
    config = tomllib.loads(before.decode())
    if config['service']['mode'] != 'canary':
        raise ValueError('expected canary; public activation is a separate step')
    if config['nostr']['bunker_pubkey'] != '669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6':
        raise ValueError('unexpected signing identity')
    if config['database']['url'] != 'sqlite:///var/lib/lightning-goats/lightning-goats.db':
        raise ValueError('unexpected database location')
    section = re.search(r'(?ms)^\[nostr\]\s*\n(.*?)(?=^\[|\Z)', before.decode())
    if section is None:
        raise ValueError('missing Nostr section')
    replacement, count = re.subn(r'(?ms)^relays\s*=\s*\[[^\]]*\]',
                                'relays = ' + json.dumps(PUBLICATION_RELAYS), section.group(1))
    if count != 1:
        raise ValueError('unexpected relay definition')
    if 'bunker_relays' in config['nostr']:
        replacement, count = re.subn(r'(?ms)^bunker_relays\s*=\s*\[[^\]]*\]',
                                    'bunker_relays = [' + json.dumps(RELAY) + ']', replacement)
        if count != 1:
            raise ValueError('unexpected bunker relay definition')
    else:
        replacement += '\nbunker_relays = [' + json.dumps(RELAY) + ']\n'
    after = (before.decode()[:section.start(1)] + replacement + before.decode()[section.end(1):]).encode()
    checked = tomllib.loads(after.decode())
    checked['nostr']['relays'] = config['nostr']['relays']
    if 'bunker_relays' in config['nostr']:
        checked['nostr']['bunker_relays'] = config['nostr']['bunker_relays']
    else:
        checked['nostr'].pop('bunker_relays')
    if checked != config:
        raise ValueError('unexpected configuration change')
    if DIRECTORY.exists():
        protected(DIRECTORY, directory=True)
    if DROPIN.exists() or DROPIN.is_symlink():
        protected(DROPIN)
        if DROPIN.read_text() != DROPIN_TEXT:
            raise ValueError('existing Nostr drop-in requires review')
    run('systemctl', 'is-active', '--quiet', UNIT)
    state = run('/usr/local/bin/lightning-goatsctl', '--config', str(CONFIG), 'status').stdout
    if 'unresolved_feed_attempt_id=none\n' not in state:
        raise ValueError('wait for the original pending feeding outcome')
    print(json.dumps({'action': 'apply' if args.apply else 'plan', 'bunker_relays': [RELAY],
                      'publication_relays': PUBLICATION_RELAYS,
                      'mode_preserved': 'canary', 'public_nostr_enabled': False}), flush=True)
    if not args.apply:
        return
    paused()
    backup = Path('/var/backups/lightning-goats') / time.strftime('household-nostr-%Y%m%dT%H%M%SZ', time.gmtime())
    backup.mkdir(mode=0o700)
    shutil.copytree('/etc/lightning-goats', backup / 'configuration')
    if DIRECTORY.exists():
        shutil.copytree(DIRECTORY, backup / 'unit-dropins')
    run('systemctl', 'stop', UNIT)
    shutil.copytree('/var/lib/lightning-goats', backup / 'state')
    with sqlite3.connect('file:/var/lib/lightning-goats/lightning-goats.db?mode=ro', uri=True) as connection:
        if connection.execute('PRAGMA quick_check').fetchall() != [('ok',)]:
            raise ValueError('database check failed; state retained, inspect service')
    quiesced = run('/usr/local/bin/lightning-goatsctl', '--config', str(CONFIG), 'status').stdout
    if 'unresolved_feed_attempt_id=none\n' not in quiesced:
        raise ValueError('pending feeding detected; state retained, reconcile original UUID')
    paused()
    if CONFIG.read_bytes() != before:
        raise ValueError('configuration changed concurrently; inspect before restart')
    atomic_write(CONFIG, after, stat.S_IMODE(metadata.st_mode))
    if not DIRECTORY.exists():
        DIRECTORY.mkdir(mode=0o755)
        DIRECTORY.chmod(0o755)
    if not DROPIN.exists():
        atomic_write(DROPIN, DROPIN_TEXT.encode(), 0o644)
    run('systemctl', 'daemon-reload')
    run('systemctl', 'start', UNIT)
    run('systemctl', 'is-active', '--quiet', UNIT)
    print(run('/usr/local/bin/lightning-goatsctl', '--config', str(CONFIG), 'status').stdout, end='')
    print(json.dumps({'backup': str(backup), 'public_nostr_enabled': False}))


if __name__ == '__main__':
    main()
