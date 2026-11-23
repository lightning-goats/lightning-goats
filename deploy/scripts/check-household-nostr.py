#!/usr/bin/env python3
"""NIP-46 signing-only check under the daemon UID and a systemd client credential.

Run through a temporary systemd unit; never publishes or prints a signed note.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.parse import urlencode
import uuid


NAK = '/usr/local/bin/nak'
DIGEST = 'b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9'
AUTHOR = '669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6'
CLIENT = 'd7a195216dec31cbdd3a98ab5d99b2865a7e74c7ad6d83144570f98b681c8256'
RELAYS = ['wss://nos.lol', 'wss://relay.primal.net', 'wss://relay.damus.io']


def run(arguments, data, environment):
    try:
        result = subprocess.run(arguments, input=data, capture_output=True,
                                env=environment, timeout=45)
    except subprocess.TimeoutExpired:
        raise ValueError('NIP-46 check timed out; subprocess output withheld') from None
    if result.returncode:
        diagnostic = result.stderr.decode(errors='replace').lower()
        categories = [word for word in ['permission denied', 'connection refused',
                      'timeout', 'deadline', 'unauthorized', 'auth-required',
                      'invalid', 'eof', 'websocket', 'no such file', 'failed',
                      'unexpected', 'bunker', 'relay', '401', '403', 'handshake',
                      'error from bunker', 'failed to connect', 'connect request',
                      'failed to sign'] if word in diagnostic]
        raise ValueError('nak check failed: exit=' + str(result.returncode) +
                         ', categories=' + ','.join(categories) +
                         '; subprocess output withheld')
    if len(result.stdout) > 65536 or len(result.stderr) > 65536:
        raise ValueError('nak check output exceeded the verification bound')
    return result.stdout


def main():
    import pwd
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--relay', choices=RELAYS, action='append')
    args = parser.parse_args()
    # Initial qualified household transport; other approved relays are opt-in probes.
    relays = args.relay or ['wss://relay.primal.net']
    if os.geteuid() != pwd.getpwnam('lightning-goats').pw_uid:
        raise ValueError('run as the non-admin lightning-goats runtime user')
    if hashlib.sha256(Path(NAK).read_bytes()).hexdigest() != DIGEST:
        raise ValueError('nak digest mismatch')
    directory = Path(os.environ['RUNTIME_DIRECTORY'])
    credential = Path(os.environ['CREDENTIALS_DIRECTORY']) / 'nostr-client-key'
    key = credential.read_text().strip()
    if not re.fullmatch(r'[0-9a-f]{64}', key):
        raise ValueError('invalid client credential')
    clean = {'NO_COLOR': '1'}
    base = [NAK, '--config-path', str(directory / 'nak')]
    # `nak key public` consumes piped stdin before the global key environment.
    public = run([*base, 'key', 'public'], (key + '\n').encode(), clean).decode().strip()
    if public != CLIENT:
        raise ValueError('client identity does not match household allowlist')
    challenge = str(uuid.uuid4())
    content = 'Lightning Goats signing-only connection check ' + challenge
    tags = [['t', 'LightningGoats'], ['challenge', challenge]]
    partial = {'kind': 1, 'content': content, 'tags': tags}
    bunker = 'bunker://' + AUTHOR + '?' + urlencode([('relay', relay) for relay in relays])
    started = int(time.time())
    signed_bytes = run([*base, 'event'], json.dumps(partial).encode(),
                       {**clean, 'NOSTR_SECRET_KEY': bunker, 'NOSTR_CLIENT_KEY': key})
    del key
    try:
        signed = json.loads(signed_bytes)
    except ValueError:
        raise ValueError('invalid signed-event response; output withheld') from None
    if any(signed.get(field) != expected for field, expected in
           [('pubkey', AUTHOR), ('kind', 1), ('content', content), ('tags', tags)]):
        raise ValueError('remote signature changed the expected identity or challenge')
    timestamp = signed.get('created_at')
    if not isinstance(timestamp, int) or not started - 5 <= timestamp <= time.time() + 5:
        raise ValueError('remote signature has an unexpected timestamp')
    canonical = json.dumps([0, AUTHOR, timestamp, 1, tags, content],
                           ensure_ascii=False, separators=(',', ':')).encode()
    if signed.get('id') != hashlib.sha256(canonical).hexdigest():
        raise ValueError('remote event id does not match the canonical challenge')
    run([*base, 'verify'], signed_bytes, clean)
    print(json.dumps({'remote_signing': 'verified', 'author': AUTHOR,
                      'client_public_key': CLIENT, 'relays': relays,
                      'runtime_uid': os.geteuid(), 'event_id': signed['id'],
                      'kind1_notes_published': 0}))


if __name__ == '__main__':
    main()
