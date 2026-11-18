"""Start verified Cashu release binaries in disposable systemd sandboxes."""
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import tarfile
import tempfile
import time


SCRIPTS = Path(__file__).resolve().parent
WALLET = 'lightning-goats-cashud'
SIGNER = 'lightning-goats-cashu-signerd'
FENCE = 'CASHU_RECOVERY_REQUIRED'
PROBE = '''
import os
from pathlib import Path
import socket
import subprocess
import sys

role, namespace, state, other_state, config, binary, source, port = sys.argv[1:]
assert os.readlink('/proc/self/ns/net') == namespace
assert os.geteuid() != 0 and os.getegid() != 0
assert os.access(state, os.W_OK)
assert not os.access(other_state + '/mount-probe', os.R_OK)
assert any(line.split()[4] == other_state for line in Path('/proc/self/mountinfo').read_text().splitlines())
assert not os.access(config, os.W_OK) and not os.access(binary, os.W_OK)
assert not os.access(source, os.R_OK)
status = dict(line.split(':', 1) for line in Path('/proc/self/status').read_text().splitlines() if ':' in line)
assert status['NoNewPrivs'].strip() == '1' and int(status['CapEff'], 16) == 0
credentials = os.environ.get('CREDENTIALS_DIRECTORY')
if role == 'signer':
    directory = Path(credentials)
    assert list(directory.iterdir()) == [directory / 'cashu-nostr-client']
    assert (directory / 'cashu-nostr-client').read_text() == '0' * 63 + '2'
else:
    assert credentials is None
try:
    with socket.create_connection(('127.0.0.1', int(port)), timeout=1):
        raise AssertionError('shipped systemd IPAddressDeny was not enforced')
except (PermissionError, TimeoutError):
    pass
subprocess.run([binary, 'check-config', config], check=True)
print('sandbox probe passed')
'''


def snapshot(path):
    return {file.name: hashlib.sha256(file.read_bytes()).hexdigest()
            for file in path.iterdir() if file.is_file()}


def diagnostics(command, process, log):
    status = command(['systemctl', 'show', process.unit,
                      '--property=LoadState,Result,ExecStartPre,ExecMainCode,ExecMainStatus,ActiveState'],
                     check=False).stdout
    journal = command(['journalctl', '--unit', process.unit, '--no-pager', '--output=cat', '-n', '20'],
                      check=False).stdout
    return '\n'.join((status[-1800:], journal[-3000:], log.read_text()[-2000:]))


def ready(command, process, endpoint, log):
    for _ in range(100):
        if endpoint.is_socket() and process.properties().get('ActiveState') == 'active':
            return
        if process.poll() is not None:
            break
        time.sleep(0.1)
    raise ValueError(f'{process.unit} did not start:\n{diagnostics(command, process, log)}')


def rehearse(selection, binaries, hashes, uids, states, wallet_config, signer_config,
             helper, helper_state, credential):
    if os.environ.get('GITHUB_ACTIONS') != 'true' or not os.environ.get('GITHUB_RUN_ID', '').isdigit():
        raise ValueError('systemd fixture requires the disposable Actions runner')
    archive, source, checksum = selection
    archive = Path(archive).resolve(strict=True)
    package = runpy.run_path(str(SCRIPTS / 'package-cashu.py'))
    systemd = runpy.run_path(str(SCRIPTS / 'rehearse-systemd.py'))
    systemd['INSTALL'].require_isolation()
    store = Path('/var/lib/systemd/credential.secret')
    runpy.run_path(str(SCRIPTS / 'initialize-staging-credential-store.py'))['inspect_key'](store)
    if package['file_sha256'](archive) != checksum:
        raise ValueError('systemd rehearsal requires a checksum-verified package')
    proof = package['verify'](archive, source)
    if proof['profile'] != 'release':
        raise ValueError('systemd rehearsal requires a release-profile package')
    command = systemd['command']
    roles = {'wallet': (WALLET, 'lightning-goats-cashu'),
             'signer': (SIGNER, 'lightning-goats-cashu-signer')}
    for role, binary in [('core', 'lightning-goatsctl'), ('wallet', WALLET), ('signer', SIGNER)]:
        with tarfile.open(archive, 'r:gz') as tar, tar.extractfile(binary) as stream:
            if hashes[role] != hashlib.file_digest(stream, 'sha256').hexdigest():
                raise ValueError('initialized fixture executable differs from release archive')

    with tempfile.TemporaryDirectory(prefix='lg-cashu-systemd-', dir='/var/lib') as temporary, ExitStack() as cleanup:
        root = Path(temporary)
        root.chmod(0o755)
        installed, configs, endpoints, state_dirs, logs = {}, {}, {}, {}, {}
        templates, accounts = {}, {}
        encrypted = root / 'synthetic-cashu-client.enc'
        command(['systemd-creds', 'encrypt', '--with-key=host', '--name=cashu-nostr-client',
                 str(credential), str(encrypted)])
        encrypted.chmod(0o400)
        probe = root / 'probe.py'
        probe.write_text(PROBE)
        probe.chmod(0o444)
        for role, (binary, name) in roles.items():
            account = f'{root.name}-{role[0]}'
            uid = str(uids[role])
            command(['/usr/sbin/groupadd', '--gid', uid, account])
            cleanup.callback(command, ['/usr/sbin/groupdel', account], check=False)
            command(['/usr/sbin/useradd', '--uid', uid, '--gid', account, '--no-user-group',
                     '--no-create-home', '--home-dir', '/nonexistent', '--shell', '/usr/sbin/nologin', account])
            cleanup.callback(command, ['/usr/sbin/userdel', account], check=False)
            accounts[role] = account
            installed[role] = root / binary
            shutil.copyfile(binaries[role], installed[role])
            installed[role].chmod(0o555)
            template_path = f'deploy/systemd/{name}.service.example'
            with tarfile.open(archive, 'r:gz') as tar, tar.extractfile(template_path) as stream:
                template = root / (name + '.service.example')
                template.write_bytes(stream.read())
            templates[role] = systemd['service_properties'](template)
            state_dirs[role] = root / role
            shutil.copytree(states[role], state_dirs[role])
            witness = state_dirs[role] / 'mount-probe'
            witness.touch(mode=0o444)
            for path in [state_dirs[role], *state_dirs[role].iterdir()]:
                os.chown(path, uids[role], uids[role])
            runtime = Path('/run') / f'{root.name}-{role}'
            endpoints[role] = runtime / 'intake.sock'
            if runtime.exists():
                raise ValueError('unexpected preexisting rehearsal runtime directory')
            cleanup.callback(systemd['remove_directory'], runtime)
            logs[role] = root / f'{role}.log'
            logs[role].touch(mode=0o600)
        helper_dir = root / 'nak'
        shutil.copytree(helper_state, helper_dir)
        os.chown(helper_dir, uids['signer'], uids['signer'])
        helper_bin = root / 'unused-nak'
        shutil.copyfile(helper, helper_bin)
        helper_bin.chmod(0o555)
        for role, original in [('wallet', wallet_config), ('signer', signer_config)]:
            value = json.loads(original.read_bytes())
            value['state_directory'] = str(state_dirs[role])
            if role == 'wallet':
                value['intake_socket'] = str(endpoints[role])
            else:
                value['socket'] = str(endpoints[role])
                value['nak']['private_directory'] = str(helper_dir)
                value['nak']['binary'] = str(helper_bin)
            configs[role] = root / f'{role}.json'
            configs[role].write_text(json.dumps(value))
            configs[role].chmod(0o444)

        listener = cleanup.enter_context(socket.socket())
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        port = listener.getsockname()[1]
        with socket.create_connection(('127.0.0.1', port), timeout=2):
            listener.accept()[0].close()

        def launch(role, phase):
            other = 'signer' if role == 'wallet' else 'wallet'
            state = str(state_dirs[role])
            runtime = endpoints[role].parent
            args = [str(probe), role, os.readlink('/proc/self/ns/net'), state,
                    str(state_dirs[other]), str(configs[role]), str(installed[role]),
                    str(encrypted), str(port)]
            replacement = {
                'User': accounts[role], 'Group': accounts[role],
                'SupplementaryGroups': accounts['signer'],
                'StateDirectory': f'{root.name}/{role}' + (f' {root.name}/nak' if role == 'signer' else ''),
                'RuntimeDirectory': runtime.name,
                'ReadWritePaths': f'{state} {runtime}' + (f' {helper_dir}' if role == 'signer' else ''),
                'LoadCredentialEncrypted': f'cashu-nostr-client:{encrypted}',
            }
            unit = f'{root.name}-{role}-{phase}.service'
            process = systemd['UnitProcess'](unit)
            cleanup.callback(command, ['systemctl', 'reset-failed', unit], check=False)
            cleanup.callback(process.terminate)
            properties = []
            for key, value in templates[role]:
                if key in ('ExecStartPre', 'ExecStart'):
                    continue
                if key == 'InaccessiblePaths':
                    value += f' {state_dirs[other]} {credential.parent}'
                properties.append((key, replacement.get(key, value)))
            properties += [('NetworkNamespacePath', f'/proc/{os.getpid()}/ns/net'),
                           ('ExecStartPre', ' '.join(['/usr/bin/python3', *args])),
                           ('StandardOutput', f'append:{logs[role]}'),
                           ('StandardError', f'append:{logs[role]}')]
            if phase == 'fenced':
                properties = [(key, value) for key, value in properties if key != 'Restart']
                properties += [('Restart', 'no'), ('CollectMode', 'inactive')]
            result = command(['systemd-run', '--unit', unit,
                              *[f'--property={k}={v}' for k, v in properties], '--',
                              str(installed[role]), 'run', str(configs[role])], check=False)
            if result.returncode and phase != 'fenced':
                raise ValueError(f'{role} sandbox launch exited {result.returncode}: '
                                 f'{(result.stdout + result.stderr)[-1400:]}\n'
                                 f'{diagnostics(command, process, logs[role])}')
            return process

        for phase in ('fresh', 'restart'):
            processes = {}
            for role in roles:
                logs[role].write_text('')
                process = launch(role, phase)
                ready(command, process, endpoints[role], logs[role])
                if 'sandbox probe passed' not in logs[role].read_text():
                    raise ValueError('service did not pass installed sandbox checks')
                runtime = Path(f'/proc/{process.pid}/status').read_text()
                status = dict(line.split(':', 1) for line in runtime.splitlines() if ':' in line)
                if set(map(int, status['Uid'].split())) != {uids[role]}:
                    raise ValueError('systemd runtime UID differs from registration')
                processes[role] = process
            for process in processes.values():
                process.terminate()
                if process.wait(timeout=15):
                    raise ValueError('Cashu runtime failed to stop cleanly')

        for role in roles:
            marker = state_dirs[role] / FENCE
            marker.touch(mode=0o600, exist_ok=False)
            os.chown(marker, uids[role], uids[role])
            before = snapshot(state_dirs[role])
            logs[role].write_text('')
            process = launch(role, 'fenced')
            stopped = process.wait(timeout=18)
            log = logs[role].read_text()
            if (stopped != 1 or endpoints[role].exists() or 'sandbox probe passed' not in log
                    or 'Cashu recovery requires explicit reconciliation' not in log):
                raise ValueError(f'{role} recovery fence rehearsal failed:\n'
                                 f'{diagnostics(command, process, logs[role])}')
            if before != snapshot(state_dirs[role]):
                raise ValueError('fenced installed start changed retained state')

    return dict(result='CASHU_RELEASE_SYSTEMD_VERIFIED', source_commit=source,
                archive_sha256=checksum, encrypted_signer_credential=True,
                repeated_start_and_stop=True, distinct_runtime_uids=True,
                loopback_ip_denied_by_systemd=True, installed_fences_preserved=True,
                scope='synthetic empty wallet and signer; no external or financial effects')
