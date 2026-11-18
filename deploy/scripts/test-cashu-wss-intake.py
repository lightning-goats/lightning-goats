#!/usr/bin/env python3
"""Exercise the native wallet's WSS intake with a private relay and synthetic proofs."""
import argparse
import base64
from contextlib import closing, contextmanager
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import shutil
import socket
import sqlite3
import ssl
import struct
import subprocess
import tempfile
import time
import uuid

WALLET_TESTS = 'acquisition::tests::wss_receipt_fixture::'
SENDER = WALLET_TESTS + 'isolated_wss_sender'
WALLET_RECEIPT = WALLET_TESTS + 'isolated_wss_receipt_wallet'
CORE_RECEIPT = 'cashu::receipt_server::cross_uid_fixture::isolated_receipt_server_child'
RECEIPT_ROWS = 'SELECT receipt_digest,receipt_json,net_sats,original_feed_credit_sats FROM cashu_receipt_inbox'
ADDRESS = '93.184.216.34'
HOST = 'relay.nutzap-fixture.example'
UNTRUSTED = 'untrusted.nutzap-fixture.example'
WRONG = 'wrong.nutzap-fixture.example'
ENV = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}


def run(command, *, env=ENV):
    result = subprocess.run(command, env=env, stdin=subprocess.DEVNULL, capture_output=True,
                            text=True, timeout=15)
    if result.returncode:
        raise ValueError(f'{Path(command[0]).name} failed in WSS fixture: {result.stderr[-1200:]}')
    return result.stdout


def isolated_network():
    if os.geteuid() != 0 or any(os.readlink(f'/proc/self/ns/{kind}') == os.readlink(f'/proc/1/ns/{kind}')
                               for kind in ('mnt', 'net')):
        raise ValueError('requires root in private mount and network namespaces')
    ip = '/usr/sbin/ip'
    if [row['ifname'] for row in json.loads(run([ip, '-j', 'link', 'show']))] != ['lo']:
        raise ValueError('WSS fixture requires a loopback-only network')
    run(['/usr/bin/mount', '--make-rprivate', '/'])
    run([ip, 'addr', 'add', ADDRESS + '/32', 'dev', 'lo'])
    run([ip, 'link', 'set', 'lo', 'up'])


def certificate(root):
    authority, private_key = root / 'ca.pem', root / 'ca.key'
    server, server_key = root / 'relay.pem', root / 'relay.key'
    request, extensions = root / 'relay.csr', root / 'relay.ext'
    openssl = '/usr/bin/openssl'
    run([openssl, 'req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
         '-nodes', '-days', '1', '-subj', '/CN=Disposable Nutzap Relay CA', '-keyout',
         str(private_key), '-out', str(authority), '-addext', 'basicConstraints=critical,CA:TRUE'])
    run([openssl, 'req', '-new', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
         '-nodes', '-subj', f'/CN={HOST}', '-keyout', str(server_key), '-out', str(request)])
    extensions.write_text(f'subjectAltName=DNS:{HOST},DNS:{UNTRUSTED}\nextendedKeyUsage=serverAuth\n')
    run([openssl, 'x509', '-req', '-days', '1', '-in', str(request), '-CA', str(authority),
         '-CAkey', str(private_key), '-set_serial', '1', '-extfile', str(extensions), '-out', str(server)])
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(server, server_key)
    authority.chmod(0o444)
    return context, authority


def read_exact(reader, count):
    value = reader.read(count)
    if len(value) != count:
        raise ValueError('relay request ended early')
    return value


def subscription(stream, recipient, mint, created_at):
    with stream.makefile('rb') as reader:
        if reader.readline(1024) != b'GET / HTTP/1.1\r\n':
            raise ValueError('unexpected relay upgrade route')
        headers = {}
        for _ in range(32):
            line = reader.readline(2048)
            if line == b'\r\n':
                break
            name, value = line.rstrip(b'\r\n').split(b':', 1)
            headers[name.lower()] = value.strip()
        else:
            raise ValueError('relay upgrade header limit reached')
        nonce = headers[b'sec-websocket-key']
        if (headers[b'host'] != HOST.encode() or headers[b'upgrade'].lower() != b'websocket'
                or headers[b'sec-websocket-version'] != b'13'
                or len(base64.b64decode(nonce, validate=True)) != 16):
            raise ValueError('unexpected relay upgrade request')
        key = base64.b64encode(hashlib.sha1(nonce + b'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest())
        stream.sendall(b'HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n'
                       b'Connection: Upgrade\r\nSec-WebSocket-Accept: ' + key + b'\r\n\r\n')
        opcode, size = read_exact(reader, 2)
        if opcode != 0x81 or not size & 0x80:
            raise ValueError('expected a masked text frame from wallet')
        size &= 0x7f
        if size == 127:
            raise ValueError('relay request exceeds the small-frame limit')
        if size == 126:
            size = struct.unpack('!H', read_exact(reader, 2))[0]
        if not 0 < size <= 32768:
            raise ValueError('unexpected relay request size')
        mask = read_exact(reader, 4)
        request = json.loads(bytes(byte ^ mask[index % 4] for index, byte in enumerate(read_exact(reader, size))))
    label, request_id, selectors = request
    if (label != 'REQ' or str(uuid.UUID(request_id)) != request_id
            or selectors['kinds'] != [9321] or selectors['#p'] != [recipient]
            or selectors['#u'] != [mint] or selectors['limit'] != 16
            or not selectors['since'] <= created_at <= selectors['until']):
        raise ValueError('wallet sent an unexpected subscription')
    return request_id


def send(stream, event):
    body = json.dumps(event, separators=(',', ':')).encode()
    if len(body) >= 65536:
        raise ValueError('synthetic websocket message is too large')
    header = bytes((0x81, len(body))) if len(body) < 126 else b'\x81\x7e' + struct.pack('!H', len(body))
    stream.sendall(header + body)


def answer(listener, context, events, phase):
    connection, _ = listener.accept()
    with connection:
        connection.settimeout(5)
        try:
            stream = context.wrap_socket(connection, server_side=True)
        except ssl.SSLError:
            if phase in ('untrusted', 'wrong-host'):
                return
            raise
        with stream:
            if phase in ('untrusted', 'wrong-host'):
                raise ValueError(f'{phase} certificate unexpectedly authenticated')
            event_name = {'interrupted': 'first', 'replay': 'replay', 'after-credit': 'after_credit'}
            proof = events[event_name[phase]]
            token = subscription(stream, events['recipient'], events['mint'], proof['created_at'])
            if phase == 'replay':
                send(stream, ['EVENT', token, {**events['first'], 'sig': '00' * 64}])
                send(stream, ['EVENT', str(uuid.uuid4()), events['first']])
            send(stream, ['EVENT', token, proof])
            send(stream, ['EVENT', token, proof])
            if phase != 'interrupted':
                send(stream, ['EOSE', token])
            stream.sendall(b'\x88\x02\x03\xe8')


@contextmanager
def runtime(command, env):
    with subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                          stderr=subprocess.PIPE, text=True) as process:
        try:
            yield process
            process.terminate()
            _, error = process.communicate(timeout=10)
            if process.returncode:
                raise ValueError('WSS wallet did not stop cleanly: ' + error[-1200:])
        except Exception as failure:
            if process.poll() is not None:
                _, error = process.communicate(timeout=5)
                raise ValueError('WSS wallet exited during relay verification: ' + error[-1200:]) from failure
            raise
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=5)


def rows(database, sql, *bindings):
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True, timeout=1)) as db:
        return db.execute(sql, bindings).fetchall()


def require_scan(database, relay, outcome, admitted, process, source=None):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and process.poll() is None:
        results = rows(database, 'SELECT s.outcome,r.admitted,r.rejected FROM relay_scans s '
                       'JOIN relay_scan_results r USING(scope,relay,lane) WHERE s.relay=?', relay)
        if ((outcome, admitted, 0) in results and
                (source is None or rows(database, 'SELECT 1 FROM intake_sources WHERE source_ref=?', source))):
            return
        time.sleep(0.025)
    raise ValueError('wallet did not durably record the expected relay outcome')


def verify_core_receipt(root, command, binaries, settings, events, operation, sender_env):
    core, test = binaries
    core_uid, wallet_uid = settings['core_uid'], settings['runtime_uid']
    state, socket_directory = root / 'core', root / 'core-run'
    for directory, mode in ((state, 0o700), (socket_directory, 0o710)):
        directory.mkdir(mode=mode)
        directory.chmod(mode)
        os.chown(directory, core_uid, wallet_uid)
    socket_path = socket_directory / 'receipts.sock'
    endpoint = root / 'core.json'
    endpoint.write_text(json.dumps({
        'socket': str(socket_path), 'ipc_gid': wallet_uid, 'wallet_uid': wallet_uid,
        'wallet_instance': settings['wallet_instance'], 'wallet_epoch': settings['wallet_epoch'],
        'core_epoch': str(uuid.uuid4()), 'import_enabled': True, 'receipts_per_24h': 10,
        'credit_sats_per_24h': 100, 'maximum_custody_sats': 100,
        'approved': [{'mint_id': 'fixture', 'policy_version': 'fixture-v1',
                      'maximum_gross_sats': 100, 'maximum_net_sats': 100}],
        'deadline_seconds': 3, 'connections_per_second': 64,
    }))
    endpoint.chmod(0o644)
    fixture = {**ENV, 'LG_CASHU_IPC_FIXTURE': str(root)}
    core_command = command(core_uid, core, '--exact', CORE_RECEIPT, '--ignored', '--nocapture', gid=wallet_uid)
    wallet_command = command(wallet_uid, test, '--exact', WALLET_RECEIPT, '--ignored', '--nocapture')
    wallet_env = {**sender_env, 'LG_CASHU_WSS_ACQUISITION': operation}
    wallet_db = root / 'state/acquisition.sqlite'
    core_db = state / 'core.sqlite'
    ready = state / 'ready.json'
    with subprocess.Popen(core_command, env=fixture, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as server:
        try:
            deadline = time.monotonic() + 10
            while not (ready.exists() and socket_path.is_socket()):
                if server.poll() is not None or time.monotonic() >= deadline:
                    raise ValueError('native core receipt fixture did not become ready')
                time.sleep(0.025)
            client = root / 'client.json'
            shutil.copyfile(ready, client)
            client.chmod(0o644)
            sent = run(wallet_command, env={**wallet_env, 'LG_CASHU_WSS_PHASE': 'acquire'})
            if 'CASHU_WSS_CDK_SWAP_SENT_WITHOUT_READING_ACK' not in sent:
                raise ValueError('wallet fixture did not execute the original CDK swap')
            deadline = time.monotonic() + 8
            committed = rows(core_db, RECEIPT_ROWS)
            while not committed and server.poll() is None and time.monotonic() < deadline:
                time.sleep(0.025)
                committed = rows(core_db, RECEIPT_ROWS)
            if (len(committed) != 1 or rows(wallet_db, 'SELECT state,core_ack FROM acquisitions') !=
                    [('credit_export_pending', None)]):
                raise ValueError('lost acknowledgment was not observed after the durable core commit')
            _, encoded, net, credit = committed[0]
            receipt = json.loads(encoded)
            provenance = (receipt['acquisition_id'], receipt['wallet_instance'], receipt['wallet_epoch'],
                          receipt['transport'], receipt['source_ref'], receipt['source_author'],
                          receipt['gross_sats'], receipt['receive_fee_sats'], receipt['net_sats'])
            if (provenance != (operation, settings['wallet_instance'], settings['wallet_epoch'], 'nutzap',
                               events['first']['id'], events['first']['pubkey'], 8, 1, 7)
                    or (net, credit) != (7, 7)):
                raise ValueError('core credited different NutZap identity or amounts')
            replay = run(wallet_command, env={**wallet_env, 'LG_CASHU_WSS_PHASE': 'replay'})
            if ('CASHU_WSS_RECEIPT_REPLAY_WITHOUT_SECOND_SWAP' not in replay
                    or rows(core_db, RECEIPT_ROWS) != committed):
                raise ValueError('receipt replay changed the original core result')
            (root / 'stop').touch()
            output, error = server.communicate(timeout=5)
            if server.returncode or 'CASHU_CORE_CROSS_UID_ONE_CREDIT_VERIFIED' not in output:
                raise ValueError('native core receipt acceptance failed: ' + error[-1200:])
        finally:
            if server.poll() is None:
                server.kill()
                server.communicate(timeout=5)
    if (rows(core_db, "SELECT COUNT(*) FROM event_log WHERE event_type='payment_received'") != [(1,)]
            or rows(core_db, 'SELECT COUNT(*) FROM feed_attempts') != [(0,)]):
        raise ValueError('core retained an unexpected payment or feeder history')
    return committed


def selected(path, target):
    path = path.resolve(strict=True)
    if not path.is_file() or not 0 < path.stat().st_size <= 512 * 1024**2:
        raise ValueError('invalid selected native artifact')
    shutil.copyfile(path, target)
    target.chmod(0o555)
    with target.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def rehearse(root, args):
    root.chmod(0o711)
    context, authority = certificate(root)
    (root / 'empty').mkdir(mode=0o755)
    empty_ca = root / 'empty.pem'
    empty_ca.touch(mode=0o444)
    used = {u.pw_uid for u in pwd.getpwall()} | {g.gr_gid for g in grp.getgrall()}
    used.update(map(int, run(['/usr/bin/ps', '-e', '-o', 'uid=']).split()))
    wallet_uid, sender_uid, core_uid = [uid for uid in range(61000, 65000) if uid not in used][:3]

    def command(uid, binary, *options, gid=None):
        return ['/usr/bin/setpriv', f'--reuid={uid}', f'--regid={gid if gid is not None else uid}', '--clear-groups',
                '--no-new-privs', '--', str(binary), *map(str, options)]

    wallet, test, core = root / 'cashud', root / 'wallet-test', root / 'core-test'
    artifacts = {'wallet': selected(args.wallet, wallet), 'wallet_fixture': selected(args.wallet_test, test),
                 'core_fixture': selected(args.core_test, core)}
    sender_env = {**ENV, 'LG_CASHU_WSS_HOST_NETNS': os.readlink('/proc/1/ns/net'),
                  'LG_CASHU_WSS_FIXTURE': str(root)}
    lines = run(command(sender_uid, test, '--exact', SENDER, '--ignored', '--nocapture'), env=sender_env).splitlines()
    marker = 'CASHU_WSS_FIXTURE '
    payloads = [line.partition(marker)[2] for line in lines if marker in line]
    if len(payloads) != 1:
        raise ValueError('selected test artifact did not provide one synthetic relay input')
    [encoded] = payloads
    events = json.loads(encoded)
    state, private_run = root / 'state', root / 'run'
    for directory, mode in ((state, 0o700), (private_run, 0o710)):
        directory.mkdir(mode=mode)
        directory.chmod(mode)
        os.chown(directory, wallet_uid, wallet_uid)
    for name, data in [('wallet-seed.bin', bytes([42])*64), ('receiving-key.bin', bytes([1])*32)]:
        key = state / name
        key.write_bytes(data)
        key.chmod(0o600)
        os.chown(key, wallet_uid, wallet_uid)
    config = root / 'wallet.json'
    config.touch(mode=0o644)
    config.chmod(0o644)
    settings = {
        'version': 1, 'runtime_uid': wallet_uid, 'core_uid': core_uid, 'ipc_gid': wallet_uid,
        'wallet_instance': str(uuid.uuid4()), 'wallet_epoch': str(uuid.uuid4()),
        'state_directory': str(state), 'intake_socket': str(private_run / 'intake.sock'),
        'receive_nutzap': True, 'receiving_key': events['receiving_key'], 'recipient': events['recipient'],
        'descriptors': [], 'body_bytes': 65536, 'proof_count': 8, 'receipt_sats': 100,
        'custody_limits': {'maximum_total_sats': 8, 'maximum_per_mint_sats': {'fixture': 8}},
        'deadline_seconds': 5, 'connections_per_second': 8,
        'mints': [{'id': 'fixture', 'advertised_url': events['mint'], 'policy_version': 'fixture-v1',
                   'receive_cashu': False, 'receive_nutzap': True, 'bolt11_exit_qualified': True}],
        'keysets': [{'mint_id': 'fixture', **events['keyset']}],
        'relay_intake': {'enabled': True, 'relays': [f'wss://{HOST}'], 'historical_floor': events['first']['created_at'],
                         'window_seconds': 60, 'overlap_seconds': 5, 'event_limit': 16},
    }
    config.write_text(json.dumps(settings))
    run(command(wallet_uid, wallet, 'initialize', config))
    database = state / 'acquisition.sqlite'
    phases = (
        ('untrusted', UNTRUSTED, empty_ca, 'incomplete', 0),
        ('wrong-host', WRONG, authority, 'incomplete', 0),
        ('interrupted', HOST, authority, 'incomplete', 1),
        ('replay', HOST, authority, 'complete', 1),
        ('after-credit', HOST, authority, 'complete', 1),
    )
    original = None
    for phase, host, trust, outcome, admitted in phases:
        if phase == 'after-credit':
            credited = verify_core_receipt(root, command, (core, test), settings, events, operation, sender_env)
        relay = f'wss://{host}'
        lookup = run(command(wallet_uid, '/usr/bin/getent', 'ahostsv4', host))
        if {line.split()[0] for line in lookup.splitlines()} != {ADDRESS}:
            raise ValueError(f'{phase} resolved outside the private fixture')
        settings['relay_intake']['relays'] = [relay]
        config.write_text(json.dumps(settings))
        env = {**ENV, 'SSL_CERT_FILE': str(trust), 'SSL_CERT_DIR': str(root / 'empty')}
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind((ADDRESS, 443))
            listener.listen(4)
            listener.settimeout(10)
            with runtime(command(wallet_uid, wallet, 'run', config), env) as process:
                try:
                    answer(listener, context, events, phase)
                except TimeoutError as failure:
                    raise ValueError(f'{phase} did not finish its expected relay connection') from failure
                source = events['after_credit']['id'] if phase == 'after-credit' else None
                require_scan(database, relay, outcome, admitted, process, source)
        history = rows(database, 'SELECT a.id,a.transport,a.source_ref,a.gross,a.state,a.net,c.source_author '
                       'FROM acquisitions a JOIN acquisition_context c ON c.acquisition_id=a.id')
        if phase == 'interrupted':
            [(operation, *data)] = history
            if data != ['nutzap', events['first']['id'], 8, 'validated', None, events['first']['pubkey']]:
                raise ValueError('original signed relay event did not preserve its admission')
            original = history
        elif (phase == 'replay' and history != original) or (not admitted and history):
            raise ValueError(f'{phase} produced unexpected wallet admissions')
        elif phase == 'after-credit':
            [(original_id, transport, event_id, gross, _, _, author)] = original
            if history != [(original_id, transport, event_id, gross, 'credited', 7, author)]:
                raise ValueError('relay replay after credit changed the retained wallet admission')
            if rows(root / 'core/core.sqlite', RECEIPT_ROWS) != credited:
                raise ValueError('relay replay after credit changed original core accounting')
    aliases = rows(database, 'SELECT transport, source_ref, acquisition_id FROM intake_sources ORDER BY source_ref')
    if aliases != sorted(('nutzap', events[label]['id'], operation) for label in ('first', 'replay', 'after_credit')):
        raise ValueError('restarted wallet did not bind all event IDs to the same acquisition')
    return {'result': 'CASHU_NATIVE_WSS_INTAKE_VERIFIED', 'artifacts_sha256': artifacts,
            'real_tls_websocket': True, 'untrusted_certificate_refused': True, 'wrong_hostname_refused': True,
            'interrupted_batch_retains_admission': True, 'restart_replay_same_acquisition': True,
            'acquisitions': 1, 'event_aliases': 3, 'external_network': False,
            'cdk_swaps': 1, 'gross_sats': 8, 'receive_fee_sats': 1, 'credited_sats': 7, 'payment_events': 1,
            'native_relay_mint_acquisition_enabled': False,
            'synthetic_mint': True, 'core_acknowledgment_replayed': True, 'no_repeat_credit_after_relay_replay': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wallet', type=Path, required=True)
    parser.add_argument('--wallet-test', type=Path, required=True)
    parser.add_argument('--core-test', type=Path, required=True)
    args = parser.parse_args()
    isolated_network()
    with tempfile.TemporaryDirectory(prefix='lg-wss-intake-', dir='/run') as temporary:
        root = Path(temporary)
        hosts = root / 'hosts'
        hosts.write_text(f'127.0.0.1 localhost\n{ADDRESS} {HOST} {UNTRUSTED} {WRONG}\n')
        run(['/usr/bin/mount', '--bind', str(hosts), '/etc/hosts'])
        try:
            result = rehearse(root, args)
        finally:
            run(['/usr/bin/umount', '/etc/hosts'])
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
