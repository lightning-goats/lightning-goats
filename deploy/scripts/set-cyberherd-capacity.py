#!/usr/bin/env python3
"""Set exactly three seats through NIP-98, using the existing household identity.

Run only as the non-admin membership identity in a private transient systemd
unit carrying its existing NIP-46 client credential. No journal, payment,
financial limit, public Nostr publication or generic operator command is exposed.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import re
import stat
import sys
import time
import urllib.error
import urllib.request
from urllib.parse import urlencode
import uuid

AUTHOR = '669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6'
ORIGIN = 'https://lightning-goats.com'
BASE = 'http://127.0.0.1:3340'
STATE = '/api/v1/cyberherd/operator/state'
COMMAND = '/api/v1/cyberherd/operator/command'


class Retryable(Exception):
    pass


class Refusal(ValueError):
    """Only fixed labels and already sanitized signer diagnostics may be shown."""


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def protected(path):
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise ValueError('canonical installed capacity client required')
    for index, part in enumerate([path, *path.parents]):
        info = part.lstat()
        if (info.st_uid != 0 or info.st_gid != 0 or info.st_mode & 0o022
                or (index == 0 and (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1))
                or (index != 0 and not stat.S_ISDIR(info.st_mode))):
            raise ValueError('protected installed capacity client required')


def update(call, pause=time.sleep):
    before = call('GET', STATE, b'')
    if before.get('owner') != AUTHOR or type(before.get('revision')) is not int:
        raise ValueError('unexpected operator state identity')
    settings = before['settings']
    if (type(settings.get('max_members')) is not int or type(settings.get('member_percent')) is not int
            or not 0 <= settings['member_percent'] <= 100
            or not isinstance(settings.get('minimum_sats'), str)
            or re.fullmatch('0|[1-9][0-9]{0,18}', settings['minimum_sats']) is None
            or int(settings['minimum_sats']) > 2**63 - 1):
        raise ValueError('canonical unchanged membership settings required')
    if settings['max_members'] == 3:
        return {'capacity': 3, 'changed': False, 'existing_remote_identity_used': True,
                'payment_or_feeding_requested': False, 'project_events_published': 0}
    command = {'id': str(uuid.uuid4()), 'expected_revision': before['revision'],
        'action': {'type': 'settings', 'minimum_sats': settings['minimum_sats'],
                   'max_members': 3, 'member_percent': settings['member_percent']}}
    body = json.dumps(command, sort_keys=True, separators=(',', ':')).encode()
    receipt = None
    for attempt in range(4):
        try:
            receipt = call('POST', COMMAND, body)
            break
        except Retryable:
            if attempt == 3:
                # Preserve the one original identity. Never issue a new command
                # to guess whether an unobserved response committed a mutation.
                raise ValueError('capacity command requires inspection; no replacement command issued') from None
            pause(1)
    if (not isinstance(receipt, dict) or receipt.get('command_id') != command['id']
            or type(receipt.get('changed')) is not bool or type(receipt.get('revision')) is not int):
        raise ValueError('exact capacity command receipt required')
    after = call('GET', STATE, b'')
    desired = dict(settings, max_members=3)
    if (after.get('owner') != AUTHOR or after.get('settings') != desired
            or after.get('revision') != receipt['revision'] or after.get('cycle') != before.get('cycle')
            or after.get('members') != before.get('members') or after.get('total_members') != before.get('total_members')):
        raise ValueError('capacity readback changed other membership state; inspect original receipt')
    return {'capacity': 3, 'changed': receipt['changed'], 'existing_remote_identity_used': True,
            'minimum_and_allocation_preserved': True, 'members_and_cycle_preserved': True,
            'payment_or_feeding_requested': False, 'project_events_published': 0}


def main(client):
    if not sys.flags.isolated or not sys.flags.no_site or not sys.dont_write_bytecode:
        raise ValueError('isolated installed client required')
    protected(Path(__file__).absolute())
    helper = Path(__file__).with_name('check-cyberherd-nostr.py')
    protected(helper)
    spec = importlib.util.spec_from_file_location('capacity_signing_support', helper)
    C = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(C)
    identity = pwd.getpwnam('cyberherd')
    if (os.geteuid() != identity.pw_uid or os.getegid() != identity.pw_gid or os.geteuid() == 0
            or re.fullmatch('[0-9a-f]{64}', client) is None or client == AUTHOR):
        raise ValueError('original non-admin membership client required')
    protected(C.NAK)
    if hashlib.sha256(C.NAK.read_bytes()).hexdigest() != C.DIGEST:
        raise ValueError('independently pinned nak required')
    directory = Path(os.environ['RUNTIME_DIRECTORY'])
    metadata = directory.lstat()
    if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != identity.pw_uid
            or metadata.st_mode & 0o077 or directory.resolve(strict=True) != directory):
        raise ValueError('private runtime directory required')
    credentials = Path(os.environ['CREDENTIALS_DIRECTORY'])
    if {p.name for p in credentials.iterdir()} != {'nostr-client-key'}:
        raise ValueError('one existing NIP-46 client credential required')
    credential = credentials / 'nostr-client-key'
    if not credential.is_file() or credential.is_symlink() or credential.stat().st_size > 128:
        raise ValueError('bounded regular client credential required')
    key = credential.read_bytes()
    if len(key) > 128 or re.fullmatch(rb'[0-9a-f]{64}', key.strip()) is None:
        raise ValueError('bounded NIP-46 client credential required')
    key = key.strip()
    nak = [str(C.NAK), '--config-path', str(directory / 'nak')]
    clean = {'NO_COLOR': '1'}
    if C.run([*nak, 'key', 'public'], key + b'\n', clean).decode().strip() != client:
        raise ValueError('existing public client binding differs')
    # The household signer uses Primal for its transport. Publication relay
    # selection belongs to the existing messaging services, not this HTTP proof.
    bunker = 'bunker://' + AUTHOR + '?' + urlencode([('relay', C.RELAYS[0])])
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def call(method, uri, body):
        if (method, uri) not in [('GET', STATE), ('POST', COMMAND)]:
            raise ValueError('capacity client route refused')
        tags = [['u', ORIGIN + uri], ['method', method]]
        if method == 'POST':
            tags.append(['payload', hashlib.sha256(body).hexdigest()])
        tags.append(['nonce', uuid.uuid4().hex])
        unsigned = {'kind': 27235, 'created_at': int(time.time()), 'content': '', 'tags': tags}
        try:
            raw = C.run([*nak, '--verbose', 'event'], json.dumps(unsigned).encode() + b'\n',
                        {**clean, 'NOSTR_SECRET_KEY': bunker, 'NOSTR_CLIENT_KEY': key.decode()},
                        expected_client_key=key)
        except C.Refusal as error:
            raise Refusal('household signing for ' + method + ' refused: ' + str(error)) from None
        C.verify_bytes(raw, unsigned)
        C.run([*nak, 'verify'], raw + b'\n', clean)
        headers = {'Authorization': 'Nostr ' + base64.b64encode(raw.strip()).decode(),
                   'Accept': 'application/json', 'Content-Type': 'application/json'}
        request = urllib.request.Request(BASE + uri, data=body if method == 'POST' else None,
                                         headers=headers, method=method)
        try:
            with opener.open(request, timeout=4) as response:
                value = response.read(131073)
                if (response.status != 200 or len(value) > 131072
                        or response.headers.get('Content-Type', '').split(';')[0].strip().lower() != 'application/json'):
                    raise ValueError('bounded native operator response required')
                return json.loads(value)
        except urllib.error.HTTPError as error:
            if method == 'POST' and error.code in (409, 429, 500, 502, 503, 504):
                raise Retryable() from None
            raise ValueError('operator HTTP request refused; authentication and response withheld') from None
        except (OSError, TimeoutError):
            if method == 'POST':
                raise Retryable() from None
            raise ValueError('operator read unavailable; response withheld') from None

    return update(call)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--client-public-key', required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(main(args.client_public_key), sort_keys=True))
    except Exception as error:
        detail = str(error) if isinstance(error, Refusal) else 'private signing/authentication output withheld'
        raise SystemExit('Capacity update refused: ' + detail) from None
