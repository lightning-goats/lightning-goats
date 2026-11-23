#!/usr/bin/env python3
"""Exercise the candidate through real NIP-46 on a disposable loopback relay."""
import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import socket
import subprocess
import tempfile
import time
from urllib.parse import urlencode


STOCK = 'b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9'
STDIN_CLIENT = 'd1b7aca5c46d4d4eb5402ebd59ef48a6e9d83ce848f94493f8f46ef948b74822'
BASE = {'NO_COLOR': '1'}


def stop(child):
    if child.poll() is None:
        child.kill()
    child.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stock', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--candidate-sha256', required=True)
    parser.add_argument('--host-network-namespace', required=True)
    parser.add_argument('--stdin-client', type=Path)
    args = parser.parse_args()
    if (os.geteuid() == 0 or 'CREDENTIALS_DIRECTORY' in os.environ
            or re.fullmatch(r'net:\[[0-9]+\]', args.host_network_namespace) is None
            or os.readlink('/proc/self/ns/net') == args.host_network_namespace):
        raise ValueError('a separate non-admin network namespace is required')
    interfaces = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link'], timeout=5))
    if [link['ifname'] for link in interfaces] != ['lo']:
        raise ValueError('only loopback may exist')
    programs = [(args.stock, STOCK), (args.candidate, args.candidate_sha256)]
    if args.stdin_client is not None:
        programs.append((args.stdin_client, STDIN_CLIENT))
    for path, expected in programs:
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError('selected nak executable digest differs')

    repository = Path(__file__).resolve().parents[2]
    checker = runpy.run_path(str(repository / 'deploy/scripts/check-cyberherd-nostr.py'))
    run, verify = checker['run'], checker['verify_bytes']
    keys = [format(index, '064x') for index in range(1, 5)]
    owner, scoped, legacy, _ = [
        run([str(args.stock), 'key', 'public'], key.encode() + b'\n', BASE).decode().strip() for key in keys
    ]
    verify.__globals__['AUTHOR'] = owner

    with tempfile.TemporaryDirectory(prefix='lg-cashu-bunker-wire-') as temporary, ExitStack() as cleanup:
        root = Path(temporary)

        def start(binary, label, *tail, environment):
            child = subprocess.Popen([str(binary), '--config-path', str(root / label), *tail],
                                     env=environment, stdin=subprocess.DEVNULL,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            cleanup.callback(stop, child)
            return child

        with socket.socket() as reserved:
            reserved.bind(('127.0.0.1', 0))
            port = reserved.getsockname()[1]
        url = f'ws://127.0.0.1:{port}'
        relay = start(args.stock, 'relay', 'serve', '--hostname', '127.0.0.1', '--port', str(port), environment=BASE)
        for _ in range(100):
            if relay.poll() is not None:
                raise ValueError('test relay exited')
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1):
                    break
            except OSError:
                time.sleep(.05)
        else:
            raise ValueError('test relay did not start')

        bunker_env = {**BASE, 'NOSTR_SECRET_KEY': keys[0]}
        flag = ('bunker', '--cashu-client', scoped, '--authorized-keys')
        overlap = subprocess.run([str(args.candidate), '--config-path', str(root / 'overlap'), *flag, scoped, url],
                                 env=bunker_env, capture_output=True, timeout=5)
        assert overlap.returncode and b'already in the general authorization list' in overlap.stderr
        bunker = start(args.candidate, 'bunker', *flag, legacy, url, environment=bunker_env)
        time.sleep(.5)
        assert bunker.poll() is None
        rendezvous = 'bunker://' + owner + '?' + urlencode({'relay': url})

        def client(index, *tail, data=b''):
            env = {**BASE, 'NOSTR_SECRET_KEY': rendezvous, 'NOSTR_CLIENT_KEY': keys[index]}
            path = args.stdin_client or args.stock
            return run([str(path), '--config-path', str(root / f'client-{index}'), *tail], data, env, deadline=15)

        def crypto(action, peer, payload):
            flag = '--recipient-pubkey' if action == 'encrypt' else '--sender-pubkey'
            if args.stdin_client is not None:
                return client(1, action, '--stdin', flag, peer, data=payload.encode() + b'\n').decode().strip()
            return client(1, action, flag, peer, payload).decode().strip()

        def sign(index, kind, content='', tags=None):
            expected = {'kind': kind, 'created_at': int(time.time()), 'content': content, 'tags': tags or []}
            signed = client(index, 'event', data=json.dumps(expected).encode() + b'\n')
            verify(signed, expected)
            run([str(args.stock), 'verify'], signed, BASE)

        def denied(operation, *arguments):
            try:
                operation(*arguments)
            except checker['Refusal'] as error:
                assert 'unauthorized' in str(error)
            else:
                raise ValueError('out-of-scope request succeeded')

        witness = 'synthetic Cashu redemption history'
        encrypted = crypto('encrypt', owner, witness)
        assert encrypted != witness and crypto('decrypt', owner, encrypted) == witness
        sign(1, 10019, tags=[['mint', 'https://mint.example'], ['relay', 'wss://relay.example'], ['pubkey', '02' + scoped]])
        sign(1, 7376, encrypted, [['e', 'ab' * 32], ['p', scoped], ['amount', '7']])
        denied(sign, 1, 1)
        denied(sign, 1, 9734)
        denied(crypto, 'encrypt', legacy, witness)
        denied(crypto, 'decrypt', legacy, encrypted)
        sign(2, 1)
        denied(sign, 3, 10019)
        for kind in (1, 10019, 7376, 9734):
            assert not run([str(args.stock), 'req', '--kind', str(kind), url], b'', BASE).strip()
    print(json.dumps({'cashu_kinds_signed': [10019, 7376], 'nip44_self_encryption_and_decryption': True,
                      'outside_kinds_and_peers_denied': True, 'unknown_client_denied': True,
                      'overlap_refused': True, 'ordinary_client_kind_1_preserved': True,
                      'bounded_stdin_client': args.stdin_client is not None,
                      'project_events_published': 0, 'synthetic_only': True}, sort_keys=True))


if __name__ == '__main__':
    main()
