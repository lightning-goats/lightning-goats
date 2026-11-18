"""Start verified Cashu release binaries in disposable systemd sandboxes."""
from contextlib import ExitStack, closing
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import sqlite3
import tarfile
import tempfile
import time


SCRIPTS = Path(__file__).resolve().parent
WALLET = 'lightning-goats-cashud'
SIGNER = 'lightning-goats-cashu-signerd'
FENCE = 'CASHU_RECOVERY_REQUIRED'
ROTATED_RECEIVER = '02e493dbf1c10d80f3581e4904930b1404cc6c13900ee0758474fa94abe8c4cd13'
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
    client = directory / 'cashu-nostr-client'
    assert list(directory.iterdir()) == [client]
    assert client.read_text() == '0' * 63 + '2'
    print('systemd credential modes', oct(directory.stat().st_mode & 0o7777),
          oct(client.stat().st_mode & 0o7777), 'read-only mount',
          bool(os.statvfs(directory).f_flag & os.ST_RDONLY))
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
        with socket.socket(socket.AF_UNIX) as probe:
            probe.settimeout(0.1)
            try:
                reachable = probe.connect_ex(str(endpoint)) == 0
            except TimeoutError:
                reachable = False
        if reachable and process.properties().get('ActiveState') == 'active':
            return
        if process.poll() is not None:
            break
        time.sleep(0.1)
    raise ValueError(f'{process.unit} did not start:\n{diagnostics(command, process, log)}')


def inspect_wallet(directory):
    database = directory / 'acquisition.sqlite'
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as db:
        return (db.execute('SELECT * FROM runtime_registration').fetchall(),
                db.execute('SELECT * FROM wallet_epoch').fetchall(),
                db.execute('SELECT * FROM cdk_mint_namespaces ORDER BY mint_id').fetchall())


def inspect_signer(directory):
    database = directory / 'cashu-signing.sqlite'
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as db:
        return (db.execute('SELECT binding FROM registration').fetchone(),
                db.execute('SELECT id,request,attempts,signed_event FROM requests ORDER BY id').fetchall())


def rehearse(selection, binaries, hashes, uids, states, wallet_config, signer_config,
             helper, helper_state, credential, request_announcement):
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
        settings = {}
        for role, original in [('wallet', wallet_config), ('signer', signer_config)]:
            value = json.loads(original.read_bytes())
            value['state_directory'] = str(state_dirs[role])
            if role == 'wallet':
                value['intake_socket'] = str(endpoints[role])
            else:
                value['socket'] = str(endpoints[role])
                value['nak']['private_directory'] = str(helper_dir)
                value['nak']['binary'] = str(helper_bin)
                value['policy'].update(announcements=True, max_new_per_hour=3, max_attempts=1)
            settings[role] = value
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
            expected_refusal = phase in ('fenced', 'missing-key', 'wrong-key')
            if expected_refusal:
                properties = [(key, value) for key, value in properties if key != 'Restart']
                properties += [('Restart', 'no'), ('CollectMode', 'inactive')]
            result = command(['systemd-run', '--unit', unit,
                              *[f'--property={k}={v}' for k, v in properties], '--',
                              str(installed[role]), 'run', str(configs[role])], check=False)
            if result.returncode and not expected_refusal:
                raise ValueError(f'{role} sandbox launch exited {result.returncode}: '
                                 f'{(result.stdout + result.stderr)[-1400:]}\n'
                                 f'{diagnostics(command, process, logs[role])}')
            return process

        def start_pair(phase):
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
            return processes

        def stop_pair(processes):
            for process in processes.values():
                process.terminate()
                if process.wait(timeout=15):
                    raise ValueError('Cashu runtime failed to stop cleanly')

        wallet_state = state_dirs['wallet']
        signer_state = state_dirs['signer']
        before_wallet = inspect_wallet(wallet_state)
        before_signer, no_requests = inspect_signer(signer_state)
        anchor_names = ('wallet-seed.bin', 'receiving-key.bin')
        anchor_hashes = {name: package['file_sha256'](wallet_state / name) for name in anchor_names}
        wallet_settings, policy = settings['wallet'], settings['signer']['policy']
        old_receiver = wallet_settings['receiving_key']
        if (no_requests or policy['receiving_key'] != old_receiver[2:]
                or ROTATED_RECEIVER == old_receiver):
            raise ValueError('unexpected source receiving policy or prior signing work')
        processes = start_pair('fresh')
        original = request_announcement(endpoints['signer'], uids['wallet'], uids['signer'], policy)
        stop_pair(processes)
        binding, original_rows = inspect_signer(signer_state)
        if (binding != before_signer or len(original_rows) != 1
                or (original_rows[0][0], original_rows[0][2], original_rows[0][3]) != (original, 1, None)):
            raise ValueError('original synthetic signer attempt was not retained')

        rotated_file = wallet_state / f'receiving-key-{ROTATED_RECEIVER}.bin'
        wallet_settings.update(receiving_key_anchor=old_receiver, receiving_key=ROTATED_RECEIVER,
                               retained_receiving_keys=[{'public_key': old_receiver}])
        policy['receiving_key'] = ROTATED_RECEIVER[2:]
        for role in roles:
            configs[role].write_text(json.dumps(settings[role]))

        for phase in ('missing-key', 'wrong-key'):
            if phase == 'wrong-key':
                with rotated_file.open('xb') as output:
                    os.fchmod(output.fileno(), 0o600)
                    os.fchown(output.fileno(), uids['wallet'], uids['wallet'])
                    output.write(bytes(31) + bytes([1]))
            before = snapshot(wallet_state)
            logs['wallet'].write_text('')
            process = launch('wallet', phase)
            stopped = process.wait(timeout=18)
            if (stopped != 1 or endpoints['wallet'].exists()
                    or 'wallet startup rejected' not in logs['wallet'].read_text()
                    or snapshot(wallet_state) != before):
                raise ValueError(f'{phase} changed or opened installed wallet state:\n'
                                 + diagnostics(command, process, logs['wallet']))
        rotated_file.write_bytes(bytes(31) + bytes([4]))

        processes = start_pair('restart')
        stale = request_announcement(endpoints['signer'], uids['wallet'], uids['signer'], policy,
                                     receiving_key=old_receiver[2:])
        current = request_announcement(endpoints['signer'], uids['wallet'], uids['signer'], policy)
        stop_pair(processes)
        binding, rows = inspect_signer(signer_state)
        retained = {row[0]: row for row in rows}
        if (binding != before_signer or set(retained) != {original, current}
                or retained[original] != original_rows[0] or stale in retained
                or retained[current][2:] != (1, None)
                or json.loads(retained[current][1])['intent']['receiving_key'] != ROTATED_RECEIVER[2:]
                or (helper_dir / 'invocations').read_bytes() != b'attempt\nattempt\n'):
            raise ValueError('rotated signer changed original history or accepted stale policy')
        if (inspect_wallet(wallet_state) != before_wallet
                or {name: package['file_sha256'](wallet_state / name) for name in anchor_names} != anchor_hashes):
            raise ValueError('rotation changed the wallet anchor, seed, registration or mint namespaces')

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
                receiving_key_rotation={'wallet_anchor_and_namespace_unchanged': True,
                                        'missing_and_wrong_key_refused': True,
                                        'signer_original_request_unchanged': True,
                                        'stale_announcement_refused': True,
                                        'new_announcement_reached_inert_helper': True},
                scope='synthetic receiving-key rotation with inert signer; no external or financial effects')
