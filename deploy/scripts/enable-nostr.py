#!/usr/bin/env python3
"""Enable operator-approved public payment/feed posts without replaying history.

Default is a read-only plan. --apply briefly quiesces the existing canary,
requires its message cursor caught up and no unpublished outbox, backs up all
paid state, and changes only service.mode to active. No credit or test note is
created, no home switch is written, and no database is restored or reset.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import stat
import time
import tomllib
import urllib.request

spec = importlib.util.spec_from_file_location(
    'household', Path(__file__).with_name('configure-household-nostr.py'))
household = importlib.util.module_from_spec(spec)
spec.loader.exec_module(household)


def state():
    with sqlite3.connect('file:/var/lib/lightning-goats/lightning-goats.db?mode=ro', uri=True) as db:
        cursor, latest = db.execute(
            'SELECT (SELECT last_event_seq FROM message_cursor WHERE singleton=1),'
            'COALESCE(MAX(seq),0) FROM event_log').fetchone()
        pending = db.execute(
            "SELECT COUNT(*) FROM message_outbox WHERE status != 'published'").fetchone()[0]
    return {'message_cursor': cursor, 'latest_event': latest, 'unpublished_outbox': pending}


def active_configuration(before):
    config = tomllib.loads(before.decode())
    if config['service']['mode'] != 'canary':
        raise ValueError('expected canary configuration')
    section = re.search(r'(?ms)^\[service\]\s*\n(.*?)(?=^\[|\Z)', before.decode())
    if section is None:
        raise ValueError('missing service section')
    body, count = re.subn(r'(?m)^mode\s*=\s*"canary"\s*$', 'mode = "active"', section.group(1))
    if count != 1:
        raise ValueError('unexpected service mode definition')
    after = (before.decode()[:section.start(1)] + body + '\n' + before.decode()[section.end(1):]).encode()
    checked = tomllib.loads(after.decode())
    checked['service']['mode'] = 'canary'
    if checked != config:
        raise ValueError('configuration changed beyond service.mode')
    return after


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError('run as root')
    os.umask(0o077)
    metadata = household.protected(household.CONFIG)
    before = household.CONFIG.read_bytes()
    config = tomllib.loads(before.decode())
    household.run('systemctl', 'is-active', '--quiet', household.UNIT)
    if config['service']['mode'] == 'active':
        print(json.dumps({'public_nostr_enabled': True, 'changed': False, **state()}))
        return
    after = active_configuration(before)
    if config['nostr']['relays'] != household.PUBLICATION_RELAYS or config['nostr']['bunker_relays'] != [household.RELAY]:
        raise ValueError('expected qualified household signing and selected announcement relays')
    credential = Path('/run/credentials') / household.UNIT / 'nostr-client-key'
    if not credential.exists():
        raise ValueError('client credential is not loaded in the existing service')
    if state()['unpublished_outbox']:
        raise ValueError('review existing unpublished messages before activation')
    print(json.dumps({'action': 'enable' if args.apply else 'plan', **state(),
                      'publication_relays': config['nostr']['relays']}), flush=True)
    if not args.apply:
        return
    household.paused()
    backup = Path('/var/backups/lightning-goats') / time.strftime('nostr-enable-%Y%m%dT%H%M%SZ', time.gmtime())
    backup.mkdir(mode=0o700)
    shutil.copytree('/etc/lightning-goats', backup / 'configuration')
    try:
        # Canary consumes preactivation events without signing. If shutdown
        # catches one final event, let that same mode finish it before retrying.
        for attempt in range(3):
            household.run('systemctl', 'stop', household.UNIT)
            boundary = state()
            if boundary['unpublished_outbox']:
                raise ValueError('unpublished outbox appeared; review before activation')
            if boundary['message_cursor'] == boundary['latest_event']:
                break
            household.run('systemctl', 'start', household.UNIT)
            time.sleep(0.75)
        else:
            raise ValueError('canary cursor did not catch up; activation not performed')
        shutil.copytree('/var/lib/lightning-goats', backup / 'state')
        with sqlite3.connect('file:/var/lib/lightning-goats/lightning-goats.db?mode=ro', uri=True) as db:
            if db.execute('PRAGMA quick_check').fetchall() != [('ok',)]:
                raise ValueError('database integrity check failed')
        status = household.run('/usr/local/bin/lightning-goatsctl', '--config', str(household.CONFIG), 'status').stdout
        if 'unresolved_feed_attempt_id=none\n' not in status:
            raise ValueError('wait for the original pending feeding outcome')
        household.paused()
        if household.CONFIG.read_bytes() != before:
            raise ValueError('configuration changed concurrently')
        household.atomic_write(household.CONFIG, after, stat.S_IMODE(metadata.st_mode))
        household.run('systemctl', 'start', household.UNIT)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        healthy = False
        for attempt in range(20):
            try:
                with opener.open('http://127.0.0.1:8787/healthz', timeout=2) as response:
                    healthy = response.status == 200
                if healthy:
                    break
            except OSError:
                pass
            time.sleep(0.5)
        if not healthy:
            raise ValueError('active daemon health check failed')
        record = {'public_nostr_enabled': True, 'cutoff_event_seq': boundary['latest_event'],
                  'backup': str(backup), 'publication_relays': config['nostr']['relays'],
                  'bunker_relays': config['nostr']['bunker_relays'], 'created_credits': 0,
                  'created_test_notes': 0, 'activated_at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}
        household.atomic_write(Path('/etc/lightning-goats/NOSTR-ACTIVATION.json'),
                               (json.dumps(record, indent=2) + '\n').encode(), 0o644)
        print(json.dumps(record))
        print(household.run('/usr/local/bin/lightning-goatsctl', '--config', str(household.CONFIG), 'status').stdout, end='')
    except Exception:
        # Configuration-only rollback; financial/event/outbox state is retained.
        household.run('systemctl', 'stop', household.UNIT)
        current = household.CONFIG.read_bytes()
        if current == after:
            household.atomic_write(household.CONFIG, before, stat.S_IMODE(metadata.st_mode))
        household.run('systemctl', 'start', household.UNIT)
        raise


if __name__ == '__main__':
    main()
