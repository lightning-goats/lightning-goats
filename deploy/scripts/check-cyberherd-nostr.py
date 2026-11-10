#!/usr/bin/env python3
"""Signing-only household checks as each non-admin CyberHerd client.

Run in a temporary sandboxed systemd unit with its own encrypted client
credential and private runtime directory. No publication, invoice, provider,
application journal or arbitrary signing input is supported.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import selectors
import stat
import subprocess
import sys
import time
from urllib.parse import urlencode
import uuid


NAK = Path('/usr/local/bin/nak')
DIGEST = 'b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9'
AUTHOR = '669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6'
# Publicly known synthetic recipient (scalar 3); never contacted or paid.
RECIPIENT = 'f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9'
ROLES = {'membership': ('cyberherd', (1, 1311)), 'payout': ('cyberherd-signer', (9734,))}
ROLES['receive'] = ('lightning-goats', (1,))  # Existing client control, never shared across users.
RELAYS = ('wss://relay.primal.net', 'wss://nos.lol', 'wss://relay.damus.io',
          'wss://nostr.chaima.info/', 'wss://relay.artx.market/')
LIMIT = 65536


class Refusal(ValueError):
    """Only deliberately sanitized messages may reach the terminal."""


def safe_trace(stderr, expected_client_key):
    """Private helper diagnostics stay in memory; return only fixed labels/bools."""
    selected = re.findall(rb'with client key ([0-9a-f]{64})', stderr)
    lowered = stderr.lower()
    labels = ('unauthorized', 'auth-required', 'auth required', 'forbidden',
              'error from bunker', 'failed to connect', 'connect request',
              'failed to sign', 'sign_event', 'get_public_key', 'handshake',
              'response error: unauthorized', 'unexpected status', 'bad handshake',
              '401', '403', 'connection refused', 'deadline', 'timeout')
    return {'nak_client_key_used_matches': (all(value == expected_client_key for value in selected)
                                          if selected else None),
            'helper_categories': [label for label in labels if label.encode() in lowered]}


def run(arguments, data, environment, deadline=45, expected_client_key=None):
    """Never expose child output; concurrently bound both pipes and time."""
    if len(data) > 4096:
        raise Refusal('signing-check input exceeds bound')
    process = subprocess.Popen(arguments, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, env=environment)
    streams = {'stdout': bytearray(), 'stderr': bytearray()}
    def detail():
        if expected_client_key is None:
            return ''
        return '; diagnostic=' + json.dumps(safe_trace(bytes(streams['stderr']), expected_client_key), sort_keys=True)
    try:
        # This bounded input is below a normal pipe's capacity. The pinned
        # helper consumes it before producing key/event output.
        process.stdin.write(data)
        process.stdin.close()
        end = time.monotonic() + deadline
        with selectors.DefaultSelector() as selected:
            selected.register(process.stdout, selectors.EVENT_READ, 'stdout')
            selected.register(process.stderr, selectors.EVENT_READ, 'stderr')
            while selected.get_map():
                remaining = end - time.monotonic()
                if remaining <= 0:
                    raise Refusal('NIP-46 check timed out; child output withheld' + detail())
                for item, _ in selected.select(min(remaining, .2)):
                    chunk = os.read(item.fd, 4096)
                    if not chunk:
                        selected.unregister(item.fileobj)
                    else:
                        streams[item.data].extend(chunk)
                        if len(streams[item.data]) > LIMIT:
                            raise Refusal('NIP-46 check output exceeds bound; output withheld')
            try:
                status = process.wait(timeout=max(.01, end - time.monotonic()))
            except subprocess.TimeoutExpired:
                raise Refusal('NIP-46 check timed out; child output withheld' + detail()) from None
        if status:
            lowered = bytes(streams['stderr']).lower()
            category = next((s for s in ('unauthorized', 'auth-required', 'timeout', 'deadline',
                                         'permission denied', 'connection refused') if s.encode() in lowered), 'helper failure')
            raise Refusal('NIP-46 check failed: ' + category + '; child output withheld' + detail())
        if expected_client_key is not None and safe_trace(bytes(streams['stderr']), expected_client_key)['nak_client_key_used_matches'] is not True:
            raise Refusal('nak client selection was not positively verified; output withheld' + detail())
        return bytes(streams['stdout'])
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        if not process.stdin.closed:
            process.stdin.close()
        process.stdout.close()
        process.stderr.close()


def challenge(kind, nonce, timestamp):
    if kind not in (1, 1311, 9734) or str(uuid.UUID(nonce)) != nonce or timestamp < 0:
        raise Refusal('invalid fixed-purpose challenge')
    tags = [['challenge', nonce]]
    if kind == 1311:
        tags.append(['a', '30311:' + AUTHOR + ':signing-only-' + nonce, '', 'root'])
    elif kind == 9734:
        tags.extend([['p', RECIPIENT], ['amount', '1000'], ['relays', 'wss://relay.damus.io/']])
    return {'kind': kind, 'created_at': timestamp, 'tags': tags,
            'content': 'Lightning Goats signing-only connection check; never publish or pay. ' + nonce}


def verify_bytes(raw, expected):
    try:
        signed = json.loads(raw)
        if (not isinstance(signed, dict) or set(signed) != {'pubkey', 'kind', 'created_at', 'content', 'tags', 'id', 'sig'}
                or type(signed.get('kind')) is not int or type(signed.get('created_at')) is not int
                or signed.get('pubkey') != AUTHOR
                or any(signed.get(k) != v for k, v in expected.items())
                or re.fullmatch('[0-9a-f]{128}', signed.get('sig', '')) is None):
            raise Refusal()
        canonical = json.dumps([0, AUTHOR, expected['created_at'], expected['kind'],
                                expected['tags'], expected['content']],
                               ensure_ascii=False, separators=(',', ':')).encode()
        if signed.get('id') != hashlib.sha256(canonical).hexdigest():
            raise Refusal()
    except (ValueError, TypeError, KeyError):
        raise Refusal('remote signer changed or malformed the frozen challenge; output withheld') from None


def check(role, client, relays, diagnostics=False):
    user, kinds = ROLES[role]
    identity = pwd.getpwnam(user)
    if (os.geteuid() != identity.pw_uid or os.getegid() != identity.pw_gid or os.geteuid() == 0
            or re.fullmatch('[0-9a-f]{64}', client) is None or client == AUTHOR
            or not relays or len(relays) > len(RELAYS) or len(set(relays)) != len(relays)
            or any(r not in RELAYS for r in relays)):
        raise Refusal('separate non-admin role and pinned public client/relays required')
    info = NAK.lstat()
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022
            or hashlib.sha256(NAK.read_bytes()).hexdigest() != DIGEST):
        raise Refusal('protected pinned nak required')
    directory = Path(os.environ['RUNTIME_DIRECTORY'])
    info = directory.lstat()
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != identity.pw_uid or info.st_mode & 0o077
            or directory.resolve(strict=True) != directory):
        raise Refusal('private canonical role runtime directory required')
    credentials = Path(os.environ['CREDENTIALS_DIRECTORY'])
    if {p.name for p in credentials.iterdir()} != {'nostr-client-key'}:
        raise Refusal('only the selected NIP-46 client credential is permitted')
    raw = (credentials / 'nostr-client-key').read_bytes()
    if len(raw) > 128 or re.fullmatch(rb'[0-9a-f]{64}', raw.strip()) is None:
        raise Refusal('invalid NIP-46 authentication credential')
    key = raw.strip()
    base = [str(NAK), '--config-path', str(directory / 'nak')]
    clean = {'NO_COLOR': '1'}
    try:
        derived = run([*base, 'key', 'public'], key + b'\n', clean).decode().strip()
    except UnicodeError:
        raise Refusal('invalid public-client response; child output withheld') from None
    if derived != client or derived == AUTHOR:
        raise Refusal('credential differs from the pinned public client')
    bunker = 'bunker://' + AUTHOR + '?' + urlencode([('relay', r) for r in relays])
    proofs = []
    for kind in kinds:
        expected = challenge(kind, str(uuid.uuid4()), int(time.time()))
        # No relay arguments: nak signs through NIP-46 and never publishes the
        # signed event. The bunker transport may exchange kind-24133 envelopes.
        try:
            if diagnostics:
                print(json.dumps({'stage': 'sign', 'kind': kind, 'verified_kinds': proofs}), file=sys.stderr, flush=True)
            # Verbose output is captured only in private pipes. Pinned nak logs the actual
            # connect-as key; compare in memory and emit only a boolean. Never
            # forward/store that log, even when a connection or signature fails.
            signed = run([*base, *(['--verbose'] if diagnostics else []), 'event'],
                         json.dumps(expected, ensure_ascii=False).encode() + b'\n',
                         {**clean, 'NOSTR_SECRET_KEY': bunker, 'NOSTR_CLIENT_KEY': key.decode()},
                         **({'expected_client_key': key} if diagnostics else {}))
            verify_bytes(signed, expected)
            run([*base, 'verify'], signed + b'\n', clean)
        except Refusal as error:
            raise Refusal('kind=' + str(kind) + '; verified_kinds=' + json.dumps(proofs) + '; ' + str(error)) from None
        proofs.append(kind)
    del key, raw
    return {'accepted': True, 'role': role, 'client_public_key': client, 'kinds_verified': proofs,
            'exact_author_bytes_hash_and_signature': True, 'relays': relays,
            'project_events_published': 0, 'invoice_requests': 0, 'provider_operations': 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--role', choices=ROLES, required=True)
    parser.add_argument('--client-public-key', required=True)
    parser.add_argument('--relay', choices=RELAYS, action='append')
    parser.add_argument('--diagnostics', action='store_true', help='emit only fixed stage labels and client-key-match booleans')
    args = parser.parse_args()
    try:
        print(json.dumps(check(args.role, args.client_public_key, args.relay or ['wss://relay.primal.net'], args.diagnostics), sort_keys=True))
    except Exception as error:
        # Filesystem/helper errors can include paths or sensitive responses.
        # ValueError messages above are deliberately bounded and sanitized.
        detail = str(error) if isinstance(error, Refusal) else type(error).__name__
        raise SystemExit('Signing-only check refused: ' + detail) from None


if __name__ == '__main__':
    main()
