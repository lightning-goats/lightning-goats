"""Rehearse paused restart of an advanced checkpoint on a disposable installation."""
from contextlib import ExitStack, closing, contextmanager
import fcntl
import hashlib
import http.client
import json
import os
from pathlib import Path
import select
import shutil
import signal
import socket
import sqlite3
import stat
import subprocess
import time
import tomllib


CORE_TABLES = (
    'ledger_entries', 'feed_attempts', 'strike_receive_requests', 'cashu_receipt_inbox',
    'cashu_treasury_issuances', 'cashu_treasury_deposits', 'cashu_treasury_completions', 'message_outbox',
)
WALLET_TABLES = ('acquisitions', 'treasury_instructions', 'treasury_results', 'treasury_cdk_cleanup')
ENABLED_IN_WALLET_CONFIG = (
    'acquisition_enabled', 'reconciliation_enabled', 'receipt_delivery_enabled',
    'treasury_intake_enabled', 'treasury_admission_enabled', 'treasury_execution_enabled',
    'treasury_reconciliation_enabled',
)
CLEAN_ENV = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
PAYMENT_AND_PRESENTATION_ROUTES = (
    ('GET', '/.well-known/lnurlp/herd'), ('GET', '/lnurlp/herd/callback?amount=1000'),
    ('POST', '/api/v1/strike/webhook'), ('POST', '/api/v1/contact'),
    ('POST', '/api/v1/cashu'), ('GET', '/api/v1/cashu/request'),
    ('POST', '/api/v1/cashu/status'), ('GET', '/ws/overlay'),
)


def rows(database, queries):
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as connection:
        return {key: connection.execute(query).fetchall() for key, query in queries.items()}


def financial_state(directories):
    core = rows(directories['core'] / 'ledger.sqlite', {
        **{table: f'SELECT * FROM {table} ORDER BY rowid' for table in CORE_TABLES},
        'financial_events': """SELECT * FROM event_log
            WHERE event_type IN ('payment_received', 'feeder_confirmed') ORDER BY seq""",
        'deposited_sats': 'SELECT SUM(deposit_sats) FROM cashu_treasury_deposits',
        'confirmed_feeds': "SELECT COUNT(*) FROM feed_attempts WHERE status='confirmed'",
    })
    wallet = rows(directories['wallet'] / 'acquisition.sqlite',
                  {table: f'SELECT * FROM {table} ORDER BY rowid' for table in WALLET_TABLES})
    mints = {path.name: rows(path, {'proofs': 'SELECT * FROM proof ORDER BY rowid',
                                   'sagas': 'SELECT * FROM wallet_sagas ORDER BY rowid'})
             for path in directories['wallet'].glob('mint-*.sqlite')}
    return core, wallet, mints


def as_runtime(worker, *args):
    return ['/usr/bin/setpriv', f"--reuid={worker['uid']}", f"--regid={worker['gid']}",
            '--clear-groups', '--no-new-privs', '--', *map(str, args)]


def private_directory(path, worker, mode=0o700):
    path.mkdir(mode=mode)
    path.chmod(mode)
    os.chown(path, worker['uid'], worker['gid'])
    return path


def config_file(path, text):
    path.write_text(text)
    path.chmod(0o444)
    return path


def credentials(path, worker, values):
    private_directory(path, worker)
    for name, value in values.items():
        secret = path / name
        secret.write_text(value)
        secret.chmod(0o400)
        os.chown(secret, worker['uid'], worker['gid'])
    return {'CREDENTIALS_DIRECTORY': str(path)}


def http_request(port, path, method='GET'):
    with closing(http.client.HTTPConnection('127.0.0.1', port, timeout=1)) as connection:
        connection.request(method, path)
        response = connection.getresponse()
        return response.status, response.read()


def http_json(port, path):
    status, body = http_request(port, path)
    if status != 200:
        raise ValueError(f'synthetic restored core returned HTTP {status}')
    return json.loads(body)


def require_writer_held(lock):
    with lock.open('rb') as contender:
        try:
            fcntl.flock(contender, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        raise ValueError('paused service failed to hold its existing writer exclusion')


def transient_command(unit, account, stop, command, properties):
    user, group = account
    settings = ['Type=exec', f'User={user}', f'Group={group}', 'NoNewPrivileges=yes',
                'TimeoutStopSec=10s', 'KillSignal=' + stop.name, *properties]
    return ['/usr/bin/systemd-run', '--quiet', '--pipe', '--wait', '--collect', '--unit=' + unit,
            *('--property=' + setting for setting in settings), '--', *command]


@contextmanager
def runtime(command, ready, log, stop, environment=None, unit=None):
    def stop_runtime(process, check=True):
        if unit is None:
            process.send_signal(stop)
        else:
            subprocess.run(['/usr/bin/systemctl', 'stop', unit], stdin=subprocess.DEVNULL,
                           capture_output=True, timeout=20, check=check, env=CLEAN_ENV)

    with log.open('w+') as output:
        os.fchmod(output.fileno(), 0o600)
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=output, stderr=output,
                                   env={**CLEAN_ENV, **(environment or {})}, umask=0o077)
        try:
            deadline = time.monotonic() + 15
            while process.poll() is None and time.monotonic() < deadline:
                try:
                    if ready():
                        break
                except OSError:
                    pass
                time.sleep(0.05)
            else:
                output.seek(0)
                raise ValueError(f'{log.stem} synthetic restart failed: {output.read()[-2200:]}')
            yield process
            stop_runtime(process)
            if process.wait(timeout=10):
                output.seek(0)
                raise ValueError(f'{log.stem} synthetic shutdown failed: {output.read()[-2200:]}')
        finally:
            if process.poll() is None:
                if unit is not None:
                    stop_runtime(process, check=False)
                process.kill()
                process.wait(timeout=5)


def installed_fingerprints(directories, extra):
    paths = list(extra)
    for directory in directories:
        paths.extend((directory, *sorted(directory.iterdir())))
    fingerprints = {}
    for path in paths:
        info = path.lstat()
        if stat.S_ISREG(info.st_mode):
            with path.open('rb') as source:
                content = hashlib.file_digest(source, 'sha256').hexdigest()
        elif stat.S_ISLNK(info.st_mode):
            content = os.readlink(path)
        elif stat.S_ISDIR(info.st_mode):
            content = None
        else:
            raise ValueError(f'unexpected installed recovery entry: {path.name}')
        fingerprints[path] = (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid,
                              info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns, content)
    return fingerprints


def require_installed_process(pid, worker, command, unit):
    proc = Path(f'/proc/{pid}')
    expected = b'\0'.join(map(os.fsencode, command)) + b'\0'
    if (proc / 'cmdline').read_bytes() != expected:
        raise ValueError('paused service did not start the exact installed executable and configuration')
    status = dict(line.split(':', 1) for line in (proc / 'status').read_text().splitlines())
    if (any(status[field].split() != [str(worker[key])] * 4
            for field, key in (('Uid', 'uid'), ('Gid', 'gid')))
            or status['NoNewPrivs'].strip() != '1' or int(status['CapEff'].strip(), 16)):
        raise ValueError('paused service did not run under its installed identity')
    if unit is not None:
        namespace = os.readlink(proc / 'ns/net')
        fixture_namespace = os.readlink('/proc/self/ns/net')
        if (namespace == os.readlink('/proc/1/ns/net')
                or (namespace == fixture_namespace) != ('--config' in command)):
            raise ValueError(f'{unit} did not join its isolated recovery network')
        mounts = [line.split()[:6] for line in (proc / 'mountinfo').read_text().splitlines()]
        for target in map(Path, (worker['state'], command[-1])):
            _, _, _, _, _, options = max(
                (row for row in mounts if target.is_relative_to(row[4])),
                key=lambda row: len(Path(row[4]).parts))
            if 'ro' not in options.split(','):
                raise ValueError(f'{unit} did not mount {target.name} read-only')


def require_inactive_masks(workers):
    for worker in workers.values():
        properties = subprocess.check_output(
            ['/usr/bin/systemctl', 'show', '--property=ActiveState,UnitFileState', '--', worker['unit']],
            stdin=subprocess.DEVNULL, env=CLEAN_ENV, timeout=5)
        values = dict(line.split(b'=', 1) for line in properties.splitlines())
        if values[b'ActiveState'] != b'inactive' or values[b'UnitFileState'] not in (b'masked', b'masked-runtime'):
            raise ValueError('ordinary installed unit lost its inactive runtime mask')


def require_core_paused(port):
    if http_json(port, '/api/v1/status') != {
            'mode': 'recovery-paused', 'feed_credit_sats': 16, 'unresolved_feed_attempt': None}:
        raise ValueError('restored core did not preserve its paused state and available credit')
    for method, path in PAYMENT_AND_PRESENTATION_ROUTES:
        if http_request(port, path, method)[0] != 404:
            raise ValueError(f'paused recovery still serves {method} {path}')


def require_wallet_paused(log, wallet):
    if json.loads(log.read_bytes()) != {
            'mode': 'recovery-paused', 'wallet_instance': wallet['wallet_instance'],
            'wallet_epoch': wallet['wallet_epoch'], 'mint_namespaces': 2,
            'journal_custody_sats': 16, 'reported_treasury_deposit_sats': 396,
            'resume_authorized': False}:
        raise ValueError('paused wallet readiness differs from the funded registration')


def require_signer_paused(log, signer):
    if json.loads(log.read_bytes()) != {
            'mode': 'recovery-paused', 'signing_requests': 1, 'client_credential_verified': True,
            'resume_authorized': False, **{key: signer['policy'][key] for key in
                ('broker_instance', 'broker_epoch', 'wallet_instance', 'wallet_epoch')}}:
        raise ValueError('paused signer readiness differs from the restored registration')


def rehearse_installed(root, workers, policy_path, archived_signer_client, systemd_roles, ciphertext):
    # The collector has just authenticated the installation and paused markers.
    root.mkdir(mode=0o711)
    policy = json.loads(policy_path.read_bytes())
    configs = {role: Path(policy[role + '_config']) for role in ('core', 'wallet', 'cashu_signer')}
    commands = {}
    for role, config in configs.items():
        artifact = policy['runtime_artifacts'][role]
        binary = Path(artifact['path'])
        with binary.open('rb') as executable:
            if hashlib.file_digest(executable, 'sha256').hexdigest() != artifact['sha256']:
                raise ValueError(f'{role} installed executable differs from the collection policy')
        mode = ['--recovery-paused', '--config'] if role == 'core' else ['recovery-paused']
        commands[role] = [str(binary), *mode, str(config)]
    core = tomllib.loads(configs['core'].read_text())
    wallet = json.loads(configs['wallet'].read_bytes())
    signer = json.loads(configs['cashu_signer'].read_bytes())
    if (core['database']['url'] != 'sqlite://' + workers['core']['state'] + '/ledger.sqlite'
            or wallet['state_directory'] != workers['wallet']['state']
            or signer['state_directory'] != workers['cashu_signer']['state']):
        raise ValueError('paused startup configuration did not select the installed images')
    host, port = core['service']['listen'].rsplit(':', 1)
    if host != '127.0.0.1':
        raise ValueError('paused startup requires the installed loopback listener')
    port = int(port)
    sockets = [wallet['intake_socket'], signer['socket']]
    for config, name in ((wallet, 'treasury_intake'), (core, 'cashu_receipts')):
        if config.get(name, {}).get('socket'):
            sockets.append(config[name]['socket'])
    helper_invocations = Path(signer['nak']['private_directory']) / 'invocations'
    dirs = [Path(worker['state']) for worker in workers.values()]
    extra = [*configs.values(), helper_invocations]
    before = installed_fingerprints(dirs, extra)
    if helper_invocations.read_bytes() != b'attempt\n':
        raise ValueError('installed signer fixture lacks the original attempt')
    signer_env = {} if systemd_roles else credentials(
        root / 'signer-creds', workers['cashu_signer'],
        {'cashu-nostr-client': archived_signer_client.read_text()})
    logs = {role: root / f'{role}.log' for role in configs}
    ready = {
        'core': lambda: http_json(port, '/healthz') == {'status': 'ok'},
        'wallet': lambda: logs['wallet'].read_bytes().endswith(b'\n'),
        'cashu_signer': lambda: logs['cashu_signer'].read_bytes().endswith(b'\n'),
    }
    for cycle, stop in enumerate((signal.SIGINT, signal.SIGTERM)):
        with ExitStack() as services:
            for role, command in commands.items():
                worker = workers[role]
                unit = None
                if systemd_roles:
                    unit = worker['unit'].removesuffix('.service') + f'-installed-paused-{cycle}.service'
                    network = (f'NetworkNamespacePath=/proc/{os.getpid()}/ns/net'
                               if role == 'core' else 'PrivateNetwork=yes')
                    properties = ['ProtectSystem=strict', network]
                    if role == 'cashu_signer':
                        properties.append(f'LoadCredentialEncrypted=cashu-nostr-client:{ciphertext}')
                    launch = transient_command(unit, systemd_roles[role], stop, command, properties)
                else:
                    launch = as_runtime(worker, *command)
                process = services.enter_context(runtime(
                    launch, ready[role], logs[role], stop, signer_env if role == 'cashu_signer' else None,
                    unit))
                pid = int(subprocess.check_output(
                    ['/usr/bin/systemctl', 'show', '--property=MainPID', '--value', unit],
                    stdin=subprocess.DEVNULL, env=CLEAN_ENV, timeout=10)) if unit else process.pid
                require_installed_process(pid, worker, command, unit)
            require_core_paused(port)
            require_wallet_paused(logs['wallet'], wallet)
            require_signer_paused(logs['cashu_signer'], signer)
            for role in ('wallet', 'cashu_signer'):
                require_writer_held(Path(workers[role]['state']) / 'wallet-writer.lock')
            if systemd_roles:
                require_inactive_masks(workers)
            if any(map(os.path.lexists, sockets)) or installed_fingerprints(dirs, extra) != before:
                raise ValueError('paused startup opened an effectful socket or changed the installed image')
        if any(map(os.path.lexists, sockets)) or installed_fingerprints(dirs, extra) != before:
            raise ValueError('paused shutdown changed the installed image or left a socket')
    return {'start_stop_cycles': 2, 'exact_installed_executables_and_configurations': True,
            'separate_runtime_identities_verified': True, 'all_six_images_unchanged': True,
            'systemd_read_only_mounts_verified': bool(systemd_roles),
            'systemd_ordinary_units_remained_masked': bool(systemd_roles),
            'signer_loaded_archived_systemd_credential': bool(systemd_roles),
            'paused_core_sats': 16, 'paused_wallet_sats': 16, 'signer_requests_retained': 1,
            'effectful_routes_and_sockets_absent': True, 'resume_authorized': False}


def rehearse(root, workers, binaries, configurations, core_daemon, archived_signer_client, encrypted_signer):
    # This helper runs inside the verified loopback-only native backup fixture.
    # Copy the exact installed later image and its paused authorization markers.
    root.mkdir(mode=0o711)
    roles = ('core', 'wallet', 'cashu_signer')
    dirs = {role: private_directory(root / role, workers[role]) for role in roles}
    for role, state in dirs.items():
        for source in Path(workers[role]['state']).iterdir():
            target = state / source.name
            if source.name in ('CYBERHERD_RECOVERY_REQUIRED', 'CASHU_RECOVERY_REQUIRED'):
                target.symlink_to(os.readlink(source))
                os.chown(target, 0, 0, follow_symlinks=False)
                continue
            shutil.copyfile(source, target)
            target.chmod(0o600)
            os.chown(target, workers[role]['uid'], workers[role]['gid'])
    before = financial_state(dirs)
    if (before[0]['deposited_sats'] != [(396,)] or before[0]['confirmed_feeds'] != [(1,)]
            or len(before[0]['cashu_receipt_inbox']) != 3 or len(before[2]) != 2):
        raise ValueError('restart fixture lacks the funded later-stage history')

    fenced = subprocess.run(
        as_runtime(workers['core'], core_daemon, '--config', configurations['core']),
        stdin=subprocess.DEVNULL, capture_output=True, timeout=10, env=CLEAN_ENV)
    if fenced.returncode != 1 or b'requires explicit external reconciliation' not in fenced.stderr:
        raise ValueError('ordinary core started despite the installed paused gate')

    def refuse_wallet(config, reason, mode='recovery-paused'):
        refused = subprocess.run(
            as_runtime(workers['wallet'], binaries['wallet'], mode, config),
            stdin=subprocess.DEVNULL, capture_output=True, timeout=10, env=CLEAN_ENV)
        if refused.returncode != 1 or refused.stdout or reason not in refused.stderr:
            raise ValueError('paused wallet accepted a nonready recovery image')

    refuse_wallet(configurations['wallet'], b'Cashu recovery requires explicit reconciliation', mode='run')

    # Keep feeding-enabled canary settings and configure ordinary Cashu and
    # receipt ingress. Paused recovery must ignore their runtime authority.
    with socket.socket() as listener_port, socket.socket() as outbound_probe:
        listener_port.bind(('127.0.0.1', 0))
        outbound_probe.bind(('127.0.0.1', 0))
        outbound_probe.listen()
        port, outbound_port = listener_port.getsockname()[1], outbound_probe.getsockname()[1]
        listener_port.close()
        original = configurations['core'].read_text()
        values = tomllib.loads(original)
        replacements = {
            values['database']['url']: 'sqlite://' + str(dirs['core'] / 'ledger.sqlite'),
            values['service']['listen']: f'127.0.0.1:{port}',
            values['lnurl']['breez_collector']['socket']: str(root / 'isolated-project.sock'),
            values['gateway']['url']: f'http://127.0.0.1:{outbound_port}/',
        }
        if values['service']['mode'] != 'canary' or set(values) & {'core_bridge', 'cashu', 'cashu_receipts', 'cashu_treasury'}:
            raise ValueError('unexpected core fixture configuration')
        for old, new in replacements.items():
            if original.count(old) != 1:
                raise ValueError('ambiguous synthetic core configuration')
            original = original.replace(old, new)
        receipt_socket = private_directory(root / 'c', workers['core'], 0o710) / 'receipts.sock'
        [(core_epoch,)] = rows(dirs['core'] / 'ledger.sqlite', {
            'epoch': 'SELECT stream_id FROM overlay_identity WHERE singleton=1'})['epoch']
        wallet_run = private_directory(root / 'w', workers['wallet'], 0o710)
        wallet_socket, treasury_socket = wallet_run / 'intake.sock', wallet_run / 'treasury.sock'
        wallet = json.loads(configurations['wallet'].read_bytes())
        wallet.update(state_directory=str(dirs['wallet']), intake_socket=str(wallet_socket),
                      execution={'acquire': True, 'reconcile': True, 'maximum_attempts': 2},
                      receipt_delivery={'enabled': True, 'socket': str(receipt_socket),
                                        'core_epoch': core_epoch, 'maximum_attempts': 2, 'deadline_seconds': 5},
                      treasury_intake={'enabled': True, 'accept_new': True, 'socket': str(treasury_socket),
                                       'core_epoch': core_epoch, 'policies': {'fixture-a': 'fixture-v1'},
                                       'destination': {'origin': f'http://127.0.0.1:{outbound_port}/',
                                                       'account_id': core_epoch, 'network': 'bitcoin',
                                                       'credential_fingerprint': 'ab' * 32}},
                      treasury_execution={'execute': True, 'reconcile': True, 'maximum_attempts': 2,
                                          'maximum_input_sats': 100, 'maximum_in_flight_sats': 100,
                                          'operations_per_24h': 2, 'input_sats_per_24h': 200,
                                          'fee_sats_per_24h': 0})
        wallet_config = config_file(root / 'wallet.json', json.dumps(wallet))
        wallet_state = dirs['wallet']
        lock = wallet_state / 'wallet-writer.lock'
        wallet_log = root / 'wallet.log'
        startup = b'paused wallet recovery rejected'
        wallet_marker = wallet_state / 'CASHU_RECOVERY_REQUIRED'
        os.chown(wallet_marker, workers['wallet']['uid'], workers['wallet']['gid'], follow_symlinks=False)
        try:
            refuse_wallet(wallet_config, b'Cashu recovery requires explicit reconciliation')
        finally:
            os.chown(wallet_marker, 0, 0, follow_symlinks=False)
        for path in (lock, next(wallet_state.glob('mint-*.sqlite'))):
            retained = root / 'retained-wallet-artifact'
            path.rename(retained)
            try:
                refuse_wallet(wallet_config, startup)
                if path.exists():
                    raise ValueError('paused wallet recreated a missing lock or mint store')
            finally:
                retained.rename(path)
        seed = wallet_state / 'wallet-seed.bin'
        original_seed = seed.read_bytes()
        try:
            seed.write_bytes(bytes(64))
            refuse_wallet(wallet_config, startup)
        finally:
            seed.write_bytes(original_seed)
        wallet_bytes = {path: hashlib.sha256(path.read_bytes()).hexdigest()
                        for path in wallet_state.iterdir()
                        if not path.name.endswith(('-wal', '-shm')) and path != wallet_marker}
        registration = f'''
wallet_uid = {wallet['runtime_uid']}
wallet_instance = "{wallet['wallet_instance']}"
wallet_epoch = "{wallet['wallet_epoch']}"
deadline_seconds = 5
'''
        core_config = config_file(root / 'core.toml', original + f'''
[cashu]
socket = "{wallet_socket}"
receive_cashu = true
body_bytes = 65536
concurrent_requests = 4
{registration}
[cashu_receipts]
socket = "{receipt_socket}"
ipc_gid = {workers['core']['gid']}
core_epoch = "{core_epoch}"
connections_per_second = 8
{registration}
''')

        signer_socket = private_directory(root / 's', workers['cashu_signer'], 0o710) / 'broker.sock'
        signer = json.loads(configurations['signer'].read_bytes())
        signer.update(state_directory=str(dirs['cashu_signer']), socket=str(signer_socket))
        signer_config = config_file(root / 'signer.json', json.dumps(signer))
        signer_worker = workers['cashu_signer']
        signer_env = credentials(root / 'signer-creds', signer_worker,
                                 {'cashu-nostr-client': archived_signer_client.read_text()})
        signer_log = root / 'signer.log'
        signer_lock = dirs['cashu_signer'] / 'wallet-writer.lock'
        signer_journal = dirs['cashu_signer'] / 'cashu-signing.sqlite'
        helper_invocations = Path(signer['nak']['private_directory']) / 'invocations'

        def refuse_signer(configuration, reason, config_env=None, mode='recovery-paused'):
            result = subprocess.run(as_runtime(signer_worker, binaries['signer'], mode, configuration),
                                    stdin=subprocess.DEVNULL, capture_output=True, timeout=10,
                                    env={**CLEAN_ENV, **(config_env or {})})
            if result.returncode != 1 or result.stdout or reason not in result.stderr:
                raise ValueError('paused signer accepted a nonready recovery image')

        startup = b'signer startup rejected'
        refuse_signer(configurations['signer'], b'Cashu recovery requires explicit reconciliation', mode='run')
        refuse_signer(signer_config, startup)
        wrong = credentials(root / 'wrong-signer-creds', signer_worker, {'cashu-nostr-client': '0' * 63 + '5'})
        refuse_signer(signer_config, startup, wrong)
        rebound = {**signer, 'policy': {**signer['policy'], 'broker_epoch': wallet['wallet_epoch']}}
        refuse_signer(config_file(root / 'rebound-signer.json', json.dumps(rebound)), startup, signer_env)
        original_signer = signer_journal.read_bytes()
        if (rows(signer_journal, {'requests': 'SELECT attempts,signed_event FROM requests'})['requests'] != [(1, None)]
                or helper_invocations.read_bytes() != b'attempt\n' or not signer['policy']['announcements']):
            raise ValueError('restored signer lacks the pending request with an enabled policy')

        for role, name in [('membership', 'membership'), ('payout', 'payout'), ('signer', 'payout_signer')]:
            command = as_runtime(workers[role], binaries[name], '--config', configurations[name])
            refused = subprocess.run(command, capture_output=True, timeout=10, env=CLEAN_ENV)
            if refused.returncode != 1 or b'disabled' not in refused.stderr:
                raise ValueError(f'{role} did not honor the disabled configuration')

        for cycle, stop in enumerate((signal.SIGINT, signal.SIGTERM)):
            signer_command = [str(binaries['signer']), 'recovery-paused', str(signer_config)]
            signer_unit = None
            if encrypted_signer is None:
                signer_command = as_runtime(signer_worker, *signer_command)
                signer_environment = signer_env
            else:
                ciphertext, account, group = encrypted_signer
                signer_unit = signer_worker['unit'].removesuffix('.service') + f'-paused-{cycle}.service'
                signer_environment = None
                signer_command = transient_command(
                    signer_unit, (account, group), stop, signer_command,
                    [f'LoadCredentialEncrypted=cashu-nostr-client:{ciphertext}', 'PrivateNetwork=yes'])
            with (runtime(as_runtime(workers['core'], core_daemon, '--recovery-paused', '--config', core_config),
                          lambda: http_json(port, '/healthz') == {'status': 'ok'}, root / 'core.log',
                          stop),
                  runtime(signer_command, lambda: signer_log.read_text().endswith('\n'),
                          signer_log, stop, signer_environment, signer_unit),
                  runtime(as_runtime(workers['wallet'], binaries['wallet'], 'recovery-paused', wallet_config),
                          lambda: wallet_log.read_text().endswith('\n'), wallet_log, stop)):
                require_core_paused(port)
                if os.path.lexists(receipt_socket) or select.select([outbound_probe], [], [], 0)[0]:
                    raise ValueError('paused core opened receipt intake or contacted a backend')
                status = json.loads(subprocess.check_output(
                    as_runtime(workers['wallet'], binaries['wallet'], 'status', wallet_config),
                    stdin=subprocess.DEVNULL, timeout=10, env=CLEAN_ENV))
                require_wallet_paused(wallet_log, wallet)
                require_writer_held(lock)
                require_signer_paused(signer_log, signer)
                require_writer_held(signer_lock)
                sockets = (wallet_socket, treasury_socket, receipt_socket, signer_socket)
                if any(os.path.lexists(path) for path in sockets):
                    raise ValueError('paused recovery exposed an effectful socket')
                funds = status['health']['totals']
                if (not status['registration_matches'] or not status['recovery_required']
                        or len(status['mint_namespaces']) != 2 or status['incomplete_mint_namespaces']
                        or funds['journal_custody_sats'] != 16 or funds['reported_treasury_deposit_sats'] != 396
                        or any(status[flag] is not True for flag in ENABLED_IN_WALLET_CONFIG)):
                    raise ValueError('paused wallet did not preserve its enabled configuration and funded custody')
            if (before != financial_state(dirs)
                    or any(hashlib.sha256(path.read_bytes()).hexdigest() != expected
                           for path, expected in wallet_bytes.items())
                    or signer_journal.read_bytes() != original_signer
                    or helper_invocations.read_bytes() != b'attempt\n'
                    or any(os.path.lexists(path) for path in sockets)
                    or select.select([outbound_probe], [], [], 0)[0]):
                raise ValueError('paused restart changed retained financial history or mint state')
    return {'native_start_stop_cycles': 2, 'paused_core_sats': 16, 'paused_wallet_sats': 16,
            'original_deposited_sats': 396, 'original_confirmed_feeds': 1,
            'disabled_workers': ['membership', 'payout', 'signer'], 'financial_history_unchanged': True,
            'core_started_without_credentials': True, 'core_effectful_routes_absent': True,
            'core_backend_connections_absent': True, 'core_fence_honored': True,
            'core_receipt_socket_absent': True,
            'wallet_fence_honored': True, 'wallet_writer_held': True, 'wallet_intake_sockets_absent': True,
            'wallet_wrong_or_missing_artifacts_refused': True, 'wallet_durable_bytes_unchanged': True,
            'signer_requests_retained': 1, 'signer_credential_verified': True, 'signer_writer_held': True,
            'signer_fence_honored': True, 'signer_refused_missing_or_wrong_binding': True,
            'signer_socket_absent': True, 'signer_helper_never_ran_during_restart': True,
            'signer_loaded_archived_systemd_credential': encrypted_signer is not None,
            'paused_markers_honored': True, 'unprivileged_paused_marker_refused': True,
            'installed_fences_cleared': False, 'resume_authorized': False}
