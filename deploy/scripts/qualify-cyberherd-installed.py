#!/usr/bin/env python3
"""Probe installed disabled roles with synthetic credentials in a new netns.

Authenticated launcher only. Application units stay masked; no real credential,
application worker, provider, relay or gateway is used. Only uniquely named
temporary probe units and marker files are created. Original journals are hashed,
never parsed or migrated. The current receive unit/process is left unchanged.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import socket
import stat
import subprocess
import tarfile
import tempfile
import threading
import time
import tomllib
import uuid


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


C = helper('installed_qualification_core', 'upgrade-cyberherd-core.py')
C.verified_tool_identity = lambda: verified_tool_identity()
I = helper('installed_qualification_roles', 'install-cyberherd-inactive.py')
ROLES = {
    'cyberherd': ('cyberherdd', 'cyberherd.db', 'nostr-client-key', 'cyberherd-nostr-client'),
    'cyberherd-payout': ('cyberherd-payoutd', 'payout.db', 'strike-payout-api-key', 'cyberherd-strike-payout'),
    'cyberherd-signer': ('cyberherd-signerd', 'signer.db', 'nostr-client-key', 'cyberherd-signer-client'),
}
BIN = Path('/usr/local/bin')
UNITS = Path('/usr/local/lib/systemd/system')
RUN_UNITS = Path('/run/systemd/system')
STATE = Path('/var/lib')
ETC = Path('/etc')
RESULT_FIELDS = {'uid_verified', 'kernel_policy_verified', 'synthetic_credential_scope_verified',
                 'foreign_state_and_credential_store_denied', 'managed_files_immutable',
                 'private_ipv4_ipv6_denied', 'public_ipv4_ipv6_mock_reachable'}

PROBE = r'''import hashlib,json,os,pathlib,socket,sys,tempfile,time
c=json.loads(pathlib.Path(sys.argv[1]).read_text())
assert os.getuid()==c['uid'] and os.getgid()==c['gid']
assert set(os.getgroups())==set(c['groups'])
s=dict(line.split(':',1) for line in pathlib.Path('/proc/self/status').read_text().splitlines() if ':' in line)
assert s['NoNewPrivs'].strip()=='1'
assert all(int(s[name].strip(),16)==0 for name in ['CapEff','CapPrm','CapBnd','CapAmb'])
assert os.readlink('/proc/self/ns/net')==c['namespace']
cred=pathlib.Path(os.environ['CREDENTIALS_DIRECTORY'])
assert sorted(p.name for p in cred.iterdir())==[c['credential_name']]
assert hashlib.sha256((cred/c['credential_name']).read_bytes()).hexdigest()==c['credential_sha256']
for target in c['immutable_paths']:
 try:fd=os.open(target,os.O_WRONLY|os.O_APPEND)
 except PermissionError:pass
 except OSError as e:assert e.errno==30
 else:
  os.close(fd);raise AssertionError('managed file is writable')
assert os.access(c['journal'],os.R_OK)
for target in c['foreign_paths']:assert not os.access(target,os.R_OK)
with tempfile.TemporaryFile(dir=c['state']) as f:f.write(b'synthetic unnamed policy probe')
def reachable(host,family):
 with socket.socket(family,socket.SOCK_STREAM) as sock:
  sock.settimeout(1)
  try:
   sock.connect((host,c['port']));return sock.recv(2)==b'OK'
  except OSError:return False
assert reachable('203.0.113.19',socket.AF_INET)
assert reachable('2001:db8:19::19',socket.AF_INET6)
assert not reachable('10.8.0.6',socket.AF_INET)
assert not reachable('fd00:19::6',socket.AF_INET6)
def publish(path,body):
 with open(path+'.writing','x') as f:f.write(body);f.flush();os.fsync(f.fileno())
 os.replace(path+'.writing',path)
publish(c['ready'],'ready')
deadline=time.monotonic()+25
while not pathlib.Path(c['release']).exists():
 assert time.monotonic()<deadline
 time.sleep(.05)
for path in c['foreign_credentials']:assert not os.access(path,os.R_OK)
publish(c['result'],json.dumps({'role':c['role'],'uid_verified':True,'kernel_policy_verified':True,
 'synthetic_credential_scope_verified':True,'foreign_state_and_credential_store_denied':True,
 'managed_files_immutable':True,'private_ipv4_ipv6_denied':True,'public_ipv4_ipv6_mock_reachable':True}))
'''


def disabled(values):
    if set(values) != set(ROLES) or any(v.get('enabled') is not False for v in values.values()):
        raise ValueError('all installed role processes must be disabled')
    intake, payout, signer = (values[r] for r in ROLES)
    if intake.get('messaging', {}).get('enabled', False) or intake.get('view', {}).get('enabled', False):
        raise ValueError('installed membership publication and view must be disabled')
    policy = payout.get('policy', {})
    if policy.get('execution_enabled') is not False:
        raise ValueError('installed payout execution must be disabled')
    limits = [(policy, n) for n in ('member_percent_ceiling', 'max_sats_per_feed', 'max_sats_per_24h', 'max_fee_sats')]
    limits += [(signer.get('policy', {}), n) for n in ('max_net_sats', 'max_new_per_hour')]
    if any(type(p.get(n)) is not int or p[n] != 0 for p, n in limits):
        raise ValueError('installed financial and signing limits must all be zero')


def namespace():
    current = os.readlink('/proc/self/ns/net')
    if current == os.readlink('/proc/1/ns/net'):
        raise ValueError('a separate fresh network namespace is required before qualification')
    links = json.loads(C.command('/usr/sbin/ip', '-j', 'link', 'show'))
    if {v['ifname'] for v in links} != {'lo'}:
        raise ValueError('only an isolated loopback interface is allowed')
    return current


def check_result(role, value):
    if (not isinstance(value, dict) or value.get('role') != role
            or set(value) != RESULT_FIELDS | {'role'}
            or any(value[name] is not True for name in RESULT_FIELDS)):
        raise ValueError('policy probe did not positively accept every required boundary')
    return {name: value[name] for name in sorted(RESULT_FIELDS)}


def no_role_processes(runtime):
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            lines = (process / 'status').read_text().splitlines()
        except FileNotFoundError:
            continue
        if any(line.startswith('Uid:') and set(map(int, line.split()[1:])) & {r['uid'] for r in runtime.values()} for line in lines):
            raise ValueError('another installed-role process exists')


def baseline(installation, masked_dropins=None):
    C.protected(installation, directory=True, private=True)
    if installation.parent != C.BASE or (installation / 'INCOMPLETE').exists():
        raise ValueError('accepted original inactive installation required')
    meta = C.read_json(installation / 'INSTALLED.json')
    if meta['version'] != 1 or set(meta['roles']) != set(ROLES) or meta['services_started'] or meta['credentials_loaded']:
        raise ValueError('unsupported original installation receipt')
    I.check_masked(masked_dropins)
    runtime = I.check_identities()
    no_role_processes(runtime)
    for name in ('cyberherd-plans', 'cyberherd-signer'):
        path = Path('/run') / name
        if path.exists() or path.is_symlink():
            raise ValueError('existing role runtime directory requires inspection before fresh qualification')
    configs, journals, programs, unit_text = {}, {}, {}, {}
    C.protected(UNITS, directory=True)
    archive = installation / 'review.tar.gz'
    C.protected(archive, private=True)
    if archive.stat().st_size > 512 * 1024 ** 2 or hashlib.sha256(archive.read_bytes()).hexdigest() != meta['archive_sha256']:
        raise ValueError('original installed archive drift')
    archived = {}
    with tarfile.open(archive, 'r:gz') as reader:
        for member in reader:
            name = str(Path(member.name))
            if name in {f'deploy/systemd/{r}.service.example' for r in ROLES}:
                if not member.isfile() or member.size > 65536 or name in archived:
                    raise ValueError('invalid original unit member')
                archived[name] = reader.extractfile(member).read()
    for role, (binary, journal, _, _) in ROLES.items():
        original = meta['roles'][role]
        if runtime[role] != original['runtime']:
            raise ValueError('installed runtime identity changed')
        config = ETC / role / 'config.toml'
        C.protected(config)
        if config.stat().st_size > 65536 or hashlib.sha256(config.read_bytes()).hexdigest() != original['config_sha256']:
            raise ValueError('original disabled configuration changed')
        configs[role] = tomllib.loads(config.read_text())
        program = BIN / binary
        C.protected(program)
        programs[role] = C.TX.fingerprint(program)
        if programs[role]['sha256'] != original['binary_sha256'] or programs[role]['mode'] != 0o755:
            raise ValueError('installed binary drift')
        state = STATE / role
        info = state.lstat()
        if (not stat.S_ISDIR(info.st_mode) or state.resolve(strict=True) != state
                or info.st_uid != runtime[role]['uid'] or info.st_gid != runtime[role]['gid'] or info.st_mode & 0o077
                or any(n.startswith('system.posix_acl') for n in os.listxattr(state))):
            raise ValueError('invalid private installed state directory')
        for suffix in ('-wal', '-shm', '-journal'):
            if (state / (journal + suffix)).exists() or (state / (journal + suffix)).is_symlink():
                raise ValueError('fresh inactive journal has unexpected sidecars')
        if (state / 'CYBERHERD_RECOVERY_REQUIRED').exists() or (state / 'CYBERHERD_RECOVERY_REQUIRED').is_symlink():
            raise ValueError('recovery staging cannot be qualified as a fresh installation')
        info = (state / journal).lstat()
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 64 * 1024 ** 2
                or info.st_uid != runtime[role]['uid'] or info.st_gid != runtime[role]['gid']
                or stat.S_IMODE(info.st_mode) != 0o600):
            raise ValueError('invalid fresh private installed journal')
        journals[role] = C.TX.fingerprint(state / journal)
        if journals[role] != original['journal']:
            raise ValueError('initial journal changed; this qualifier is for fresh inactive roles only')
        unit = UNITS / (role + '.service')
        C.protected(unit)
        raw = unit.read_bytes()
        if raw != archived[f'deploy/systemd/{role}.service.example']:
            raise ValueError('installed unit differs from the reviewed archive')
        unit_text[role] = raw.decode()
    disabled(configs)
    no_role_processes(runtime)
    core = C.status(C.UNIT_NAME)
    core_files = C.unit_files(core)
    C.protected(C.CONFIG)
    C.protected(Path('/etc/credstore.encrypted'), directory=True, private=True)
    info = C.DATABASE.lstat()
    core_user = C.pwd.getpwnam('lightning-goats')
    if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_uid != core_user.pw_uid
            or info.st_gid != core_user.pw_gid or info.st_mode & 0o077):
        raise ValueError('original private core journal identity is invalid')
    core_files[str(C.CONFIG)] = C.TX.fingerprint(C.CONFIG)
    return {'runtime': runtime, 'journal': journals, 'program': programs,
            'configuration': {r: meta['roles'][r]['config_sha256'] for r in ROLES},
            'units': unit_text, 'core': {'properties': core, 'files': core_files}}


def once(text, before, after):
    if text.count(before) != 1:
        raise ValueError('unexpected reviewed unit shape')
    return text.replace(before, after)


def render_unit(text, role, probe, config, ciphertext, netns):
    binary, _, credential, filename = ROLES[role]
    expected = f'ExecStart=/usr/local/bin/{binary} --config /etc/{role}/config.toml'
    if any(line.strip().startswith('Exec') and line.strip() != expected for line in text.splitlines()):
        raise ValueError('probe must not execute additional application/helper commands')
    directives = {line.strip().split('=', 1)[0] for line in text.splitlines() if '=' in line and not line.strip().startswith('#')}
    if directives & {'NetworkNamespacePath', 'PrivateNetwork', 'Environment', 'EnvironmentFile',
                     'PassEnvironment', 'ImportCredential', 'SetCredential', 'SetCredentialEncrypted', 'LoadCredential'}:
        raise ValueError('unreviewed namespace/environment in installed unit')
    text = once(text, expected, f'ExecStart=/usr/bin/python3 -I -S -B {probe} {config}')
    text = once(text, 'Restart=on-failure', 'Restart=no')
    old = f'LoadCredentialEncrypted={credential}:/etc/credstore.encrypted/{filename}'
    text = once(text, ('# ' if role == 'cyberherd' else '') + old,
                f'LoadCredentialEncrypted={credential}:{ciphertext}')
    # No installed credential path may survive into a probe unit.
    if any(line.strip().startswith(('LoadCredential=', 'LoadCredentialEncrypted=')) and str(ciphertext) not in line for line in text.splitlines()):
        raise ValueError('unreviewed credential in probe unit')
    return once(text, '[Install]', f'NetworkNamespacePath={netns}\n\n[Install]')


def listener(family, port):
    server = socket.socket(family, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    if family == socket.AF_INET6:
        server.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
    server.bind(('::' if family == socket.AF_INET6 else '0.0.0.0', port))
    server.listen()
    server.settimeout(.2)
    stopped = threading.Event()
    def accept():
        while not stopped.is_set():
            try:
                client, _ = server.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with client:
                client.sendall(b'OK')
    thread = threading.Thread(target=accept, daemon=True)
    thread.start()
    return server, stopped, thread


def probes(before, destination):
    C.protected(RUN_UNITS, directory=True)
    public = Path(tempfile.mkdtemp(prefix='lg-cyberherd-policy-', dir='/run'))
    public.chmod(0o755)
    probe = public / 'probe.py'
    probe.write_text(PROBE)
    probe.chmod(0o644)
    token = uuid.uuid4().hex[:12]
    names = {r: 'lg-cyberherd-policy-' + r + '-' + token + '.service' for r in ROLES}
    namespace_path = Path('/proc') / str(os.getpid()) / 'ns/net'
    units, markers, servers, results = [], [], [], {}
    try:
        C.command('/usr/sbin/ip', 'link', 'set', 'lo', 'up')
        for address in ('203.0.113.19/32', '10.8.0.6/32'):
            C.command('/usr/sbin/ip', 'address', 'add', address, 'dev', 'lo')
        for address in ('2001:db8:19::19/128', 'fd00:19::6/128'):
            C.command('/usr/sbin/ip', '-6', 'address', 'add', address, 'dev', 'lo', 'nodad')
        port = 18789
        servers.extend([listener(socket.AF_INET, port), listener(socket.AF_INET6, port)])
        for family, host in [(socket.AF_INET, '203.0.113.19'), (socket.AF_INET, '10.8.0.6'),
                             (socket.AF_INET6, '2001:db8:19::19'), (socket.AF_INET6, 'fd00:19::6')]:
            with socket.socket(family, socket.SOCK_STREAM) as connection:
                connection.settimeout(2)
                connection.connect((host, port))
                if connection.recv(2) != b'OK':
                    raise ValueError('positive mock reachability control failed')
        for role, (binary, journal, credential, _) in ROLES.items():
            # Invalid synthetic tokens: never read a production credential.
            key = ('SYNTHETIC-NOT-A-REAL-CREDENTIAL-' + role).encode()
            cipher = destination / (role + '.synthetic.cred')
            result = subprocess.run(['/usr/bin/systemd-creds', 'encrypt', '--with-key=host', '--name=' + credential, '-', str(cipher)],
                                    input=key, capture_output=True, timeout=30, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
            if result.returncode:
                raise ValueError('synthetic credential encryption failed; diagnostics withheld')
            cipher.chmod(0o600)
            state = STATE / role
            ready, output = (state / ('.qualification-' + kind + '-' + token) for kind in ('ready', 'result'))
            markers.extend([ready, output, Path(str(ready) + '.writing'), Path(str(output) + '.writing')])
            user = C.pwd.getpwnam(role)
            data = {'role': role, **before['runtime'][role], 'groups': os.getgrouplist(role, user.pw_gid),
                    'namespace': os.readlink(namespace_path), 'credential_name': credential,
                    'credential_sha256': hashlib.sha256(key).hexdigest(), 'state': str(state), 'journal': str(state / journal),
                    'foreign_paths': [str(STATE / other / ROLES[other][1]) for other in ROLES if other != role]
                        + [str(C.DATABASE), '/etc/credstore.encrypted'],
                    'immutable_paths': [str(BIN / binary), str(ETC / role / 'config.toml')],
                    'foreign_credentials': [str(Path('/run/credentials') / names[other] / ROLES[other][2]) for other in ROLES if other != role],
                    'ready': str(ready), 'result': str(output), 'release': str(public / 'release'), 'port': port}
            config = public / (role + '.json')
            config.write_text(json.dumps(data))
            config.chmod(0o644)
            raw = render_unit(before['units'][role], role, probe, config, cipher, namespace_path).encode()
            path = RUN_UNITS / names[role]
            with path.open('xb') as stream:
                os.fchmod(stream.fileno(), 0o644)
                stream.write(raw)
            units.append((path, hashlib.sha256(raw).hexdigest()))
        C.command('/usr/bin/systemctl', 'daemon-reload')
        for role in ROLES:
            C.command('/usr/bin/systemctl', 'start', names[role])
        for kind in ('ready', 'result'):
            if kind == 'result':
                (public / 'release').write_text('release')
                (public / 'release').chmod(0o644)
            deadline = time.monotonic() + 25
            paths = [STATE / r / ('.qualification-' + kind + '-' + token) for r in ROLES]
            while not all(p.exists() for p in paths):
                if time.monotonic() > deadline:
                    raise ValueError('policy probe timed out; retain private qualification evidence')
                time.sleep(.1)
        for role in ROLES:
            path = STATE / role / ('.qualification-result-' + token)
            info = path.lstat()
            if not stat.S_ISREG(info.st_mode) or info.st_uid != before['runtime'][role]['uid'] or info.st_nlink != 1 or info.st_size > 4096:
                raise ValueError('invalid policy probe result')
            results[role] = check_result(role, json.loads(path.read_text()))
        return results
    except Exception as error:
        logs = {}
        for path, _ in units:
            output = subprocess.run(['/usr/bin/journalctl', '--no-pager', '-u', path.name, '-n', '20'],
                                    capture_output=True, text=True, timeout=10,
                                    env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
            logs[path.name] = output.stdout[-6000:]
        C.write_json(destination / 'FAILURE.json', {'error_type': type(error).__name__, 'probe_journals': logs})
        raise
    finally:
        # Stop/remove only the exact units created by this invocation. A changed
        # file is retained for inspection; never replace an unrelated unit.
        errors = []
        for path, digest in units:
            try:
                if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                    raise ValueError('probe unit changed')
                C.command('/usr/bin/systemctl', 'stop', path.name)
                path.unlink()
            except Exception:
                errors.append(path.name)
        if units:
            C.command('/usr/bin/systemctl', 'daemon-reload')
        for path in markers:
            if path.exists() or path.is_symlink():
                path.unlink()
        for server, stopped, thread in servers:
            stopped.set()
            server.close()
            thread.join(timeout=1)
        if errors:
            raise ValueError('some temporary probe units require inspection; no application restart requested')


def qualify(installation, destination, approved):
    C.root()
    if not approved:
        raise ValueError('explicit disabled-role policy probe flag required')
    namespace()  # Before NSS/filesystem/service/credential side effects.
    C.protected(C.BASE, directory=True, private=True)
    C.protected(Path('/var/lib/systemd/credential.secret'), private=True)
    if (destination.parent != C.BASE or installation.parent != C.BASE
            or any(re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', p.name) is None for p in (installation, destination))):
        raise ValueError('fresh private destination must be directly under the upgrade base')
    before = baseline(installation)
    destination.mkdir(mode=0o700)
    C.TX.sync_directory(destination.parent)
    C.write_json(destination / 'INCOMPLETE.json', {'accepted': False, 'application_workers_started': False})
    C.write_json(destination / 'BASELINE.json', before)
    results = probes(before, destination)
    if baseline(installation) != before:
        raise ValueError('installed journals/configuration/units/core process changed during qualification')
    receipt = {'accepted': True, 'actual_installed_os_policy_probes': 3, 'results': results,
               'journals_unchanged': True, 'core_process_and_unit_unchanged': True,
               'application_workers_started': False, 'real_credentials_loaded': False,
               'provider_qualified': False, 'household_qualified': False, 'dns_qualified': False,
               'rust_socket_protocol_qualified': False}
    C.write_json(destination / 'RESULT.json', receipt)
    (destination / 'INCOMPLETE.json').unlink()
    C.TX.sync_directory(destination)
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--installation', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--probe-disabled-roles', action='store_true', required=True)
    args = parser.parse_args()
    print(json.dumps(qualify(args.installation, args.destination, args.probe_disabled_roles), sort_keys=True))


if __name__ == '__main__':
    main()
