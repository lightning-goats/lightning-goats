#!/usr/bin/env python3
"""Synthetic four-client NIP-46 trial; fresh loopback-only namespace required."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile
import time
from urllib.parse import urlencode
import uuid


spec = importlib.util.spec_from_file_location('bounded_check', Path(__file__).with_name('check-cyberherd-nostr.py'))
q = importlib.util.module_from_spec(spec)
spec.loader.exec_module(q)
SIGNER = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'


def check(nak, host_network_namespace):
    if (os.geteuid() == 0 or 'CREDENTIALS_DIRECTORY' in os.environ
            or re.fullmatch(r'net:\[[0-9]+\]', host_network_namespace) is None
            or os.readlink('/proc/self/ns/net') == host_network_namespace):
        raise q.Refusal('separate non-admin credential-free network namespace required')
    links = json.loads(q.run([shutil.which('ip'), '-j', 'link'], b'', {}, deadline=5))
    if len(links) != 1 or links[0]['ifname'] != 'lo':
        raise q.Refusal('only loopback may exist')
    nak = nak.resolve(strict=True)
    if hashlib.sha256(nak.read_bytes()).hexdigest() != q.DIGEST:
        raise q.Refusal('pinned nak required')
    processes = []
    with tempfile.TemporaryDirectory(prefix='lg-synthetic-multi-nak-') as temporary:
        root = Path(temporary)
        def start(args, environment):
            child = subprocess.Popen([str(nak), *args], env=environment, stdin=subprocess.DEVNULL,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            processes.append(child)
            return child
        try:
            with socket.socket() as selected:
                selected.bind(('127.0.0.1', 0))
                port = selected.getsockname()[1]
            url = 'ws://127.0.0.1:' + str(port)
            relay = start(['--config-path', str(root / 'relay'), 'serve', '--hostname', '127.0.0.1', '--port', str(port)], {'NO_COLOR': '1'})
            deadline = time.monotonic() + 10
            while True:
                if relay.poll() is not None or time.monotonic() > deadline:
                    raise q.Refusal('synthetic relay did not start')
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=.2):
                        break
                except OSError:
                    time.sleep(.05)
            # Publicly known scalars 1..6 only. No systemd or application key is read.
            keys = [format(i, '064x') for i in range(1, 7)]
            clients = [q.run([str(nak), 'key', 'public'], key.encode() + b'\n', {'NO_COLOR': '1'}).decode().strip() for key in keys]
            if clients[0] != SIGNER or len(set(clients)) != 6:
                raise q.Refusal('synthetic identities differ')
            arguments = ['--config-path', str(root / 'bunker'), 'bunker']
            for client in clients[1:5]:
                arguments.extend(['--authorized-keys', client])
            arguments.append(url)
            bunker = start(arguments, {'NO_COLOR': '1', 'NOSTR_SECRET_KEY': keys[0]})
            time.sleep(.5)
            if bunker.poll() is not None:
                raise q.Refusal('synthetic four-client bunker exited')
            q.AUTHOR = SIGNER  # Only this fixture module; never application configuration.
            def sign(index, kind):
                expected = q.challenge(kind, str(uuid.uuid4()), int(time.time()))
                environment = {'NO_COLOR': '1', 'NOSTR_SECRET_KEY': 'bunker://' + SIGNER + '?' + urlencode({'relay': url}),
                               'NOSTR_CLIENT_KEY': keys[index]}
                signed = q.run([str(nak), '--config-path', str(root / ('client-' + str(index))), '--verbose', 'event'],
                               json.dumps(expected).encode() + b'\n', environment, expected_client_key=keys[index].encode())
                q.verify_bytes(signed, expected)
                q.run([str(nak), 'verify'], signed + b'\n', {'NO_COLOR': '1'})
            for index in range(1, 5):
                for kind in (1, 1311, 9734):
                    sign(index, kind)
            try:
                sign(5, 1)
            except q.Refusal as error:
                if 'response error: unauthorized' not in str(error):
                    raise q.Refusal('unknown-client refusal was not a bunker authorization response') from None
            else:
                raise q.Refusal('unknown client was accepted')
            sign(1, 1)
            for kind in (1, 1311, 9734):
                events = q.run([str(nak), '--config-path', str(root / 'query'), 'req', '--kind', str(kind), url], b'', {'NO_COLOR': '1'})
                if events.strip():
                    raise q.Refusal('signing unexpectedly published a project event')
            return {'accepted': True, 'synthetic_clients_verified': 4, 'kinds_per_client': [1, 1311, 9734],
                    'unknown_client_rejected_by_bunker': True, 'authorized_client_still_works': True,
                    'project_events_published': 0, 'production_credentials_read': False, 'external_network': False}
        finally:
            for child in reversed(processes):
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=5)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nak', type=Path, required=True)
    parser.add_argument('--host-network-namespace', required=True)
    try:
        args = parser.parse_args()
        print(json.dumps(check(args.nak, args.host_network_namespace), sort_keys=True))
    except Exception as error:
        raise SystemExit(str(error) if isinstance(error, q.Refusal) else 'Synthetic check failed; child output withheld.') from None
