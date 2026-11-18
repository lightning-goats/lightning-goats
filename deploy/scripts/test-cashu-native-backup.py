#!/usr/bin/env python3
"""Qualify native encrypted recovery inventories in disposable, offline state.

Use initialized state or the exact core and wallet snapshots of the funded
two-mint test. This test never clears a recovery fence.
"""
import argparse
from contextlib import closing, contextmanager, ExitStack
import fcntl
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import runpy
import shutil
import signal
import sqlite3
import subprocess
import tempfile
import time
import uuid


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def copy_funded_inputs(source, core, wallet, core_uid, wallet_uid, phase='before'):
    if source.resolve(strict=True) != source or not source.name.startswith('lg-cashu-combined-'):
        raise ValueError('select a disposable synthetic settlement fixture')
    snapshot = source / 'wallet-private' / ('mint-custody-' + phase)
    ledger = source / 'core-private' / ('mint-custody-core-' + phase + '.db')

    def copy(original, target, uid):
        if original.resolve(strict=True) != original or not original.is_file():
            raise ValueError('funded fixture requires regular canonical snapshots')
        shutil.copyfile(original, target)
        target.chmod(0o600)
        os.chown(target, uid, uid)
        if digest(original) != digest(target):
            raise ValueError('funded image changed during preparation')

    copy(ledger, core, core_uid)
    copy(snapshot / 'acquisition.sqlite', wallet / 'acquisition.sqlite', wallet_uid)
    copy(snapshot / 'wallet.sqlite', wallet / 'wallet.sqlite', wallet_uid)
    receiver = json.loads((source / 'wallet-private/mint-custody-before/receiving-key.json').read_bytes())
    if not re.fullmatch(r'0[23][a-f0-9]{64}', receiver):
        raise ValueError('invalid funded receiving fixture identity')
    with closing(sqlite3.connect((wallet / 'acquisition.sqlite').as_uri() + '?mode=ro&immutable=1', uri=True)) as db:
        registration = db.execute('SELECT instance FROM runtime_registration WHERE singleton=1').fetchall()
        epochs = db.execute('SELECT epoch FROM wallet_epoch WHERE singleton=1').fetchall()
        namespaces = dict(db.execute('SELECT mint_id,mint_url FROM cdk_mint_namespaces WHERE ready=1'))
    if (len(registration) != 1 or len(epochs) != 1
            or namespaces != {mint: f'https://{mint}.example' for mint in ('fixture-a', 'fixture-b')}):
        raise ValueError('funded source registration or issuer fixture differs')
    for mint in namespaces:
        namespace = hashlib.sha256(b'lightning-goats/cdk-mint-store/v1\0' + mint.encode()).hexdigest()
        name = 'mint-' + namespace + '.sqlite'
        copy(snapshot / name, wallet / name, wallet_uid)
    # This source is the fixed public synthetic seed/key used by the Rust
    # fixture. The native collector rederives and verifies their binding.
    lock = wallet / 'wallet-writer.lock'
    lock.touch(mode=0o600, exist_ok=False)
    os.chown(lock, wallet_uid, wallet_uid)
    return registration[0][0], epochs[0][0], receiver, namespaces


@contextmanager
def funded_reference_inputs(source, root, states, uids, expected):
    with tempfile.TemporaryDirectory(prefix='reference-runtime-', dir=root) as temporary, ExitStack() as stack:
        parked = Path(temporary)
        for role in ('core', 'wallet'):
            active = states[role]
            original = parked / ('before-' + role)
            active.rename(original)
            stack.callback(restore_fixture_state, active, original, parked / ('after-' + role))
            active.mkdir(mode=0o700)
            os.chown(active, uids[role], uids[role])
        identity = copy_funded_inputs(source, states['core'] / 'ledger.sqlite', states['wallet'],
                                     uids['core'], uids['wallet'], phase='after')
        if identity != expected:
            raise ValueError('funded reference changed the wallet registration or mint scope')
        for name in ('wallet-seed.bin', 'receiving-key.bin'):
            target = states['wallet'] / name
            shutil.copyfile(parked / 'before-wallet' / name, target)
            target.chmod(0o600)
            os.chown(target, uids['wallet'], uids['wallet'])
        yield


def restore_fixture_state(active, original, after):
    if active.exists():
        active.rename(after)
    original.rename(active)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['core', 'wallet', 'signer']:
        parser.add_argument('--' + name + '-cli', type=Path, required=True)
        parser.add_argument('--' + name + '-sha256', required=True)
    extended_names = ['membership', 'payout', 'payout_signer', 'statectl']
    for name in extended_names:
        option = name.replace('_', '-')
        parser.add_argument('--' + option + '-cli', type=Path)
        parser.add_argument('--' + option + '-sha256')
    for name in ['collector_copy','age','age_keygen']:
        option=name.replace('_','-')
        parser.add_argument('--'+option+'-cli',type=Path)
        parser.add_argument('--'+option+'-sha256')
    parser.add_argument('--rotate-receiving-key', action='store_true')
    parser.add_argument('--funded-fixture', type=Path, help='restore the disposable two-mint settlement checkpoints')
    parser.add_argument('--systemd-masks', action='store_true',
                        help='exercise the full collector against real masks on a disposable Actions runner')
    parser.add_argument('--systemd-package', nargs=3, metavar=('ARCHIVE', 'SOURCE', 'SHA256'),
                        help='rehearse packaged services using the initialized synthetic fixture')
    args = parser.parse_args()
    selected = [getattr(args, name + suffix) is not None
                for name in extended_names for suffix in ['_cli', '_sha256']]
    if any(selected) and not all(selected):
        raise ValueError('combined native fixture requires all four additional selected executables')
    extended = all(selected)
    collector_options=[getattr(args,name+suffix) is not None
                       for name in ['collector_copy','age','age_keygen'] for suffix in ['_cli','_sha256']]
    if any(collector_options) and (not all(collector_options) or not extended):
        raise ValueError('collector fixture requires all native programs and encryption artifacts')
    if args.rotate_receiving_key and not all(collector_options):
        raise ValueError('key rotation requires the complete archive rehearsal')
    if args.funded_fixture is not None and (not all(collector_options) or args.rotate_receiving_key):
        raise ValueError('funded recovery requires the archive rehearsal with its original key')
    if args.systemd_package and extended:
        raise ValueError('systemd rehearsal uses only the three package executables')
    if args.systemd_masks and (not all(collector_options)
            or os.environ.get('GITHUB_ACTIONS') != 'true'
            or not os.environ.get('GITHUB_RUN_ID', '').isdigit()
            or not Path('/run/systemd/system').is_dir()):
        raise ValueError('real mask testing requires the complete fixture on the disposable Actions runner')
    if (os.geteuid() != 0
            or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net')):
        raise ValueError('requires root in a fresh network namespace')
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link', 'show']))
    if [link['ifname'] for link in links] != ['lo']:
        raise ValueError('fixture requires loopback-only namespace')
    used = {p.pw_uid for p in pwd.getpwall()} | {g.gr_gid for g in grp.getgrall()}
    used |= {int(v) for v in subprocess.check_output(['ps', '-e', '-o', 'uid='], text=True).split()}
    available = [v for v in range(61000, 65000) if v not in used]
    core_uid, wallet_uid, signer_uid = available[:3]
    runtime_uids = dict(core=core_uid, wallet=wallet_uid, signer=signer_uid)
    if extended:
        runtime_uids.update(zip(['membership', 'payout', 'payout_signer'], available[3:6]))
    source = Path(__file__).resolve().parents[2]
    verify = runpy.run_path(str(source / 'deploy/scripts/verify-cashu-backup.py'))['verify']
    # The host manager needs its original /run. Stage this fixture's programs
    # on the executable filesystem used by the other systemd rehearsals.
    fixture_parent = '/var/lib' if args.systemd_package or args.systemd_masks else '/run'
    with (tempfile.TemporaryDirectory(prefix='lg-native-backup-', dir=fixture_parent) as temporary,
          ExitStack() as cleanup):
        root = Path(temporary)
        root.chmod(0o711)
        binaries, hashes, states = {}, {}, {}
        for name, uid in runtime_uids.items():
            original = getattr(args, name + '_cli').resolve(strict=True)
            data = original.read_bytes()
            expected = getattr(args, name + '_sha256')
            if not 0 < len(data) <= 512 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != expected:
                raise ValueError('selected native CLI hash or size mismatch')
            binary = root / (name + '-cli')
            binary.write_bytes(data)
            binary.chmod(0o555)
            binaries[name], hashes[name] = binary, expected
            state = root / (name + '-state')
            state.mkdir(mode=0o700)
            os.chown(state, uid, uid)
            states[name] = state

        def protected(name, value):
            path = root / name
            path.write_text(json.dumps(value))
            path.chmod(0o444)
            return path

        def command_for(name, uid, arguments):
            return ['/usr/bin/setpriv', f'--reuid={uid}', f'--regid={uid}',
                    '--clear-groups', '--no-new-privs', '--', str(binaries[name]),
                    *map(str, arguments)]

        def call(name, uid, *arguments, success=True, environment=None):
            command = command_for(name, uid, arguments)
            result = subprocess.run(command, env={'PATH': '/usr/bin:/bin', **(environment or {})},
                                    stdin=subprocess.DEVNULL, capture_output=True,
                                    text=True, timeout=30, umask=0o077)
            if (result.returncode == 0) != success:
                raise ValueError(f'{name} synthetic fixture exited {result.returncode}: {result.stderr[-1200:]}')
            return result

        ledger = states['core'] / 'ledger.sqlite'
        config = root / 'core.toml'
        template = (source / 'deploy/config.canary.toml.example').read_text()
        anchor = 'sqlite:///var/lib/lightning-goats/lightning-goats-canary.db'
        if template.count(anchor) != 1:
            raise ValueError('core fixture configuration anchor changed')
        config.write_text(template.replace(anchor, 'sqlite://' + str(ledger)))
        config.chmod(0o444)
        funded = args.funded_fixture is not None
        if funded:
            wallet_instance, wallet_epoch, receiver, namespaces = copy_funded_inputs(
                args.funded_fixture, ledger, states['wallet'], core_uid, wallet_uid)
        else:
            call('core', core_uid, '--config', config, 'status')
            wallet_instance, wallet_epoch = str(uuid.uuid4()), str(uuid.uuid4())
            receiver = '0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
            namespaces = {name: f'https://{name}.example' for name in ('first', 'second')}
        uri = ledger.as_uri() + ('?mode=ro&immutable=1' if funded else '')
        with closing(sqlite3.connect(uri, uri=True)) as connection:
            identity = connection.execute('SELECT stream_id FROM overlay_identity WHERE singleton=1').fetchone()
            if identity is None and not funded:
                identity = (str(uuid.uuid4()),)
                connection.execute('INSERT INTO overlay_identity VALUES(1,?)', identity)
                connection.commit()
        if identity is None:
            raise ValueError('funded core checkpoint lacks its ledger identity')
        core_epoch = identity[0]
        owner = 'f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9'
        client = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
        mints = [{'id': name, 'advertised_url': url,
                  'policy_version': 'fixture-v1' if funded else 'offline-v1', 'receive_cashu': False,
                  'receive_nutzap': False, 'bolt11_exit_qualified': True}
                 for name, url in namespaces.items()]
        wallet_config = protected('wallet.json', {
            'version': 1, 'runtime_uid': wallet_uid, 'core_uid': core_uid, 'ipc_gid': wallet_uid,
            'wallet_instance': wallet_instance, 'wallet_epoch': wallet_epoch,
            'state_directory': str(states['wallet']),
            'intake_socket': str(root / 'wallet-run/intake.sock'),
            'receiving_key': receiver, 'recipient': owner,
            'descriptors': [], 'keysets': [], 'mints': mints,
            'body_bytes': 65536, 'proof_count': 64, 'receipt_sats': 10000,
            'deadline_seconds': 5, 'connections_per_second': 8,
        })
        receiving_secret = bytes([1]) * 32 if funded else bytes(31) + bytes([1])
        for name, data in [('wallet-seed.bin', bytes([42]) * 64), ('receiving-key.bin', receiving_secret)]:
            path = states['wallet'] / name
            path.write_bytes(data)
            path.chmod(0o600)
            os.chown(path, wallet_uid, wallet_uid)
        if not funded:
            call('wallet', wallet_uid, 'initialize', wallet_config)
        status = json.loads(call('wallet', wallet_uid, 'status', wallet_config).stdout)
        if status.get('recovery_required'):
            raise ValueError('fresh wallet unexpectedly requires recovery')
        if args.rotate_receiving_key:
            original = receiver
            receiver = '02e493dbf1c10d80f3581e4904930b1404cc6c13900ee0758474fa94abe8c4cd13'
            rotated_key = states['wallet'] / ('receiving-key-' + receiver + '.bin')
            rotated_key.write_bytes(bytes(31) + bytes([4]))
            rotated_key.chmod(0o600)
            os.chown(rotated_key, wallet_uid, wallet_uid)
            settings = json.loads(wallet_config.read_bytes())
            settings.update(receiving_key_anchor=original, receiving_key=receiver,
                            retained_receiving_keys=[{'public_key': original}])
            wallet_config.write_text(json.dumps(settings))
        helper = root / 'unused-nak'
        helper.write_text('#!/bin/sh\nexit 97\n')
        helper.chmod(0o555)
        helper_state = root / 'unused-nak-state'
        helper_state.mkdir(mode=0o700)
        os.chown(helper_state, signer_uid, signer_uid)
        credentials = root / 'synthetic-credentials'
        credentials.mkdir(mode=0o700)
        os.chown(credentials, signer_uid, signer_uid)
        credential = credentials / 'cashu-nostr-client'
        credential.write_text('0' * 63 + '2')
        credential.chmod(0o400)
        os.chown(credential, signer_uid, signer_uid)
        signer_environment = {'CREDENTIALS_DIRECTORY': str(credentials)}
        signer_run = root / 'signer-run'
        signer_run.mkdir(mode=0o710)
        os.chown(signer_run, signer_uid, signer_uid)
        signer_config = protected('signer.json', {
            'version': 1, 'runtime_uid': signer_uid, 'ipc_gid': signer_uid,
            'state_directory': str(states['signer']), 'socket': str(root / 'signer-run/control.sock'),
            'policy': {'broker_instance': str(uuid.uuid4()), 'broker_epoch': str(uuid.uuid4()),
                       'wallet_instance': wallet_instance, 'wallet_epoch': wallet_epoch,
                       'wallet_uid': wallet_uid, 'owner': owner, 'receiving_key': receiver[2:],
                       'mints': [m['advertised_url'] for m in mints], 'relays': ['wss://relay.example']},
            'nak': {'binary': str(helper), 'binary_sha256': digest(helper),
                    'private_directory': str(root / 'unused-nak-state'),
                    'owner': owner, 'client_pubkey': client,
                    'relays': ['wss://relay.example'], 'deadline_seconds': 5},
        })
        call('signer', signer_uid, 'initialize', signer_config)
        # Positive control proves the same synthetic credential/configuration can
        # open the broker. No request reaches the inert helper or any relay.
        process = subprocess.Popen(command_for('signer', signer_uid, ['run', signer_config]),
                                   env={'PATH': '/usr/bin:/bin', **signer_environment},
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, umask=0o077)
        try:
            deadline = time.monotonic() + 10
            while not (signer_run / 'control.sock').exists():
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise ValueError('positive signer startup control did not become ready')
                time.sleep(0.025)
            process.send_signal(signal.SIGTERM)
            _, error = process.communicate(timeout=10)
            if process.returncode != 0 or error:
                raise ValueError('positive signer shutdown control failed')
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=10)
        (signer_run / 'control.sock').unlink(missing_ok=True)
        if args.systemd_package:
            rehearse = runpy.run_path(str(source / 'deploy/scripts/rehearse-cashu-systemd.py'))['rehearse']
            print(json.dumps(rehearse(args.systemd_package, binaries, hashes, runtime_uids, states,
                                      wallet_config, signer_config, helper, helper_state, credential),
                             sort_keys=True))
            return
        native_configurations, native_policy = {}, None
        if extended:
            native_fixture = runpy.run_path(str(source / 'deploy/scripts/cashu-native-membership-fixture.py'))
            native_configurations, native_policy = native_fixture['prepare'](
                source, root, states, runtime_uids, call, core_epoch, owner,
                '2f8bde4d1a07209355b4a7250a5c5128e88b84bddc619ab7cba8d569b240efe4')
            statectl = root / 'statectl'
            selected_statectl = args.statectl_cli.resolve(strict=True)
            combined_module = runpy.run_path(str(source / 'deploy/scripts/verify-cashu-combined-backup.py'))
            combined_module['freeze_executable'](selected_statectl, statectl, args.statectl_sha256)
            hashes['statectl'] = args.statectl_sha256
        # All initializing/control children exited; no writer remains active.
        # Retain both actual native writer locks while taking SQLite backups.
        if extended:
            recovery = root / 'recovery'
            recovery.mkdir(mode=0o700)
            snapshot = recovery / 'cashu'
        else:
            snapshot = root / 'snapshot'
        snapshot.mkdir(mode=0o700)
        components = []

        def component(role, original, **extra):
            target = snapshot / (str(len(components)) + '.component')
            if role in {'core_ledger', 'wallet_journal', 'cdk_wallet', 'signing_broker', 'cdk_mint_wallet'}:
                with closing(sqlite3.connect(original.as_uri() + '?mode=ro', uri=True)) as src, closing(sqlite3.connect(target)) as dst:
                    src.backup(dst)
            else:
                target.write_bytes(original.read_bytes())
            target.chmod(0o600)
            components.append(dict(role=role, path=target.name, sha256=digest(target), **extra))

        with ExitStack() as locks:
            for name in ['wallet', 'signer']:
                stream = locks.enter_context((states[name] / 'wallet-writer.lock').open('rb'))
                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            component('core_ledger', ledger)
            component('wallet_journal', states['wallet'] / 'acquisition.sqlite')
            component('cdk_wallet', states['wallet'] / 'wallet.sqlite')
            component('signing_broker', states['signer'] / 'cashu-signing.sqlite')
            for name, role in [('wallet-seed.bin', 'wallet_seed'), ('receiving-key.bin', 'receiving_key')]:
                component(role, states['wallet'] / name)
            for path in [config, wallet_config, signer_config, *native_configurations.values()]:
                component('protected_policy', path)
            for mint in mints:
                namespace = hashlib.sha256(b'lightning-goats/cdk-mint-store/v1\0' + mint['id'].encode()).hexdigest()
                component('cdk_mint_wallet', states['wallet'] / ('mint-' + namespace + '.sqlite'),
                          mint_id=mint['id'], mint_url=mint['advertised_url'])
        inventory = snapshot / 'inventory.json'
        inventory.write_text(json.dumps({'version': 1, 'checkpoint_id': str(uuid.uuid4()),
                                        'registrations': dict(core_epoch=core_epoch,
                                                              wallet_instance=wallet_instance,
                                                              wallet_epoch=wallet_epoch),
                                        'components': components}))
        inventory.chmod(0o600)
        result = verify(snapshot)
        if result['mint_stores_checked'] != 2 or result['restore_authorized']:
            raise ValueError('native snapshot did not verify both mint namespaces')
        combined_result = None
        if extended:
            combined_result = native_fixture['combine'](
                root, snapshot, native_policy, statectl, args.statectl_sha256,
                combined_module['verify'])
            combined_result['empty_initialized_state_only'] = not funded
        collector_result=None
        if all(collector_options):
            collector_fixture=runpy.run_path(str(source/'deploy/scripts/cashu-collector-fixture.py'))
            reference = None
            if funded:
                def reference():
                    return funded_reference_inputs(args.funded_fixture, root, states, runtime_uids,
                                                   (wallet_instance, wallet_epoch, receiver, namespaces))
            collector_result=collector_fixture['qualify'](source,root,states,runtime_uids,binaries,
                dict(core=config,wallet=wallet_config,signer=signer_config,**native_configurations),
                native_policy,args,cleanup,reference)
        # Exercise refusal against the actual initialized schemas, not hand-made
        # substitutes. Restore each disposable byte image before the next case.
        def refused():
            try:
                verify(snapshot)
            except (ValueError, OSError, sqlite3.Error, KeyError, TypeError):
                return
            raise ValueError('incomplete or rebound native snapshot was accepted')

        key_row = next(r for r in components if r['role'] == 'receiving_key')
        key_path = snapshot / key_row['path']
        retained_key = key_path.read_bytes()
        key_path.unlink()
        refused()
        key_path.write_bytes(retained_key)
        key_path.chmod(0o600)
        original_inventory = inventory.read_bytes()
        changed = json.loads(original_inventory)
        changed['registrations']['wallet_epoch'] = str(uuid.uuid4())
        inventory.write_text(json.dumps(changed))
        refused()
        inventory.write_bytes(original_inventory)
        namespace_row = next(r for r in components if r['role'] == 'cdk_mint_wallet')
        namespace_path = snapshot / namespace_row['path']
        retained_namespace = namespace_path.read_bytes()
        namespace_path.unlink()
        refused()
        namespace_path.write_bytes(retained_namespace)
        namespace_path.chmod(0o600)
        verify(snapshot)
        marker = states['wallet'] / 'CASHU_RECOVERY_REQUIRED'
        marker.write_bytes(b'fixture recovery requires reconciliation')
        marker.chmod(0o600)
        os.chown(marker, wallet_uid, wallet_uid)
        before = {p.name: digest(p) for p in states['wallet'].iterdir() if p.is_file()}
        call('wallet', wallet_uid, 'run', wallet_config, success=False)
        after = {p.name: digest(p) for p in states['wallet'].iterdir() if p.is_file()}
        if before != after:
            raise ValueError('fenced native startup changed retained state')
        core_marker = states['core'] / 'CYBERHERD_RECOVERY_REQUIRED'
        core_marker.write_bytes(b'fixture core restore requires reconciliation')
        core_marker.chmod(0o600)
        os.chown(core_marker, core_uid, core_uid)
        before_core = {p.name: digest(p) for p in states['core'].iterdir() if p.is_file()}
        call('core', core_uid, '--config', config, 'status', success=False)
        after_core = {p.name: digest(p) for p in states['core'].iterdir() if p.is_file()}
        if before_core != after_core:
            raise ValueError('fenced core open changed retained state')
        signer_marker = states['signer'] / 'CASHU_RECOVERY_REQUIRED'
        signer_marker.write_bytes(b'fixture signer restore requires reconciliation')
        signer_marker.chmod(0o600)
        os.chown(signer_marker, signer_uid, signer_uid)
        before_signer = {p.name: digest(p) for p in states['signer'].iterdir() if p.is_file()}
        call('signer', signer_uid, 'run', signer_config, success=False,
             environment=signer_environment)
        after_signer = {p.name: digest(p) for p in states['signer'].iterdir() if p.is_file()}
        if before_signer != after_signer:
            raise ValueError('fenced signer startup changed retained state')
        print(json.dumps({'result': ('CASHU_FUNDED_NATIVE_INSTALL_VERIFIED' if funded
                                     else 'CASHU_EMPTY_NATIVE_BACKUP_VERIFIED'),
                          'artifacts_sha256': hashes, 'inventory_result': result,
                          'combined_native_review': combined_result,
                          'coordinated_collector': collector_result,
                          'distinct_runtime_uids': True, 'wallet_recovery_marker_refused': True,
                          'fenced_wallet_bytes_unchanged': True,
                          'core_recovery_marker_refused': True,
                          'fenced_core_bytes_unchanged': True,
                          'positive_signer_startup_control': True,
                          'signer_recovery_marker_refused': True,
                          'fenced_signer_bytes_unchanged': True,
                          'missing_key_refused': True, 'wrong_epoch_refused': True,
                          'missing_mint_namespace_refused': True,
                          'empty_initialized_state_only': not funded, 'funded_restore_qualified': False,
                          'funded_fenced_installation_verified': funded,
                          'external_reconciliation_verified': False, 'production_changes': False},
                         sort_keys=True))


if __name__ == '__main__':
    main()
