"""Start verified Cashu release binaries in disposable systemd sandboxes."""
from contextlib import ExitStack, closing
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import runpy
import shutil
import socket
import sqlite3
import stat
import tarfile
import tempfile
import tomllib
import time
import uuid


SCRIPTS = Path(__file__).resolve().parent
WALLET = 'lightning-goats-cashud'
SIGNER = 'lightning-goats-cashu-signerd'
RECEIPT_DROP_IN = 'deploy/systemd/lightning-goats-receive.service.d/80-cashu-receipts.conf.example'
RECEIPT_DIRECTORY = 'deploy/tmpfiles.d/lightning-goats-cashu-receipts.conf.example'
RECEIPT_PATH = '/run/lightning-goats-cashu-receipts'
FENCE = 'CASHU_RECOVERY_REQUIRED'
ROTATED_RECEIVER = '02e493dbf1c10d80f3581e4904930b1404cc6c13900ee0758474fa94abe8c4cd13'
PROBE = '''
import hashlib
import json
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
if role == 'signer':
    helper = json.loads(Path(config).read_text())['nak']['private_directory']
    def local_state():
        return {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                for folder in (state, helper) for path in Path(folder).iterdir() if path.is_file()}
    original = local_state()
    result = subprocess.run([binary, 'check-credential', config], capture_output=True, text=True, check=True)
    assert result.stdout.strip() == 'Signer local credential and helper binding verified.'
    without_credential = {key: value for key, value in os.environ.items() if key != 'CREDENTIALS_DIRECTORY'}
    for candidate, environment in (
        (config, without_credential),
        (Path(config).with_name('wrong-client.json'), os.environ),
        (Path(config).with_name('wrong-helper.json'), os.environ),
    ):
        subprocess.run([binary, 'check-config', candidate], env=environment, check=True)
        refusal = subprocess.run([binary, 'check-credential', candidate], env=environment,
                                 capture_output=True, text=True)
        assert refusal.returncode == 1 and not refusal.stdout
        assert refusal.stderr.strip() == 'signer local credential or helper binding rejected'
    assert local_state() == original
print('sandbox probe passed')
'''

RECEIPT_PROBE = r'''
import json, socket, struct, sys
from pathlib import Path

path, request, core_uid, expectation = sys.argv[1:]
with socket.socket(socket.AF_UNIX) as stream:
    stream.settimeout(5)
    try:
        stream.connect(path)
    except PermissionError:
        assert expectation == 'filesystem-denied'
        raise SystemExit(0)
    assert expectation != 'filesystem-denied'
    credentials = stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize('3i'))
    assert struct.unpack('3i', credentials)[1] == int(core_uid)
    body = Path(request).read_bytes()
    try:
        stream.sendall(struct.pack('!I', len(body)) + body)
        with stream.makefile('rb') as response:
            size = response.read(4)
            if expectation == 'peer-denied':
                assert size == b''
            else:
                assert len(size) == 4
                size = struct.unpack('!I', size)[0]
                assert 0 < size <= 4096
                ack = json.loads(response.read(size))
                assert ack['result']['acquisition_id'] == json.loads(body)['acquisition_id']
                assert ack['result']['net_sats'] == json.loads(body)['net_sats']
                print(json.dumps(ack, sort_keys=True))
    except (BrokenPipeError, ConnectionResetError):
        assert expectation == 'peer-denied'
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


def rehearse_receipt_socket(archive, root, cleanup, systemd, accounts, uids,
                            core_state, core_config, wallet_config):
    command = systemd['command']
    runtime = Path('/run') / (root.name + '-receipts')
    if os.path.lexists(runtime):
        raise ValueError('unexpected preexisting receipt directory')
    cleanup.callback(systemd['remove_directory'], runtime)
    groups = {role: grp.getgrgid(pwd.getpwnam(user).pw_gid) for role, user in accounts.items()}
    core_user = accounts['core']
    wallet_group, wallet_gid = groups['wallet'].gr_name, groups['wallet'].gr_gid
    with tarfile.open(archive, 'r:gz') as package:
        tmpfiles = root / 'receipt-tmpfiles.conf'
        tmpfiles.write_text(package.extractfile(RECEIPT_DIRECTORY).read().decode()
                            .replace(RECEIPT_PATH, str(runtime))
                            .replace('lightning-goats lg-cashu-ipc', f'{core_user} {wallet_group}'))
        fragment = root / 'receipt-drop-in.conf'
        fragment.write_bytes(package.extractfile(RECEIPT_DROP_IN).read())
        core = root / 'lightning-goatsd'
        with core.open('xb') as output, package.extractfile('lightning-goatsd') as binary:
            shutil.copyfileobj(binary, output)
        core.chmod(0o555)
    command(['systemd-tmpfiles', '--create', f'--prefix={runtime}', str(tmpfiles)])
    metadata = runtime.stat()
    if (metadata.st_uid, metadata.st_gid, stat.S_IMODE(metadata.st_mode)) != (uids['core'], wallet_gid, 0o750):
        raise ValueError('packaged core receipt directory has unexpected ownership or access')
    properties = [(key, value.replace(RECEIPT_PATH, str(runtime)).replace('lg-cashu-ipc', wallet_group))
                  for key, value in systemd['service_properties'](fragment)]
    if ('SupplementaryGroups', wallet_group) not in properties or ('ReadWritePaths', str(runtime)) not in properties:
        raise ValueError('packaged core receipt permissions are incomplete')

    state = root / 'core-receipt-state'
    state.mkdir(mode=0o700)
    ledger = state / 'ledger.sqlite'
    original = (core_state / 'ledger.sqlite').as_uri() + '?mode=ro'
    with closing(sqlite3.connect(original, uri=True)) as source, closing(sqlite3.connect(ledger)) as connection:
        source.backup(connection)
        [(epoch,)] = connection.execute('SELECT stream_id FROM overlay_identity').fetchall()
        if connection.execute('SELECT COUNT(*) FROM ledger_entries').fetchone() != (0,):
            raise ValueError('receipt fixture requires an empty ledger')
        connection.execute('INSERT OR IGNORE INTO reward_funding_state(singleton,bootstrap_through,last_ledger_id)'
                           ' VALUES(1,0,0)')
        connection.commit()
    os.chown(ledger, uids['core'], groups['core'].gr_gid)
    ledger.chmod(0o600)
    os.chown(state, uids['core'], groups['core'].gr_gid)

    endpoint = runtime / 'receipt.sock'
    wallet = json.loads(wallet_config.read_bytes())
    identity = {name: wallet[name] for name in ('wallet_instance', 'wallet_epoch')}
    candidate = root / 'core-with-receipts.toml'
    candidate.write_text(core_config.read_text()
                         .replace('sqlite://' + str(core_state / 'ledger.sqlite'), 'sqlite://' + str(ledger))
                         .replace('listen = "127.0.0.1:8788"', 'listen = "127.0.0.1:0"')
                         .replace('mode = "canary"', 'mode = "shadow"')
                         .replace('https://api.dev.strike.me/', 'https://127.0.0.1:9/')
                         .replace('http://10.8.0.6:8790/', 'http://127.0.0.1:9/') + f'''
[cashu_receipts]
socket = "{endpoint}"
ipc_gid = {wallet_gid}
wallet_uid = {uids['wallet']}
wallet_instance = "{identity['wallet_instance']}"
wallet_epoch = "{identity['wallet_epoch']}"
core_epoch = "{epoch}"
import_enabled = true
receipts_per_24h = 10
credit_sats_per_24h = 20
maximum_custody_sats = 20
deadline_seconds = 5
connections_per_second = 8
[[cashu_receipts.approved]]
mint_id = "fixture"
policy_version = "v1"
maximum_gross_sats = 10
maximum_net_sats = 10
''')
    candidate.chmod(0o444)
    namespace = f'NetworkNamespacePath=/proc/{os.getpid()}/ns/net'
    unit = f'{root.name}-native-receipts.service'
    process = systemd['UnitProcess'](unit)
    cleanup.callback(command, ['systemctl', 'reset-failed', unit], check=False)
    cleanup.callback(process.terminate)
    log = root / 'core-receipts.log'
    log.touch(mode=0o600)
    command(['systemd-run', '--unit', unit,
             *[f'--property={key}={value}' for key, value in properties],
             *['--property=' + entry for entry in (
                 f'User={core_user}', f'Group={groups["core"].gr_name}', f'ReadWritePaths={state}',
                 namespace, 'IPAddressDeny=any', 'ProtectSystem=strict',
                 'NoNewPrivileges=yes', 'CapabilityBoundingSet=',
                 'SetCredential=strike-api-key:synthetic-credentials-only',
                 'SetCredential=strike-webhook-secret:synthetic-credentials-only',
                 f'StandardOutput=append:{log}', f'StandardError=append:{log}')],
             '--', str(core), '--config', str(candidate)])
    ready(command, process, endpoint, log)
    metadata = endpoint.stat()
    if (metadata.st_uid, metadata.st_gid, stat.S_IMODE(metadata.st_mode)) != (uids['core'], wallet_gid, 0o660):
        raise ValueError('native core receipt socket mode or isolation group differs')

    probe = root / 'receipt-probe.py'
    probe.write_text(RECEIPT_PROBE)
    receipt = root / 'receipt.json'
    receipt.write_text(json.dumps(dict(identity, schema_version=1, acquisition_id=str(uuid.uuid4()),
                                      transport='cashu', source_ref='synthetic-systemd-receipt',
                                      source_author=None, mint_id='fixture', unit='sat', gross_sats=8,
                                      receive_fee_sats=1, net_sats=7, acquisition_evidence_digest='12' * 32,
                                      policy_version='v1', credit_pool='herd')))
    for path in (probe, receipt):
        path.chmod(0o444)

    def send(role, expectation, extra=()):
        account = accounts[role]
        result = command(['systemd-run', '--quiet', '--pipe', '--wait', '--collect',
                          f'--property=User={account}', f'--property=Group={groups[role].gr_name}',
                          '--property=IPAddressDeny=any', '--property=NoNewPrivileges=yes',
                          '--property=ProtectSystem=strict', *extra, '--',
                          '/usr/bin/python3', '-I', '-S', '-B', str(probe), str(endpoint),
                          str(receipt), str(uids['core']), expectation], check=False)
        if result.returncode:
            raise ValueError(f'native receipt probe {expectation} failed:\n'
                             + result.stderr[-1500:] + '\n' + diagnostics(command, process, log))
        return result.stdout

    send('signer', 'filesystem-denied')
    send('signer', 'peer-denied', (f'--property=SupplementaryGroups={wallet_group}',))
    first = send('wallet', 'accept')
    if not first or send('wallet', 'accept') != first:
        raise ValueError('native core did not replay the same wallet acknowledgment')
    process.terminate()
    if process.wait(timeout=15):
        raise ValueError('native cashu receipt core did not stop cleanly')
    with closing(sqlite3.connect(ledger.as_uri() + '?mode=ro', uri=True)) as connection:
        events = connection.execute("SELECT payload_json FROM event_log WHERE event_type='payment_received'").fetchall()
        if (connection.execute('SELECT net_sats FROM cashu_receipt_inbox').fetchall() != [(7,)]
                or len(events) != 1 or json.loads(events[0][0])['feeding']['reason'] != 'mode_disabled'
                or connection.execute('SELECT COUNT(*) FROM feed_attempts').fetchone() != (0,)):
            raise ValueError('synthetic receipt failed one-credit and zero-feeds guarantees')
    return dict(acknowledgment_replayed=True, ledger_rows=1, payment_events=1, feed_attempts=0,
                captured_feeding_snapshot=True, filesystem_denial=True,
                kernel_peer_denial_with_group_access=True)


def rehearse_pilot_config(archive, root, command, binaries, uids, states, core_config,
                          wallet_config, signer_config):
    with tarfile.open(archive, 'r:gz') as package:
        renderer = root / 'render-cashu-pilot.py'
        renderer.write_bytes(package.extractfile('deploy/scripts/render-cashu-pilot.py').read())
    base = root / 'config-base.toml'
    missing = root / 'offline-check-must-not-create.sqlite'
    wallet = json.loads(wallet_config.read_bytes())
    signing = json.loads(signer_config.read_bytes())
    base.write_text(core_config.read_text()
                    .replace(str(states['core'] / 'ledger.sqlite'), str(missing))
                    .replace('0' * 64, wallet['recipient']))
    epoch, broker, broker_epoch, account = [str(uuid.uuid4()) for _ in range(4)]
    bindings = root / 'pilot-bindings.json'
    relay = tomllib.loads(base.read_text())['nostr']['relays'][0]
    bindings.write_text(json.dumps(dict(
        version=1, core_uid=uids['core'], wallet_uid=uids['wallet'], wallet_gid=uids['wallet'],
        signer_uid=uids['signer'], signer_gid=uids['signer'], core_epoch=epoch,
        wallet_instance=wallet['wallet_instance'], wallet_epoch=wallet['wallet_epoch'],
        broker_instance=broker, broker_epoch=broker_epoch, receiving_key=wallet['receiving_key'],
        client_pubkey=signing['nak']['client_pubkey'], helper_sha256=signing['nak']['binary_sha256'],
        strike_account_id=account, strike_credential_fingerprint='ab' * 32, daily_credit_sats=20000,
        minimum_deposit_sats=1, relay_since_unix=1_800_000_000,
        nutzap_relays=[relay],
        mint_keysets=[dict(id='00deadbeef123456', keys={'1': wallet['receiving_key']})],
    )))
    review = root / 'pilot'
    command(['/usr/bin/python3', '-I', '-S', '-B', str(renderer), str(base), str(bindings), str(review)])
    proof = json.loads((review / 'REVIEW.json').read_bytes())
    if proof['activation_authorized'] is not False:
        raise ValueError('offline configuration rendered a live authorization')
    review.chmod(0o755)
    for role, filename in [('core', 'core.toml'), ('wallet', 'wallet.json'), ('signer', 'signer.json')]:
        path = review / filename
        if hashlib.sha256(path.read_bytes()).hexdigest() != proof['files'][filename]:
            raise ValueError('rendered configuration differs from its review')
        path.chmod(0o444)
        invocation = ['/usr/bin/setpriv', f'--reuid={uids[role]}', f'--regid={uids[role]}',
                      '--clear-groups', '--no-new-privs', '--', str(binaries[role])]
        arguments = ['--config', str(path), 'check-config'] if role == 'core' else ['check-config', str(path)]
        command([*invocation, *arguments])
    examined_wallet = json.loads((review / 'wallet.json').read_bytes())
    examined_signer = json.loads((review / 'signer.json').read_bytes())
    delivery = examined_wallet['publication_delivery']
    if ([relay] != examined_wallet['relay_intake']['relays']
            or [relay] != examined_signer['policy']['relays']
            or [relay] != delivery['base_relays'] or [relay] != delivery['discovery_relays']
            or any(delivery[key] is not False for key in ('announcements', 'redemptions'))):
        raise ValueError('inactive Nutzap relay bindings do not agree across the native roles')
    malformed = review / 'malformed-core.toml'
    malformed.write_text((review / 'core.toml').read_text()
                         .replace('import_enabled = false', 'import_enabled = "off"'))
    malformed.chmod(0o444)
    rejected = command(['/usr/bin/setpriv', f'--reuid={uids["core"]}', f'--regid={uids["core"]}',
                        '--clear-groups', '--no-new-privs', '--', str(binaries['core']),
                        '--config', str(malformed), 'check-config'], check=False)
    if not rejected.returncode or 'core configuration rejected' not in rejected.stderr or missing.exists():
        raise ValueError('offline core validator accepted invalid policy or opened the database')
    wallet_command = ['/usr/bin/setpriv', f'--reuid={uids["wallet"]}', f'--regid={uids["wallet"]}',
                      '--clear-groups', '--no-new-privs', '--', str(binaries['wallet'])]
    endpoint = 'https://goats.example/api/v1/cashu'
    for encoding in ('creqA', 'creqB'):
        preview = root / f'{encoding}.json'
        unregistered = command([*wallet_command, 'prepare-request', str(review / 'wallet.json'),
                                endpoint, encoding]).stdout
        preview.write_text(unregistered)
        selection = root / f'pilot-{encoding}'
        command(['/usr/bin/python3', '-I', '-S', '-B', str(renderer), str(base), str(bindings),
                 str(selection), '--reviewed-request', endpoint, str(preview)])
        proof = json.loads((selection / 'REVIEW.json').read_bytes())
        if (proof['activation_authorized'] is not False
                or proof['request_preview_sha256'] != hashlib.sha256(preview.read_bytes()).hexdigest()):
            raise ValueError('request review did not retain the inactive preview binding')
        selection.chmod(0o755)
        selected_wallet = selection / 'wallet.json'
        selected_wallet.chmod(0o444)
        config = json.loads(selected_wallet.read_bytes())
        if config['receive_cashu'] or config['receive_nutzap']:
            raise ValueError('reviewed request enabled a receiving path')
        command([*wallet_command, 'check-config', str(selected_wallet)])
        registered = json.loads(command([*wallet_command, 'prepare-request', str(selected_wallet),
                                         endpoint, encoding]).stdout)
        expected = json.loads(unregistered)
        expected['descriptor_registered'] = True
        if registered != expected:
            raise ValueError('wallet did not reproduce the original request')
        changed_endpoint = selection / 'wallet-changed-endpoint.json'
        config['public_request']['endpoint'] = endpoint + '/wrong'
        changed_endpoint.write_text(json.dumps(config))
        changed_endpoint.chmod(0o444)
        if command([*wallet_command, 'check-config', str(changed_endpoint)], check=False).returncode == 0:
            raise ValueError('wallet accepted descriptor from another endpoint')
    return dict(checked_roles=['core', 'wallet', 'signer'], absent_core_database_preserved=True,
                malformed_core_config_refused=True, activation_authorized=False,
                reviewed_requests_reproduced=['creqA', 'creqB'], nutzap_relay_scope_consistent=True)


def rehearse(selection, binaries, hashes, uids, states, wallet_config, signer_config,
             helper, helper_state, credential, request_announcement, core_config):
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
        core_account = f'{root.name}-c'
        command(['/usr/sbin/groupadd', '--gid', str(uids['core']), core_account])
        cleanup.callback(command, ['/usr/sbin/groupdel', core_account], check=False)
        command(['/usr/sbin/useradd', '--uid', str(uids['core']), '--gid', core_account, '--no-user-group',
                 '--no-create-home', '--home-dir', '/nonexistent', '--shell', '/usr/sbin/nologin', core_account])
        cleanup.callback(command, ['/usr/sbin/userdel', core_account], check=False)
        accounts['core'] = core_account
        receipts = rehearse_receipt_socket(archive, root, cleanup, systemd, accounts, uids,
                                          states['core'], core_config, wallet_config)
        pilot = rehearse_pilot_config(archive, root, command, binaries, uids, states, core_config,
                                      wallet_config, signer_config)
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
        for name, field, wrong in (
            ('wrong-client.json', 'client_pubkey', '2f8bde4d1a07209355b4a7250a5c5128e88b84bddc619ab7cba8d569b240efe4'),
            ('wrong-helper.json', 'binary_sha256', '0' * 64),
        ):
            original = settings['signer']
            candidate = root / name
            candidate.write_text(json.dumps({**original, 'nak': {**original['nak'], field: wrong}}))
            candidate.chmod(0o444)

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
                offline_signer_credential_check={'correct_mount_accepted': True,
                                                 'missing_and_wrong_client_refused': True,
                                                 'wrong_helper_digest_refused': True,
                                                 'journal_and_helper_state_preserved': True},
                repeated_start_and_stop=True, distinct_runtime_uids=True,
                loopback_ip_denied_by_systemd=True, installed_fences_preserved=True,
                core_receipt_socket=receipts,
                inactive_pilot_configuration=pilot,
                receiving_key_rotation={'wallet_anchor_and_namespace_unchanged': True,
                                        'missing_and_wrong_key_refused': True,
                                        'signer_original_request_unchanged': True,
                                        'stale_announcement_refused': True,
                                        'new_announcement_reached_inert_helper': True},
                scope='synthetic core credit and receiving-key rotation with inert signer; no external effects')
