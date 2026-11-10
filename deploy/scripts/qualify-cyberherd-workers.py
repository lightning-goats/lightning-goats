#!/usr/bin/env python3
"""Run installed Rust workers on isolated synthetic journals, never live state.

Authenticated launcher only. Real application units stay masked. A distinct
locked fixture-core UID, a fresh loopback netns, inaccessible production paths,
synthetic credentials, shadow core and zero execution/signing limits contain
the temporary workers. No provider, public relay or physical gateway is used.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import base64
import copy
import hashlib
import http.server
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import tarfile
import tempfile
import threading
import time
import tomllib
import urllib.request
import uuid

SPEC = importlib.util.spec_from_file_location('worker_qualification_policy', Path(__file__).with_name('qualify-cyberherd-installed.py'))
Q = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(Q)
Q.verified_tool_identity = lambda: verified_tool_identity()
C = Q.C
FIXTURE_CORE = 'cyberherd-core-fixture'
OWNER = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
CLIENT = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
SYNTHETIC_PAYOUT = b'SYNTHETIC-NO-PROVIDER-PAYOUT-KEY'
RELAYS = ['wss://nostr.chaima.info/', 'wss://relay.artx.market/', 'wss://relay.damus.io/']
ZERO_SIGN_REQUEST = {
    'method': 'sign_payout', 'intent': {
        'id': '00000000-0000-4000-8000-000000000011', 'grant_id': '00000000-0000-4000-8000-000000000012',
        'owner': OWNER, 'member': 'f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9',
        'address': 'member@example.com', 'recipient': {
            'lnurl_url': 'https://example.com/.well-known/lnurlp/member', 'callback': 'https://pay.example.com/invoice',
            'min_msat': 1000, 'max_msat': 100000, 'metadata_hash': 'ab' * 32,
            'receipt_pubkey': 'f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9'},
        'net_sats': 1, 'created_at': 0, 'relays': RELAYS,
        'content': 'SYNTHETIC socket-policy fixture; never pay or publish'}}
SAFE = {'UMask', 'NoNewPrivileges', 'CapabilityBoundingSet', 'AmbientCapabilities', 'PrivateTmp', 'PrivateDevices',
        'ProtectSystem', 'ProtectHome', 'ProtectProc', 'ProcSubset', 'ProtectKernelTunables', 'ProtectKernelModules',
        'ProtectKernelLogs', 'ProtectControlGroups', 'RestrictNamespaces', 'RestrictRealtime', 'RestrictSUIDSGID',
        'LockPersonality', 'MemoryDenyWriteExecute', 'SystemCallArchitectures', 'RestrictAddressFamilies', 'IPAddressDeny'}
HIDDEN = ['/etc/credstore.encrypted', '/etc/lightning-goats', '/var/lib/lightning-goats', '/run/lightning-goats',
          '/run/lightning-goats-bridge', '/run/cyberherd-plans', '/run/cyberherd-signer',
          '/run/credentials/lightning-goats-receive.service']
HIDDEN += [str(p / r) for r in Q.ROLES for p in (Path('/etc'), Path('/var/lib'))]
HIDDEN += ['/run/credentials/' + r + '.service' for r in Q.ROLES]


def toml_text(value):
    def key(k):
        if re.fullmatch('[A-Za-z0-9_]+', k) is None:
            raise ValueError('unsupported fixture TOML key')
        return k
    def scalar(v):
        if type(v) is bool:
            return str(v).lower()
        if type(v) is int or type(v) is float and math.isfinite(v):
            return repr(v)
        if isinstance(v, str):
            return json.dumps(v, ensure_ascii=False)
        if isinstance(v, list) and all(not isinstance(x, dict) for x in v):
            return '[' + ', '.join(scalar(x) for x in v) + ']'
        raise ValueError('unsupported fixture TOML scalar')
    lines = []
    def emit(table, prefix, header):
        if header:
            lines.extend(['', header])
        for k, v in table.items():
            if not isinstance(v, dict) and not (isinstance(v, list) and v and isinstance(v[0], dict)):
                lines.append(key(k) + ' = ' + scalar(v))
        for k, v in table.items():
            path = '.'.join([*prefix, key(k)])
            if isinstance(v, dict):
                emit(v, [*prefix, k], '[' + path + ']')
            elif isinstance(v, list) and v and isinstance(v[0], dict):
                for item in v:
                    emit(item, [*prefix, k], '[[' + path + ']]')
    emit(value, [], None)
    result = '\n'.join(lines) + '\n'
    if tomllib.loads(result) != value:
        raise ValueError('fixture TOML round-trip changed configuration')
    return result


def properties(text):
    section, values = None, []
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith(('#', ';')):
            continue
        if line.startswith('['):
            section = line
        elif section == '[Service]':
            if '=' not in line or line.endswith('\\'):
                raise ValueError('unsupported fixture sandbox source')
            name, value = line.split('=', 1)
            if name in SAFE:
                values.append((name, value))
    if not {('NoNewPrivileges', 'yes'), ('CapabilityBoundingSet', ''), ('AmbientCapabilities', ''),
            ('ProtectSystem', 'strict')}.issubset(set(values)):
        raise ValueError('installed sandbox lacks required fixture isolation')
    return values


def unit_text(template, user, group, supplementary, command, writes, credentials, namespace_path, oneshot=False):
    # Intentionally reconstruct only reviewed sandbox properties. No original
    # dependency, Exec hook, state/runtime directory, environment or key loads.
    lines = ['[Unit]', 'Description=Isolated synthetic CyberHerd worker qualification', '[Service]',
             'Type=' + ('oneshot' if oneshot else 'simple'), 'User=' + user, 'Group=' + group,
             'SupplementaryGroups=' + ' '.join(supplementary), 'ExecStart=' + command, 'Restart=no',
             'TimeoutStartSec=45s', 'TimeoutStopSec=10s', 'MemoryMax=128M', 'MemorySwapMax=0', 'TasksMax=64',
             'NetworkNamespacePath=' + str(namespace_path), 'Environment=RUST_LOG=warn']
    lines += [k + '=' + v for k, v in properties(template)]
    lines += ['ReadWritePaths=' + ' '.join(map(str, writes)), 'InaccessiblePaths=' + ' '.join('-' + p for p in HIDDEN)]
    lines += ['LoadCredentialEncrypted=' + name + ':' + str(path) for name, path in credentials.items()]
    return '\n'.join(lines) + '\n'


def fixture_identity(create):
    if not create:
        raise ValueError('explicit fixture identity flag required')
    try:
        user = C.pwd.getpwnam(FIXTURE_CORE)
    except KeyError:
        try:
            C.grp.getgrnam(FIXTURE_CORE)
        except KeyError:
            pass
        else:
            raise ValueError('partial fixture identity requires inspection')
        C.command('/usr/bin/systemd-sysusers', '--inline',
                  f'u {FIXTURE_CORE} - "Isolated CyberHerd core fixture" /nonexistent /usr/sbin/nologin')
        user = C.pwd.getpwnam(FIXTURE_CORE)
    groups = {C.grp.getgrgid(g).gr_name for g in os.getgrouplist(FIXTURE_CORE, user.pw_gid)}
    if (user.pw_uid == 0 or user.pw_gid == 0 or user.pw_shell != '/usr/sbin/nologin' or user.pw_dir != '/nonexistent'
            or groups != {FIXTURE_CORE} or user.pw_uid in {C.pwd.getpwnam(r).pw_uid for r in [*Q.ROLES, 'lightning-goats']}):
        raise ValueError('fixture core must have a distinct locked non-admin identity')
    if C.command('/usr/bin/passwd', '-S', FIXTURE_CORE).split()[1] not in ('L', 'LK'):
        raise ValueError('fixture core password is not locked')
    Q.I.require_no_sudo(FIXTURE_CORE)
    Q.no_role_processes({FIXTURE_CORE: {'uid': user.pw_uid}})
    return user


def publish(path, data, mode=0o644):
    with path.open('xb') as f:
        os.fchmod(f.fileno(), mode)
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    C.TX.sync_directory(path.parent)


def encrypt(path, name, data):
    r = subprocess.run(['/usr/bin/systemd-creds', 'encrypt', '--with-key=host', '--name=' + name, '-', str(path)],
                       input=data, capture_output=True, timeout=30, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
    if r.returncode:
        raise ValueError('fixture encryption failed; diagnostics withheld')
    path.chmod(0o600)


CLIENT_PROBE = r'''import copy,json,os,pathlib,socket,struct,sys,time
c=json.loads(pathlib.Path(sys.argv[1]).read_text())
assert os.getuid()==c['uid'] and os.getgid()==c['gid']
for path in c['hidden']:assert not os.access(path,os.R_OK)
def call(path,uid,body,wanted=None,denied_peer=False):
 deadline=time.monotonic()+12
 while True:
  try:
   with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as sock:
    sock.settimeout(2);sock.connect(path)
    assert struct.unpack('3i',sock.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))[1]==uid
    data=json.dumps(body).encode();sock.sendall(struct.pack('!I',len(data))+data)
    def read(n):
     raw=b''
     while len(raw)<n:
      extra=sock.recv(n-len(raw))
      if not extra:raise EOFError()
      raw+=extra
     return raw
    head=read(4)
    if denied_peer:raise AssertionError('unauthorized peer received a frame')
    size=struct.unpack('!I',head)[0];assert 0<size<=262144
    result=json.loads(read(size))
    if wanted is None or result.get('result')==wanted:return result
  except (EOFError,ConnectionResetError,BrokenPipeError):
   if denied_peer:return None
  assert time.monotonic()<deadline
  time.sleep(.7)
role=c['role'];proof={}
if role=='cyberherd':
 state=call(c['core_socket'],c['core_uid'],{'method':'safety'},'safety')
 assert state['state']['nostr_enabled'] is False
 page=call(c['core_socket'],c['core_uid'],{'method':'events','expected_epoch':c['epoch'],'after':0,'limit':1},'events')
 assert page['batch']['epoch']==c['epoch'];proof['intake_safety_and_events']=True
 try:
  call(c['signer_socket'],c['signer_uid'],{'method':'sign_kind1','content':'never sign'})
 except PermissionError:proof['intake_cannot_reach_payout_signer']=True
 else:raise AssertionError('intake unexpectedly reached signer socket')
elif role=='cyberherd-payout':
 assert call(c['core_socket'],c['core_uid'],{'method':'safety'},'rejected')['result']=='rejected'
 assert call(c['intake_socket'],c['intake_uid'],{'method':'next','owner':c['owner'],'after':0},'waiting')['result']=='waiting'
 request=copy.deepcopy(c['zero_sign_request']);request['intent']['created_at']=int(time.time())
 assert call(c['signer_socket'],c['signer_uid'],request,'rejected')['result']=='rejected'
 assert call(c['signer_socket'],c['signer_uid'],{'method':'sign_kind1','content':'never sign'},'rejected')['result']=='rejected'
 proof={'payout_cannot_read_intake_events':True,'payout_plan_export_authenticated':True,'valid_zero_limit_signing_rejected':True,'arbitrary_signing_rejected':True}
elif role=='core':
 call(c['core_socket'],c['core_uid'],{'method':'safety'},denied_peer=True)
 proof['socket_owner_is_not_an_authorized_intake_peer']=True
else:raise AssertionError('unknown client role')
with open(c['result'],'x') as f:json.dump(proof,f);f.flush();os.fsync(f.fileno())
'''


def http_json(url):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(url, timeout=3) as response:
        raw = response.read(262145)
        if len(raw) > 262144 or response.status != 200:
            raise ValueError('fixture HTTP response exceeds bound')
        return json.loads(raw)


def epoch_snapshot(port):
    key = base64.b64encode(os.urandom(16)).decode()
    with socket.create_connection(('127.0.0.1', port), timeout=3) as sock:
        sock.sendall(('GET /ws/overlay HTTP/1.1\r\nHost: fixture\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: ' + key + '\r\n\r\n').encode())
        raw = b''
        while b'\r\n\r\n' not in raw:
            chunk = sock.recv(4096)
            if not chunk or len(raw) + len(chunk) > 32768:
                raise ValueError('invalid fixture WebSocket upgrade')
            raw += chunk
        headers, buffer = raw.split(b'\r\n\r\n', 1)
        if headers.split(b'\r\n')[0] != b'HTTP/1.1 101 Switching Protocols':
            raise ValueError('fixture WebSocket did not upgrade')
        def read(n):
            nonlocal buffer
            while len(buffer) < n:
                chunk = sock.recv(n - len(buffer))
                if not chunk:
                    raise ValueError('short fixture WebSocket frame')
                buffer += chunk
            result, buffer = buffer[:n], buffer[n:]
            return result
        a, b = read(2)
        if a != 0x81 or b & 0x80:
            raise ValueError('unexpected fixture WebSocket frame')
        size = b & 127
        if size == 126:
            size = struct.unpack('!H', read(2))[0]
        elif size == 127:
            size = struct.unpack('!Q', read(8))[0]
        if size > 16384:
            raise ValueError('fixture snapshot exceeds bound')
        data = json.loads(read(size))
        if data.get('type') != 'snapshot' or data['feed_credit_sats'] != 0:
            raise ValueError('fixture has unexpected financial activity')
        mask = os.urandom(4)
        sock.sendall(b'\x88\x82' + mask + bytes(v ^ mask[i % 4] for i, v in enumerate(b'\x03\xe8')))
        return str(uuid.UUID(data['stream_id']))


def await_ready(predicate, description):
    end = time.monotonic() + 30
    while True:
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError):
            pass
        if time.monotonic() >= end:
            raise ValueError('fixture readiness timeout: ' + description)
        time.sleep(.2)


PROOFS = {
    'cyberherd': {'intake_safety_and_events', 'intake_cannot_reach_payout_signer'},
    'cyberherd-payout': {'payout_cannot_read_intake_events', 'payout_plan_export_authenticated',
                        'valid_zero_limit_signing_rejected', 'arbitrary_signing_rejected'},
    'core': {'socket_owner_is_not_an_authorized_intake_peer'},
}


def check_proof(role, value):
    if not isinstance(value, dict) or set(value) != PROOFS[role] or any(v is not True for v in value.values()):
        raise ValueError('incomplete native socket boundary proof')
    return value


def socket_ready(path, uid):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(.5)
        sock.connect(str(path))
        pid, peer, _ = struct.unpack('3i', sock.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        return pid > 0 and peer == uid


def run(installation, core_transaction, destination, create):
    C.root()
    if not create:
        raise ValueError('explicit isolated-worker fixture flag required')
    Q.namespace()
    C.protected(C.BASE, directory=True, private=True)
    C.protected(Path('/var/lib/systemd/credential.secret'), private=True)
    for p in (installation, core_transaction, destination):
        if p.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', p.name) is None:
            raise ValueError('protected bounded transaction names required')
    available = next(int(line.split()[1]) for line in Path('/proc/meminfo').read_text().splitlines() if line.startswith('MemAvailable:'))
    if available < 256 * 1024:
        raise ValueError('insufficient spare host memory for isolated native worker fixtures')
    before = Q.baseline(installation)
    C.protected(core_transaction, directory=True, private=True)
    upgrade = C.read_json(core_transaction / 'UPGRADE.json')
    for name, path in C.TARGETS.items():
        C.protected(path)
        if C.TX.fingerprint(path)['sha256'] != upgrade['release']['payload_sha256'][name]:
            raise ValueError('original core program provenance changed')
    core_template = core_transaction / 'payload/deploy/systemd/lightning-goats-receive.service'
    C.protected(core_template)
    if C.TX.fingerprint(core_template)['sha256'] != upgrade['release']['payload_sha256']['deploy/systemd/lightning-goats-receive.service']:
        raise ValueError('reviewed core fixture sandbox provenance changed')
    original_meta = C.read_json(installation / 'INSTALLED.json')
    with tarfile.open(installation / 'review.tar.gz', 'r:gz') as archive:
        selected = [m for m in archive if str(Path(m.name)) == 'cyberherd-statectl']
        if len(selected) != 1 or selected[0].size > 128 * 1024 ** 2:
            raise ValueError('original native state verifier missing')
        expected_ctl = hashlib.sha256(archive.extractfile(selected[0]).read()).hexdigest()
    native_ctl = Q.BIN / 'cyberherd-statectl'
    C.protected(native_ctl)
    if C.TX.fingerprint(native_ctl)['sha256'] != expected_ctl or original_meta['services_started']:
        raise ValueError('installed native verifier provenance changed')
    probe_user = fixture_identity(create)
    destination.mkdir(mode=0o700)
    C.write_json(destination / 'INCOMPLETE.json', {'accepted': False, 'production_workers_started': False})
    root = Path(tempfile.mkdtemp(prefix='lg-cw-', dir='/run'))
    root.chmod(0o755)
    for name in ('state', 'sockets', 'results'):
        (root / name).mkdir(mode=0o755)
        (root / name).chmod(0o755)
    users = {r: C.pwd.getpwnam(r) for r in Q.ROLES}
    users['core'] = probe_user
    groups = {r: [C.grp.getgrgid(g).gr_name for g in os.getgrouplist(u.pw_name, u.pw_gid) if g != u.pw_gid] for r, u in users.items()}
    groups['core'] = ['cyberherd-bridge']
    states, outputs, sockets = {}, {}, {}
    for role, user in users.items():
        for parent, mapping in [('state', states), ('results', outputs)]:
            path = root / parent / role
            path.mkdir(mode=0o700)
            os.chown(path, user.pw_uid, user.pw_gid)
            mapping[role] = path
    for role, group, filename in [('core', 'cyberherd-bridge', 'core.sock'),
                                  ('cyberherd', 'cyberherd-plans', 'plans.sock'),
                                  ('cyberherd-signer', 'cyberherd-payout-signer', 'signer.sock')]:
        path = root / 'sockets' / role
        path.mkdir(mode=0o750)
        path.chmod(0o750)
        os.chown(path, users[role].pw_uid, C.grp.getgrnam(group).gr_gid)
        sockets[role] = path / filename
    token = uuid.uuid4().hex[:10]
    names = {r: 'lg-cyberherd-worker-' + r + '-' + token + '.service' for r in users}
    namespace_path = Path('/proc') / str(os.getpid()) / 'ns/net'
    templates = {r: before['units'][r] for r in Q.ROLES}
    templates['core'] = core_template.read_text()
    units, server, thread, cycles = [], None, None, []
    credentials = {r: {} for r in users}
    keys = {'core': {'strike-api-key': b'SYNTHETIC-NO-PROVIDER-RECEIVE-KEY', 'strike-webhook-secret': b'SYNTHETIC-NO-REAL-WEBHOOK-SECRET'},
            'cyberherd-payout': {'strike-payout-api-key': SYNTHETIC_PAYOUT},
            'cyberherd-signer': {'nostr-client-key': ('0' * 63 + '1').encode()}}
    counts = {'post': 0}
    account = str(uuid.uuid4())
    def add_unit(role, path, command, writes, creds, oneshot=False):
        raw = unit_text(templates[role], users[role].pw_name, C.grp.getgrgid(users[role].pw_gid).gr_name,
                        groups[role], command, writes, creds, namespace_path, oneshot).encode()
        publish(path, raw)
        units.append({'path': path, 'sha256': hashlib.sha256(raw).hexdigest(), 'masked': False})
    def worker_unit(role, config):
        binary = 'lightning-goatsd' if role == 'core' else Q.ROLES[role][0]
        writable = [states[role]] + ([sockets[role].parent] if role in sockets else [])
        add_unit(role, Q.RUN_UNITS / names[role], f'/usr/local/bin/{binary} --config {config}', writable, credentials[role])
    def check_process(role):
        props = C.status(names[role])
        if props['ActiveState'] != 'active' or props['SubState'] != 'running' or props['MainPID'] == '0':
            raise ValueError('native fixture worker is not running')
        status = (Path('/proc') / props['MainPID'] / 'status').read_text()
        fields = dict(line.split(':', 1) for line in status.splitlines() if ':' in line)
        if (set(map(int, fields['Uid'].split())) != {users[role].pw_uid} or fields['NoNewPrivs'].strip() != '1'
                or any(int(fields[n].strip(), 16) != 0 for n in ('CapEff', 'CapPrm', 'CapBnd', 'CapAmb'))):
            raise ValueError('native worker kernel identity/capabilities differ')
        return int(props['MainPID'])
    try:
        C.command('/usr/sbin/ip', 'link', 'set', 'lo', 'up')
        fake_nak = root / 'nak-fixture'
        nak_called = states['cyberherd-signer'] / 'nak-invoked'
        publish(fake_nak, ('#!/bin/sh\n: > ' + str(nak_called) + '\nexit 97\n').encode(), 0o755)
        class Gateway(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_GET(self):
                payload = {'override_enabled': True, 'remote_enabled': False} if self.path == '/v1/feeder/override' else {}
                raw = json.dumps(payload).encode()
                self.send_response(200 if self.path in ('/healthz', '/v1/feeder/override') else 404)
                self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(raw)))
                self.end_headers(); self.wfile.write(raw)
            def do_POST(self):
                counts['post'] += 1
                self.send_response(405); self.end_headers()
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 18789), Gateway)
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        for role, values in keys.items():
            for name, key in values.items():
                path = destination / (role + '-' + name + '.synthetic.cred')
                encrypt(path, name, key); credentials[role][name] = path
        core = tomllib.loads(C.CONFIG.read_text())
        core['service'] = {'listen': '127.0.0.1:18787', 'mode': 'shadow'}
        core['database']['url'] = 'sqlite://' + str(states['core'] / 'lightning-goats.db')
        core['gateway']['url'] = 'http://127.0.0.1:18789/'
        core['lnurl'].pop('zap', None)
        core['informational'].update(interface_info_enabled=False, weather_enabled=False)
        core['nostr']['bunker_pubkey'] = OWNER; core['nostr']['nak_config_path'] = str(states['core'] / 'nak')
        core['core_bridge'] = {'socket': str(sockets['core']), 'peer_uid': users['cyberherd'].pw_uid,
            'group_gid': C.grp.getgrnam('cyberherd-bridge').gr_gid, 'payout_peer_uid': users['cyberherd-payout'].pw_uid,
            'payout_execution_enabled': False, 'reward_policy': {'owner': OWNER, 'member_percent_ceiling': 0,
                                                               'max_sats_per_feed': 0, 'max_sats_per_24h': 0}}
        core_config = root / 'core.toml'; publish(core_config, toml_text(core).encode())
        worker_unit('core', core_config)
        C.command('/usr/bin/systemctl', 'daemon-reload'); C.command('/usr/bin/systemctl', 'start', names['core'])
        epoch = await_ready(lambda: epoch_snapshot(18787), 'core snapshot')
        configs = {}
        for role in Q.ROLES:
            value = tomllib.loads((Q.ETC / role / 'config.toml').read_text())
            value['state_dir'] = str(states[role]); value['enabled'] = False
            if role == 'cyberherd':
                value['core_socket'] = str(sockets['core']); value['project_pubkey'] = OWNER
                value['core'] = {'core_uid': probe_user.pw_uid, 'epoch': epoch, 'start_after': 0,
                                 'credit_pool': 'herd', 'payees': {'herd': OWNER}}
                value['settings']['member_percent'] = 0
                for name in ('messaging', 'rewards', 'view'):
                    value.pop(name, None)
                value['payout_bridge'] = {'socket': str(sockets[role]), 'peer_uid': users['cyberherd-payout'].pw_uid,
                                         'group_gid': C.grp.getgrnam('cyberherd-plans').gr_gid}
            elif role == 'cyberherd-payout':
                value.update(core_socket=str(sockets['core']), intake_socket=str(sockets['cyberherd']), signer_socket=str(sockets['cyberherd-signer']))
                scope = value['policy']['scope']; scope.update(core_uid=probe_user.pw_uid, epoch=epoch, owner=OWNER)
                scope['provider'].update(account_id=account, credential_fingerprint=hashlib.sha256(SYNTHETIC_PAYOUT).hexdigest())
            else:
                value['socket'] = str(sockets[role]); value['policy'].update(owner=OWNER, client_pubkey=CLIENT)
                value['nostr'].update(bunker_pubkey=OWNER, nak_config_path=str(states[role] / 'nak'), nak_path=str(fake_nak))
            configs[role] = value
            path = root / (role + '.toml'); publish(path, toml_text(value).encode())
            C.command('/usr/bin/setpriv', '--reuid=' + str(users[role].pw_uid), '--regid=' + str(users[role].pw_gid),
                      '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
                      str(Q.BIN / Q.ROLES[role][0]), '--config', str(path), '--initialize-state', timeout=120)
            value['enabled'] = True
            if role == 'cyberherd':
                value['view'] = {'enabled': True, 'listen': '127.0.0.1:13340', 'overlay': False}
            pending = path.with_suffix('.enabled')
            publish(pending, toml_text(value).encode()); os.replace(pending, path)
            worker_unit(role, path)
        client = root / 'client.py'; publish(client, CLIENT_PROBE.encode())
        C.command('/usr/bin/systemctl', 'daemon-reload')
        for cycle in range(2):
            if cycle:
                C.command('/usr/bin/systemctl', 'start', names['core'])
                await_ready(lambda: epoch_snapshot(18787), 'restarted core snapshot')
                for role in Q.ROLES:
                    C.command('/usr/bin/systemctl', 'start', names[role])
            else:
                for role in Q.ROLES:
                    C.command('/usr/bin/systemctl', 'start', names[role])
            await_ready(lambda: all(socket_ready(p, users[r].pw_uid) for r, p in sockets.items()), 'native Unix sockets')
            view = await_ready(lambda: http_json('http://127.0.0.1:13340/api/v1/cyberherd'), 'native readonly roster')
            if view['roster']['owner'] != OWNER or view['roster']['total_members'] != 0 or epoch_snapshot(18787) != epoch:
                raise ValueError('native fixture scope changed across startup/restart')
            worker_pids = {r: check_process(r) for r in users}
            proof = {}
            for role in ('cyberherd', 'cyberherd-payout', 'core'):
                result = outputs[role] / ('cycle-' + str(cycle) + '.json')
                payload = {'role': role, 'uid': users[role].pw_uid, 'gid': users[role].pw_gid, 'hidden': HIDDEN,
                    'core_socket': str(sockets['core']), 'core_uid': probe_user.pw_uid,
                    'intake_socket': str(sockets['cyberherd']), 'intake_uid': users['cyberherd'].pw_uid,
                    'signer_socket': str(sockets['cyberherd-signer']), 'signer_uid': users['cyberherd-signer'].pw_uid,
                    'owner': OWNER, 'epoch': epoch, 'zero_sign_request': ZERO_SIGN_REQUEST, 'result': str(result)}
                input_path = root / (role + '-client-' + str(cycle) + '.json')
                publish(input_path, json.dumps(payload).encode())
                name = 'lg-cyberherd-client-' + role + '-' + token + '-' + str(cycle) + '.service'
                add_unit(role, Q.RUN_UNITS / name, f'/usr/bin/python3 -I -S -B {client} {input_path}', [outputs[role]], {}, True)
                C.command('/usr/bin/systemctl', 'daemon-reload'); C.command('/usr/bin/systemctl', 'start', name, timeout=45)
                proof[role] = check_proof(role, json.loads(result.read_text()))
            cycles.append({'worker_pids': worker_pids, 'socket_proofs': proof, 'readonly_roster_empty': True})
            for role in users:
                C.command('/usr/bin/systemctl', 'stop', names[role])
        # Mask only our stopped fixture workers before native four-store evidence.
        for entry in units:
            path = entry['path']
            if path.name not in names.values():
                continue
            if hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
                raise ValueError('temporary fixture unit changed')
            path.unlink(); path.symlink_to('/dev/null'); entry['masked'] = True
        C.command('/usr/bin/systemctl', 'daemon-reload')
        databases = {'core': 'lightning-goats.db', 'cyberherd': 'cyberherd.db', 'cyberherd-payout': 'payout.db', 'cyberherd-signer': 'signer.db'}
        policy = {'version': 1, 'owner': OWNER, 'epoch': epoch, 'max_component_bytes': 67108864,
                  'provider': {'environment': 'strike-production', 'account_id': account,
                               'credential_fingerprint': hashlib.sha256(SYNTHETIC_PAYOUT).hexdigest()}, 'components': {}}
        for role, component in [('core', 'core'), ('cyberherd', 'membership'), ('cyberherd-payout', 'payout'), ('cyberherd-signer', 'signer')]:
            policy['components'][component] = {'database': str(states[role] / databases[role]), 'uid': users[role].pw_uid,
                                                'gid': users[role].pw_gid, 'unit': names[role]}
        policy_path = destination / 'fixture-maintenance.toml'; publish(policy_path, toml_text(policy).encode(), 0o600)
        bundle = destination / 'fixture-snapshot'
        C.command(str(native_ctl), '--config', str(policy_path), 'snapshot', '--destination', str(bundle), '--writers-stopped', timeout=120)
        C.command(str(native_ctl), '--config', str(policy_path), 'verify', '--bundle', str(bundle), timeout=120)
        report = C.read_json(bundle / 'MANIFEST.json')['report']
        for field in ('pending_feeds', 'unresolved_provider_payments', 'execution_claims', 'payout_held_sats'):
            if report[field] != 0:
                raise ValueError('synthetic fixture acquired unexpected financial/physical work')
        for component, table in [('core', 'settled_payments'), ('core', 'feed_attempts'), ('core', 'message_outbox'),
                                 ('payout', 'payout_execute_intents'), ('signer', 'signer_intents')]:
            if report['rows'][component][table] != 0:
                raise ValueError('synthetic fixture produced forbidden payment/feed/publication/signing work')
        if counts['post'] or nak_called.exists():
            raise ValueError('shadow fixture unexpectedly attempted feeder POST or remote signing')
        receipt = {'accepted': True, 'actual_installed_rust_workers': 4, 'distinct_kernel_uids': 4,
                   'synthetic': True, 'fixture_restarts': 1, 'same_fixture_epoch': True,
                   'native_four_journal_snapshot_verified': True, 'cycles': cycles,
                   'production_workers_started': False, 'real_credentials_loaded': False,
                   'provider_qualified': False, 'household_qualified': False, 'browser_qualified': False}
    except Exception as error:
        logs = {}
        for entry in units:
            r = subprocess.run(['/usr/bin/journalctl', '--no-pager', '-u', entry['path'].name, '-n', '25'],
                               capture_output=True, text=True, timeout=10, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
            logs[entry['path'].name] = r.stdout[-8000:]
        C.write_json(destination / 'FAILURE.json', {'error_type': type(error).__name__, 'synthetic_worker_journals': logs})
        raise
    finally:
        errors = []
        for entry in reversed(units):
            path = entry['path']
            try:
                if entry['masked']:
                    if not path.is_symlink() or os.readlink(path) != '/dev/null':
                        raise ValueError('fixture mask changed')
                else:
                    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
                        raise ValueError('fixture unit changed')
                    C.command('/usr/bin/systemctl', 'stop', path.name)
                path.unlink()
            except Exception:
                errors.append(path.name)
        if units:
            C.command('/usr/bin/systemctl', 'daemon-reload')
        if server:
            server.shutdown(); server.server_close(); thread.join(timeout=2)
        root.chmod(0o700)  # Retain only private synthetic fixtures for diagnosis.
        if errors:
            raise ValueError('some temporary worker units require inspection; production units remain untouched')
    if Q.baseline(installation) != before:
        raise ValueError('original installed state/core process changed during native fixture qualification')
    Q.no_role_processes({FIXTURE_CORE: {'uid': probe_user.pw_uid}})
    receipt['production_journals_and_core_unchanged'] = True
    C.write_json(destination / 'RESULT.json', receipt)
    (destination / 'INCOMPLETE.json').unlink(); C.TX.sync_directory(destination)
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--installation', type=Path, required=True)
    parser.add_argument('--core-transaction', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--create-isolated-fixture', action='store_true', required=True)
    args = parser.parse_args()
    print(json.dumps(run(args.installation, args.core_transaction, args.destination, args.create_isolated_fixture), sort_keys=True))


if __name__ == '__main__':
    main()
