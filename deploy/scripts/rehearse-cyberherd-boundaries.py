#!/usr/bin/env python3
"""TEST ONLY: effective CyberHerd unit boundaries on a disposable systemd VM.

Starts short-lived Python probes with the shipped unit protection/identity policy,
never application workers. Credentials are synthetic, servers are mock endpoints
inside a private network namespace, and installed journal bytes are unchanged.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import socket
import subprocess
import tempfile
import threading
import time
import uuid
import sys

ROLES = {'cyberherd': ('cyberherdd', 'cyberherd.db', 'nostr-client-key', 'cyberherd-nostr-client'),
         'cyberherd-payout': ('cyberherd-payoutd', 'payout.db', 'strike-payout-api-key', 'cyberherd-strike-payout'),
         'cyberherd-signer': ('cyberherd-signerd', 'signer.db', 'nostr-client-key', 'cyberherd-signer-client')}


def run(*args):
    result = subprocess.run(args, capture_output=True, text=True, timeout=30,
                            env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'})
    if result.returncode:
        raise RuntimeError(f'test-only command failed: {args[0]}\n{result.stderr[-4000:]}')
    return result.stdout


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
 try:
  fd=os.open(target,os.O_WRONLY|os.O_APPEND)
 except PermissionError:pass
 except OSError as e:assert e.errno==30
 else:
  os.close(fd);raise AssertionError('managed file is writable')
assert os.access(c['journal'],os.R_OK)
for target in c['foreign_journals']:assert not os.access(target,os.R_OK)
with tempfile.TemporaryFile(dir=c['state']) as f:f.write(b'test-only unnamed write')
def reachable(host,family):
 with socket.socket(family,socket.SOCK_STREAM) as sock:
  sock.settimeout(1)
  try:
   sock.connect((host,c['port']));return sock.recv(2)==b'OK'
  except OSError:return False
assert reachable('203.0.113.19',socket.AF_INET)
assert not reachable('10.8.0.6',socket.AF_INET)
assert not reachable('fd00:19::6',socket.AF_INET6)
pathlib.Path(c['ready']).write_text('ready')
deadline=time.monotonic()+25
while not pathlib.Path(c['release']).exists():
 assert time.monotonic()<deadline
 time.sleep(.05)
for path in c['foreign_credentials']:assert not os.access(path,os.R_OK)
pathlib.Path(c['result']).write_text(json.dumps({'role':c['role'],'uid_verified':True,'kernel_policy_verified':True,
 'credential_scope_verified':True,'foreign_state_denied':True,'managed_files_immutable':True,
 'private_ipv4_ipv6_denied':True,'public_mock_reachable':True}))
'''


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--disposable-ci-host', action='store_true', required=True)
    args = parser.parse_args()
    if (os.geteuid() != 0 or not args.disposable_ci_host or os.environ.get('GITHUB_ACTIONS') != 'true'
            or not os.environ.get('GITHUB_RUN_ID', '').isdigit()):
        raise ValueError('explicit disposable root CI host required')
    if os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('never use the host network namespace')
    if {v['ifname'] for v in json.loads(run('/usr/sbin/ip', '-j', 'link', 'show'))} != {'lo'}:
        raise ValueError('fresh loopback-only namespace required')
    for role in ROLES:
        assert os.readlink(Path('/etc/systemd/system') / (role + '.service')) == '/dev/null'
    run('/usr/sbin/ip', 'link', 'set', 'lo', 'up')
    for address in ['203.0.113.19/32', '10.8.0.6/32']:
        run('/usr/sbin/ip', 'address', 'add', address, 'dev', 'lo')
    run('/usr/sbin/ip', '-6', 'address', 'add', 'fd00:19::6/128', 'dev', 'lo', 'nodad')
    port = 18789
    servers = [listener(socket.AF_INET, port), listener(socket.AF_INET6, port)]
    # Positive controls prove private endpoints are actually reachable in this
    # namespace before attributing service failures to their cgroup policy.
    for family, address in [(socket.AF_INET, '203.0.113.19'), (socket.AF_INET, '10.8.0.6'), (socket.AF_INET6, 'fd00:19::6')]:
        with socket.socket(family, socket.SOCK_STREAM) as connection:
            connection.settimeout(2);connection.connect((address, port));assert connection.recv(2) == b'OK'
    root = Path(tempfile.mkdtemp(prefix='lg-cyberherd-boundaries-', dir='/run'))
    root.chmod(0o755)
    probe = root / 'probe.py'
    probe.write_text(PROBE)
    probe.chmod(0o644)
    token = uuid.uuid4().hex[:12]
    names = {role: 'lg-boundary-' + role + '-' + token + '.service' for role in ROLES}
    journals = {role: Path('/var/lib') / role / data[1] for role, data in ROLES.items()}
    before = {role: hashlib.sha256(path.read_bytes()).hexdigest() for role, path in journals.items()}
    units, markers, results = [], [], {}
    try:
        for role, (binary, journal, credential_name, filename) in ROLES.items():
            state = Path('/var/lib') / role
            cipher = Path('/etc/credstore.encrypted') / filename
            if role == 'cyberherd-payout':
                key = b'test-only-no-installed-spend-key'
            else:
                key = ('TEST-ONLY-NOT-A-REAL-NOSTR-CLIENT-' + role).encode()
                assert not cipher.exists() and not cipher.is_symlink()
                plain = root / (role + '.input')
                plain.write_bytes(key);plain.chmod(0o600)
                run('/usr/bin/systemd-creds', 'encrypt', '--with-key=host', '--name=' + credential_name, str(plain), str(cipher))
                cipher.chmod(0o600)
            user = pwd.getpwnam(role)
            ready, result = state / ('.boundary-ready-' + token), state / ('.boundary-result-' + token)
            markers.extend([ready, result])
            config = {'role': role, 'uid': user.pw_uid, 'gid': user.pw_gid, 'groups': os.getgrouplist(role, user.pw_gid),
                      'namespace': os.readlink('/proc/self/ns/net'), 'credential_name': credential_name,
                      'credential_sha256': hashlib.sha256(key).hexdigest(), 'state': str(state),
                      'journal': str(journals[role]), 'foreign_journals': [str(path) for other, path in journals.items() if other != role],
                      'immutable_paths': [str(Path('/usr/local/bin') / binary), str(Path('/etc') / role / 'config.toml')],
                      'port': port, 'ready': str(ready), 'result': str(result), 'release': str(root / 'release'),
                      'foreign_credentials': [str(Path('/run/credentials') / names[other] / ROLES[other][2]) for other in ROLES if other != role]}
            path = root / (role + '.json')
            path.write_text(json.dumps(config));path.chmod(0o644)
            text = (Path('/usr/local/lib/systemd/system') / (role + '.service')).read_text()
            expected = f'ExecStart=/usr/local/bin/{binary} --config /etc/{role}/config.toml'
            assert text.count(expected) == 1
            text = text.replace(expected, f'ExecStart=/usr/bin/python3 -I -S -B {probe} {path}')
            text = text.replace('Restart=on-failure', 'Restart=no')
            if role == 'cyberherd':
                text = text.replace('# LoadCredentialEncrypted=', 'LoadCredentialEncrypted=')
            text = text.replace('[Install]', 'NetworkNamespacePath=' + str(Path('/proc') / str(os.getpid()) / 'ns/net') + '\n\n[Install]')
            unit_path = Path('/run/systemd/system') / names[role]
            assert not unit_path.exists()
            unit_path.write_text(text);unit_path.chmod(0o644)
            units.append(unit_path)
        run('/usr/bin/systemctl', 'daemon-reload')
        for role in ROLES:
            run('/usr/bin/systemctl', 'start', names[role])
        deadline = time.monotonic() + 25
        while not all(path.exists() for path in markers[::2]):
            if time.monotonic() > deadline:
                raise RuntimeError('boundary probes did not become ready; retain CI journal evidence')
            time.sleep(.1)
        (root / 'release').write_text('release')
        (root / 'release').chmod(0o644)
        while not all(path.exists() for path in markers[1::2]):
            if time.monotonic() > deadline:
                raise RuntimeError('boundary probes did not complete; retain CI journal evidence')
            time.sleep(.1)
        for role in ROLES:
            results[role] = json.loads((Path('/var/lib') / role / ('.boundary-result-' + token)).read_text())
            assert hashlib.sha256(journals[role].read_bytes()).hexdigest() == before[role]
        print(json.dumps({'synthetic': True, 'actual_systemd_policy_probes': 3, 'results': results,
                          'application_workers_started': False, 'journals_unchanged': True,
                          'provider_qualified': False, 'household_qualified': False, 'live_host_qualified': False}, sort_keys=True))
    except Exception:
        for role in ROLES:
            report = subprocess.run(['/usr/bin/journalctl', '--no-pager', '-u', names[role], '-n', '20'],capture_output=True,text=True)
            print(report.stdout[-6000:], file=sys.stderr)
        raise
    finally:
        for path in units:
            subprocess.run(['/usr/bin/systemctl', 'stop', path.name],capture_output=True,timeout=30)
            path.unlink()
        if units:
            run('/usr/bin/systemctl', 'daemon-reload')
        for path in markers:
            if path.exists():path.unlink()
        for server, stopped, thread in servers:
            stopped.set();server.close();thread.join(timeout=1)


if __name__ == '__main__':
    main()
