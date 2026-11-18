"""Rehearse startup and restart from a disposable copy of the advanced checkpoint."""
from contextlib import closing, contextmanager
import http.client
import json
import os
from pathlib import Path
import select
import shutil
import signal
import socket
import sqlite3
import subprocess
import time
import tomllib


CORE_TABLES = (
    'ledger_entries', 'feed_attempts', 'strike_receive_requests', 'cashu_receipt_inbox',
    'cashu_treasury_issuances', 'cashu_treasury_deposits', 'cashu_treasury_completions', 'message_outbox',
)
WALLET_TABLES = ('acquisitions', 'treasury_instructions', 'treasury_results', 'treasury_cdk_cleanup')
PAUSED = (
    'acquisition_enabled', 'reconciliation_enabled', 'receipt_delivery_enabled',
    'treasury_intake_enabled', 'treasury_admission_enabled', 'treasury_execution_enabled',
    'treasury_split_execution_enabled', 'treasury_reconciliation_enabled', 'relay_intake_enabled',
    'publication_signing_enabled', 'publication_delivery_enabled',
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


def unix_listener(path):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(1)
        client.connect(str(path))
    return True


@contextmanager
def runtime(command, ready, log, stop, environment=None):
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
                    time.sleep(0.05)
            else:
                output.seek(0)
                raise ValueError(f'{log.stem} synthetic restart failed: {output.read()[-2200:]}')
            yield
            process.send_signal(stop)
            if process.wait(timeout=10):
                output.seek(0)
                raise ValueError(f'{log.stem} synthetic shutdown failed: {output.read()[-2200:]}')
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)


def rehearse(root, workers, binaries, configurations, core_daemon):
    # This helper runs inside the verified loopback-only native backup fixture.
    # Only clone files from the exact installed later image. Leave its fences intact.
    root.mkdir(mode=0o711)
    roles = ('core', 'wallet', 'cashu_signer')
    dirs = {role: private_directory(root / role, workers[role]) for role in roles}
    for role, state in dirs.items():
        for source in Path(workers[role]['state']).iterdir():
            if source.name in ('CYBERHERD_RECOVERY_REQUIRED', 'CASHU_RECOVERY_REQUIRED'):
                continue
            target = state / source.name
            shutil.copyfile(source, target)
            target.chmod(0o600)
            os.chown(target, workers[role]['uid'], workers[role]['gid'])
    before = financial_state(dirs)
    if (before[0]['deposited_sats'] != [(396,)] or before[0]['confirmed_feeds'] != [(1,)]
            or len(before[0]['cashu_receipt_inbox']) != 3 or len(before[2]) != 2):
        raise ValueError('restart fixture lacks the funded later-stage history')

    fenced = subprocess.run(
        as_runtime(workers['core'], core_daemon, '--recovery-paused', '--config', configurations['core']),
        stdin=subprocess.DEVNULL, capture_output=True, timeout=10, env=CLEAN_ENV)
    if fenced.returncode != 1 or b'requires explicit external reconciliation' not in fenced.stderr:
        raise ValueError('paused recovery started despite the installed core fence')

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
            values['strike']['api_url']: f'https://127.0.0.1:{outbound_port}/',
            values['gateway']['url']: f'http://127.0.0.1:{outbound_port}/',
        }
        if values['service']['mode'] != 'canary' or set(values) & {'core_bridge', 'cashu', 'cashu_receipts', 'cashu_treasury'}:
            raise ValueError('unexpected core fixture configuration')
        for old, new in replacements.items():
            if original.count(old) != 1:
                raise ValueError('ambiguous synthetic core configuration')
            original = original.replace(old, new)
        wallet_socket = private_directory(root / 'w', workers['wallet'], 0o710) / 'intake.sock'
        wallet = json.loads(configurations['wallet'].read_bytes())
        wallet.update(state_directory=str(dirs['wallet']), intake_socket=str(wallet_socket))
        wallet_config = config_file(root / 'wallet.json', json.dumps(wallet))

        receipt_socket = private_directory(root / 'c', workers['core'], 0o710) / 'receipts.sock'
        [(core_epoch,)] = rows(dirs['core'] / 'ledger.sqlite', {
            'epoch': 'SELECT stream_id FROM overlay_identity WHERE singleton=1'})['epoch']
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
        signer_env = credentials(root / 'signer-creds', workers['cashu_signer'], {'cashu-nostr-client': '0' * 63 + '2'})

        for role, name in [('membership', 'membership'), ('payout', 'payout'), ('signer', 'payout_signer')]:
            command = as_runtime(workers[role], binaries[name], '--config', configurations[name])
            refused = subprocess.run(command, capture_output=True, timeout=10, env=CLEAN_ENV)
            if refused.returncode != 1 or b'disabled' not in refused.stderr:
                raise ValueError(f'{role} did not honor the disabled configuration')

        for stop in (signal.SIGINT, signal.SIGTERM):
            with (runtime(as_runtime(workers['core'], core_daemon, '--recovery-paused', '--config', core_config),
                          lambda: http_json(port, '/healthz') == {'status': 'ok'}, root / 'core.log',
                          stop),
                  runtime(as_runtime(workers['cashu_signer'], binaries['signer'], 'run', signer_config),
                          lambda: unix_listener(signer_socket), root / 'signer.log', signal.SIGTERM, signer_env),
                  runtime(as_runtime(workers['wallet'], binaries['wallet'], 'run', wallet_config),
                          lambda: unix_listener(wallet_socket), root / 'wallet.log', signal.SIGTERM)):
                core = http_json(port, '/api/v1/status')
                if core != {'mode': 'recovery-paused', 'feed_credit_sats': 16, 'unresolved_feed_attempt': None}:
                    raise ValueError('restored core did not preserve its paused state and available credit')
                for method, path in PAYMENT_AND_PRESENTATION_ROUTES:
                    if http_request(port, path, method)[0] != 404:
                        raise ValueError(f'paused recovery still serves {method} {path}')
                if os.path.lexists(receipt_socket) or select.select([outbound_probe], [], [], 0)[0]:
                    raise ValueError('paused core opened receipt intake or contacted a backend')
                status = json.loads(subprocess.check_output(
                    as_runtime(workers['wallet'], binaries['wallet'], 'status', wallet_config),
                    stdin=subprocess.DEVNULL, timeout=10, env=CLEAN_ENV))
                funds = status['health']['totals']
                if (not status['registration_matches'] or status['recovery_required']
                        or len(status['mint_namespaces']) != 2 or status['incomplete_mint_namespaces']
                        or funds['journal_custody_sats'] != 16 or funds['reported_treasury_deposit_sats'] != 396
                        or any(status[flag] is not False for flag in PAUSED)):
                    raise ValueError('restored wallet did not preserve its paused policy and funded custody')
            if (before != financial_state(dirs) or os.path.lexists(receipt_socket)
                    or select.select([outbound_probe], [], [], 0)[0]):
                raise ValueError('paused restart changed retained financial history or mint state')
    return {'native_start_stop_cycles': 2, 'paused_core_sats': 16, 'paused_wallet_sats': 16,
            'original_deposited_sats': 396, 'original_confirmed_feeds': 1,
            'disabled_workers': ['membership', 'payout', 'signer'], 'financial_history_unchanged': True,
            'core_started_without_credentials': True, 'core_effectful_routes_absent': True,
            'core_backend_connections_absent': True, 'core_fence_honored': True,
            'core_receipt_socket_absent': True,
            'installed_fences_cleared': False, 'resume_authorized': False}
