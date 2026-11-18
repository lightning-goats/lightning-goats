"""Actual native/key/encryption composition; service statuses are explicitly mocked."""
from argparse import Namespace
from contextlib import ExitStack, closing, contextmanager, nullcontext
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import sqlite3
import subprocess
import tarfile
import time
import types
from unittest.mock import patch


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda:source.read(65536),b''): h.update(chunk)
    return h.hexdigest()


def prepare_systemd_masks(root, workers, lock, cleanup):
    units = [worker['unit'] for worker in workers.values()]
    if (root.parent != Path('/var/lib') or not root.name.startswith('lg-native-backup-')
            or any(not unit.startswith(root.name + '-') for unit in units)):
        raise ValueError('runtime masks must belong to this disposable fixture')

    def systemctl(*args, check=True):
        return subprocess.run(['/usr/bin/systemctl', *args], check=check, capture_output=True, timeout=30,
                              stdin=subprocess.DEVNULL, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})

    directory = Path('/run/systemd/system')
    masks = [directory / unit for unit in units]
    for unit, mask in zip(units, masks, strict=True):
        shown = systemctl('show', '--property=LoadState', '--value', '--', unit, check=False)
        if shown.stdout.strip() != b'not-found' or os.path.lexists(mask):
            raise ValueError('fixture cannot adopt an existing systemd unit')
    # This fixed lock path must be new on the disposable runner. Refuse to
    # reuse an installed maintenance directory, even if it appears unused.
    lock.parent.mkdir(mode=0o700)
    cleanup.callback(lock.parent.rmdir)
    cleanup.callback(lock.unlink, missing_ok=True)
    cleanup.callback(systemctl, 'daemon-reload')
    for path in masks:
        path.symlink_to('/dev/null')
        cleanup.callback(path.unlink)
    systemctl('daemon-reload')


@contextmanager
def active_process(worker):
    proc = subprocess.Popen(['/usr/bin/setpriv', f"--reuid={worker['uid']}", f"--regid={worker['gid']}",
                             '--clear-groups', '--no-new-privs', '--', '/usr/bin/sleep', '30'],
                            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        identity = Path(f'/proc/{proc.pid}/status')
        deadline = time.monotonic() + 5
        while not any(line.startswith('Uid:') and int(line.split()[1]) == worker['uid']
                      for line in identity.read_text().splitlines()):
            if proc.poll() is not None or time.monotonic() >= deadline:
                raise ValueError('disposable runtime probe did not assume its UID')
            time.sleep(0.01)
        yield
    finally:
        if proc.poll() is None:
            proc.terminate()
        proc.wait(timeout=5)


def verify_restored_runtime_fences(stage, root, workers, binaries, configurations, roles):
    """Open the installed role images as their distinct runtime identities."""
    runtime = root / 'restored-runtime'
    runtime.mkdir(mode=0o711)
    states = {}
    for role, name in roles.items():
        directory = Path(workers[role]['state'])
        states[role] = directory
        config = configurations[name].read_text()
        if workers[role]['state'] not in config:
            raise ValueError('fixture configuration does not identify its original state')
        if role in ('membership', 'payout', 'signer'):
            if not config.startswith('enabled = false\n') and '\nenabled = false\n' not in config:
                raise ValueError('fixture configuration is missing its disabled switch')
            config = config.replace('enabled = false\n', 'enabled = true\n', 1)
        config_path = runtime / (role + '.conf')
        config_path.write_text(config)
        config_path.chmod(0o444)

    names = list(roles)
    for index, (role, name) in enumerate(roles.items()):
        directory = states[role]
        before = {path.name: digest(path) for path in directory.iterdir()}
        archived = stage / 'fenced-images' / role
        if before != {path.name: digest(path) for path in archived.iterdir()}:
            raise ValueError('runtime image differs from exact fenced checkpoint')
        worker = workers[role]
        user = ['/usr/bin/setpriv', f"--reuid={worker['uid']}", f"--regid={worker['gid']}",
                '--clear-groups', '--no-new-privs', '--']

        def run(*args):
            return subprocess.run([*user, *map(str, args)], stdin=subprocess.DEVNULL,
                                  capture_output=True, timeout=15, umask=0o077,
                                  env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})

        own = next(path for path in directory.iterdir() if path.suffix in ('.db', '.sqlite'))
        other = states[names[index - 1]]
        if run('/usr/bin/test', '-r', own).returncode or run('/usr/bin/test', '-x', other).returncode != 1:
            raise ValueError('restored role permissions do not isolate the runtime images')
        config_path = runtime / (role + '.conf')
        message = b'restored journal requires explicit external reconciliation before startup'
        args = ['--config', config_path]
        if role == 'core':
            args.append('status')
        elif role in ('wallet', 'cashu_signer'):
            args = ['run', config_path]
            message = b'Cashu recovery requires explicit reconciliation'
        refused = run(binaries[name], *args)
        if refused.returncode != 1 or message not in refused.stderr:
            raise ValueError(f'{role} did not refuse runtime startup at its recovery fence: '
                             + refused.stderr.decode(errors='replace'))
        if role == 'wallet':
            seed = directory / 'wallet-seed.bin'
            seed.chmod(0)
            try:
                no_seed_access = run(binaries[name], *args)
                if no_seed_access.returncode != 1 or no_seed_access.stderr != refused.stderr:
                    raise ValueError('wallet did not reject its recovery fence before key access')
            finally:
                seed.chmod(0o600)
        if before != {path.name: digest(path) for path in directory.iterdir()}:
            raise ValueError(f'{role} runtime modified restored state after refusal')
    return sorted(roles)


def qualify_install(capsule, bootstrap, stage, output_root, policy_path, common,
                    workers, binaries, configurations, roles, systemd_masks):
    args = Namespace(stage=stage, stage_sha256=digest(stage / 'STAGED.json'),
                     policy=policy_path, policy_sha256=digest(policy_path), signer_credential_store=None)
    launcher = runpy.run_path(str(capsule / 'launch-cashu-backup.py'))
    tool = launcher['verify'](capsule, bootstrap)
    install = runpy.run_path(str(capsule / 'install-cashu-restore.py'))['install']

    def refused(reason, **context):
        try:
            install(args, {**common, **context}, tool)
        except ValueError as error:
            if reason not in str(error):
                raise
        else:
            raise ValueError('invalid installed checkpoint accepted')

    def invoke(checksum, *extra):
        return subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
                               bootstrap, 'install-fenced', str(stage), checksum, str(policy_path),
                               args.policy_sha256, *extra], stdin=subprocess.DEVNULL, capture_output=True, timeout=120)

    wrong = invoke('ff' * 32)
    if (wrong.returncode != 1 or wrong.stdout or wrong.stderr !=
            b'Cashu fenced installation refused. Preserve the checkpoint and any installed fences.\n'):
        raise ValueError('authenticated install launch did not reject a changed review digest')
    refused('existing target differs')
    wallet_config = configurations['wallet']
    approved_config = wallet_config.read_bytes()
    wallet_config.write_bytes(approved_config + b'\n')
    try:
        refused('installed config or executable differs')
    finally:
        wallet_config.write_bytes(approved_config)

    def active(*_):
        raise ValueError('fixture active writer refusal')

    retained = output_root / 'preexisting-runtime-untouched'
    retained.mkdir(mode=0o700)
    installed = output_root / 'installed-fenced-runtime'
    installed.mkdir(mode=0o700)
    published = []
    signer_clients = None
    try:
        for role, worker in workers.items():
            Path(worker['state']).rename(retained / role)
        if systemd_masks:
            with active_process(workers['wallet']):
                refused('runtime UID still has a process')
        else:
            refused('fixture active writer refusal', check_quiescence=active)
        if any(os.path.lexists(worker['state']) for worker in workers.values()):
            raise ValueError('active writer refusal published restored bytes')
        wallet = workers['wallet']
        placeholder = Path(wallet['state'])
        placeholder.mkdir(mode=0o700)
        os.chown(placeholder, wallet['uid'], wallet['gid'])

        def interrupt(role):
            published.append(role)
            raise InterruptedError('fixture interruption after role publication')

        try:
            install(args, common, tool, after_publish=interrupt)
        except InterruptedError:
            present = [role for role, worker in workers.items() if Path(worker['state']).exists()]
            if len(published) != 1 or set(present) != {*published, 'wallet'}:
                raise ValueError('interrupted restore published an unexpected role set')
        else:
            raise ValueError('fixture interruption was missed')
        if systemd_masks:
            resumed = invoke(args.stage_sha256)
            if resumed.returncode:
                raise ValueError('authenticated installer refused the real masked fixture: '
                                 + resumed.stderr.decode(errors='replace'))
            result = json.loads(resumed.stdout)
        else:
            result = install(args, common, tool)
        if (result['already_installed'] != published or result['roles'] != sorted(roles)
                or result['result'] != 'CASHU_RESTORE_INSTALLED_FENCED'
                or result['resume_authorized'] or result['services_changed']):
            raise ValueError('exact restore retry differed from expected scope')
        retries = install(args, common, tool)
        if retries['already_installed'] != sorted(roles):
            raise ValueError('completed installation did not recognize the original images')
        extra = Path(workers['core']['state']) / 'unexpected'
        extra.touch(mode=0o600)
        try:
            refused('existing target differs')
            if not extra.exists():
                raise ValueError('installer removed existing runtime evidence')
        finally:
            extra.unlink()
        if systemd_masks:
            signer_clients = qualify_signer_clients(install, invoke, args, common, tool, workers, output_root)
        verified = verify_restored_runtime_fences(stage, output_root.parent, workers, binaries, configurations, roles)
    finally:
        for role, worker in workers.items():
            if not (retained / role).exists():
                continue
            state = Path(worker['state'])
            if state.exists():
                state.rename(installed / role)
            (retained / role).rename(state)
    return {'exact_images_installed_fenced': verified, 'interrupted_install_recovered': True,
            'exact_retry_verified': True, 'changed_target_refused': True,
            'existing_live_image_refused': True, 'active_writer_refused': True,
            'existing_empty_target_supported': True, 'changed_installed_config_refused': True,
            'authenticated_launch_wrong_digest_refused': True,
            'authenticated_cli_retry_with_actual_masks': systemd_masks, 'signer_clients': signer_clients}


def signer_systemd_command(*command, check=True):
    return subprocess.run(command, check=check, stdin=subprocess.DEVNULL, capture_output=True,
                          timeout=30, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})


@contextmanager
def fixture_role_account(worker, fixture_root, role):
    if fixture_root.parent != Path('/var/lib') or not fixture_root.name.startswith('lg-native-backup-'):
        raise ValueError('runtime account requires a disposable systemd fixture')
    suffix = fixture_root.name.removeprefix('lg-native-backup-')
    account = f"lgc-{suffix}-{role.replace('_', '-')}"
    group = account + '-group'
    run = signer_systemd_command
    with ExitStack() as cleanup:
        run('/usr/sbin/groupadd', '--gid', str(worker['gid']), group)
        cleanup.callback(run, '/usr/sbin/groupdel', group)
        run('/usr/sbin/useradd', '--uid', str(worker['uid']), '--gid', group, '--no-user-group',
            '--no-create-home', '--home-dir', '/nonexistent', '--shell', '/usr/sbin/nologin', account)
        cleanup.callback(run, '/usr/sbin/userdel', account)
        yield account, group


def qualify_signer_clients(install, invoke, args, common, tool, workers, output_root):
    """Reprovision archived keys in a throwaway store and let PID 1 load them."""
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    subprocess.run(['/usr/bin/systemd-creds', 'setup'], check=True, timeout=15,
                   stdin=subprocess.DEVNULL, capture_output=True, env=env)
    store = output_root / 'disposable-credential-store'
    store.mkdir(mode=0o700)
    args = Namespace(**{**vars(args), 'signer_credential_store': store})
    keys = args.stage / 'checkpoint-evidence/recovery-material'
    credentials = [('cashu_signer', 'cashu-nostr-client', 'lg-cashu-nostr-client', keys / 'cashu-client.hex'),
                   ('signer', 'nostr-client-key', 'cyberherd-signer-client', keys / 'payout-client.hex')]
    flags = ('--signer-credential-store', str(store))
    cashu = store / credentials[0][2]
    payout = store / credentials[1][2]
    original_key = credentials[0][3]

    def refused():
        result = invoke(args.stage_sha256, *flags)
        if (result.returncode != 1 or result.stdout or result.stderr !=
                b'Cashu fenced installation refused. Preserve the checkpoint and any installed fences.\n'):
            raise ValueError('invalid signer restore was accepted or disclosed private context')

    saved = original_key.read_bytes()
    original_key.write_bytes(credentials[1][3].read_bytes())
    try:
        refused()
        if any(store.iterdir()):
            raise ValueError('changed portable signer client published a credential')
    finally:
        original_key.write_bytes(saved)

    sealed = subprocess.run(['/usr/bin/systemd-creds', '--with-key=null', '--name=cashu-nostr-client',
                             'encrypt', '-', '-'], input=saved, check=True, capture_output=True,
                            timeout=15, env=env).stdout
    common['write'](cashu, sealed)
    refused()
    if cashu.read_bytes() != sealed or payout.exists():
        raise ValueError('null-key refusal changed the credential store')
    cashu.unlink()

    def interrupt(role):
        if role != 'signer-credential:cashu_signer':
            raise ValueError('signer credential publication started in an unexpected phase')
        raise InterruptedError('fixture interruption after first signer credential')

    try:
        install(args, common, tool, after_publish=interrupt)
    except InterruptedError:
        if not cashu.is_file() or payout.exists():
            raise ValueError('interrupted restore published an unexpected credential set')
    else:
        raise ValueError('fixture missed the first signer credential publication')
    first = digest(cashu)
    restored = invoke(args.stage_sha256, *flags)
    if restored.returncode:
        raise ValueError('exact interrupted credential restore refused')
    result = json.loads(restored.stdout)
    if (result['signer_clients_verified'] != ['cashu_signer', 'signer'] or result['resume_authorized']
            or result['services_changed'] or digest(cashu) != first):
        raise ValueError('signer restore retry returned a different scope or rewrote the exact first credential')

    probe = """
import hashlib
import os
from pathlib import Path
import sys
directory = Path(os.environ['CREDENTIALS_DIRECTORY'])
name, expected, ciphertext, uid = sys.argv[1:]
assert os.getuid() == int(uid)
assert os.listdir(directory) == [name]
assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected
assert not os.access(ciphertext, os.R_OK)
"""
    for role, name, filename, original in credentials:
        worker = workers[role]
        ciphertext = store / filename
        unit = worker['unit'].removesuffix('.service') + '-credential-probe.service'
        with fixture_role_account(worker, output_root.parent, role) as (account, group):
            result = signer_systemd_command(
                         '/usr/bin/systemd-run', '--quiet', '--wait', '--collect', '--unit=' + unit,
                         '--property=User=' + account, '--property=Group=' + group,
                         '--property=LoadCredentialEncrypted=' + name + ':' + str(ciphertext),
                         '--property=PrivateNetwork=yes', '--property=NoNewPrivileges=yes',
                         '--property=StandardOutput=null', '--property=StandardError=null', '--',
                         '/usr/bin/python3', '-I', '-S', '-B', '-c', probe, name, digest(original),
                         str(ciphertext), str(worker['uid']), check=False)
            if result.returncode:
                raise ValueError(f'systemd could not load the exact isolated {role} credential')
    return {'archived_key_substitution_refused': True, 'non_host_envelope_refused': True,
            'credential_interruption_recovered': True, 'exact_existing_bytes_preserved': True,
            'systemd_loaded_separate_signer_clients': True, 'external_bunker_verified': False}


def verify_funded_stage(stage, worklist, source, common):
    def rows(database, query):
        with closing(sqlite3.connect(database.as_uri() + '?mode=ro&immutable=1', uri=True)) as db:
            return db.execute(query).fetchall()

    core = stage / 'fenced-images/core/ledger.sqlite'
    wallet = stage / 'fenced-images/wallet'
    issuance = 'SELECT intent_id,receive_request_id,payment_hash FROM cashu_treasury_issuances'
    original = rows(source / 'core-private/mint-custody-core-before.db', issuance)
    if (len(original) != 1 or rows(core, issuance) != original
            or rows(wallet / 'acquisition.sqlite', 'SELECT id,receive_id,payment_hash FROM treasury_instructions') != original
            or rows(wallet / 'acquisition.sqlite', 'SELECT state FROM treasury_instructions') != [('bound',)]
            or rows(core, 'SELECT COUNT(*) FROM cashu_receipt_inbox') != [(2,)]
            or rows(core, 'SELECT COUNT(*) FROM cashu_treasury_deposits') != [(0,)]
            or rows(core, 'SELECT COUNT(*) FROM feed_attempts') != [(0,)]):
        raise ValueError('funded stage lost the imported receipts or original unsettled invoice')
    amounts = {}
    store_name = runpy.run_path(str(Path(__file__).with_name('review-cashu-restore.py')))['mint_store_name']
    for mint, expected in (('fixture-a', 250), ('fixture-b', 150)):
        amount = rows(wallet / store_name(mint, common), "SELECT SUM(amount) FROM proof WHERE state='UNSPENT'")
        if amount != [(expected,)]:
            raise ValueError('funded stage lost original mint custody')
        amounts[mint] = expected
    expected_counts = {'strike_receives': 1, 'mint_melts': 1, 'mint_splits': 0, 'feeder_requests': 0,
                       'cashu_acquisitions': 0, 'financial_feed_plans': 1}
    for group, count in expected_counts.items():
        if worklist['records'][group]['total'] != count:
            raise ValueError('funded stage worklist lost its unresolved original')
    for group in ('strike_receives', 'mint_melts'):
        if worklist['records'][group]['items'][0]['staged']['id'] != original[0][0]:
            raise ValueError('funded stage worklist rebound the original intent')
    plan = worklist['records']['financial_feed_plans']['items'][0]['staged']
    if (plan['state'] != 'pending' or plan['request_id'] is not None
            or rows(core, 'SELECT id,threshold_sats FROM feed_financial_plans') != [(plan['id'], plan['threshold_sats'])]):
        raise ValueError('funded stage worklist lost the pre-dispatch financial plan')
    return {'original_invoice_retained': True, 'credited_cashu_receipts_retained': 2, 'staged_custody_sats': amounts,
            'original_pending_intent_in_authenticated_worklist': True,
            'pending_financial_plan_in_authenticated_worklist': True}


@contextmanager
def exercise_paused_gate(install, invoke, args, common, tool, workers, output, real_masks):
    approval = output / 'paused-approval.json'
    common['write_json'](approval, {'version': 1, 'purpose': 'cashu-paused-recovery',
                                   'previous_stage_sha256': args.advance_from[1],
                                   'stage_sha256': args.stage_sha256, 'policy_sha256': args.policy_sha256,
                                   'evidence_sha256': args.advance_evidence[1]})
    choice = (str(approval), digest(approval))
    prepare = Namespace(**{**vars(args), 'signer_credential_store': None,
                           'paused_gate': 'prepare', 'paused_approval': choice})
    restore = Namespace(**{**vars(prepare), 'paused_gate': 'restore'})
    markers = {role: Path(workers[role]['state']) / ('CYBERHERD_RECOVERY_REQUIRED' if role == 'core'
                                                    else 'CASHU_RECOVERY_REQUIRED')
               for role in ('core', 'wallet', 'cashu_signer')}
    originals = {role: marker.read_bytes() for role, marker in markers.items()}

    def verify(paused):
        for role, marker in markers.items():
            if role in paused:
                label = f'LG_CASHU_PAUSED_RECOVERY_V1-{role}-{choice[1]}'
                metadata = marker.lstat()
                if (not marker.is_symlink() or os.readlink(marker) != label
                        or (metadata.st_uid, metadata.st_gid, metadata.st_nlink) != (0, 0, 1)):
                    raise ValueError('paused gate differs from the approved root-owned marker')
            elif (marker.is_symlink() or marker.read_bytes() != originals[role]
                    or marker.stat().st_uid != workers[role]['uid']):
                raise ValueError('original hard marker was not restored exactly')

    def interrupt(role):
        if role != 'core':
            raise ValueError('first marker replacement changed an unexpected role')
        raise InterruptedError('fixture stopped after durable marker replacement')

    def interrupted(candidate, expected):
        try:
            install(candidate, common, tool, after_publish=interrupt)
        except InterruptedError:
            verify(expected)
        else:
            raise ValueError('paused gate did not interrupt after its first role')

    def refused(candidate, fragment):
        try:
            install(candidate, common, tool)
        except ValueError as error:
            if fragment not in str(error):
                raise
        else:
            raise ValueError('paused gate accepted an unmatched approval or live writer')

    def complete(candidate, expected):
        if real_masks:
            completed = invoke(candidate)
            if completed.returncode:
                raise ValueError('authenticated paused gate command refused the real masked fixture')
            result = json.loads(completed.stdout)
        else:
            result = install(candidate, common, tool)
        if (result['result'] != expected or result['approval_sha256'] != choice[1]
                or result['roles'] != sorted(markers) or result['resume_authorized'] or result['services_changed']):
            raise ValueError('paused gate returned an unexpected scope')

    refused(Namespace(**{**vars(prepare), 'paused_approval': (choice[0], '00' * 32)}), 'approval differs')
    interrupted(prepare, {'core'})
    amended = output / 'same-paused-approval-different-digest.json'
    common['write'](amended, approval.read_bytes() + b'\n')
    refused(Namespace(**{**vars(prepare), 'paused_approval': (str(amended), digest(amended))}),
            'marker differs from the approved operation')
    verify({'core'})
    complete(restore, 'CASHU_HARD_FENCES_RESTORED')
    verify(set())
    if real_masks:
        with active_process(workers['wallet']):
            refused(prepare, 'runtime UID still has a process')
    complete(prepare, 'CASHU_PAUSED_GATE_PREPARED')
    complete(prepare, 'CASHU_PAUSED_GATE_PREPARED')
    verify(set(markers))
    try:
        yield
    finally:
        interrupted(restore, {'wallet', 'cashu_signer'})
        complete(restore, 'CASHU_HARD_FENCES_RESTORED')
        complete(restore, 'CASHU_HARD_FENCES_RESTORED')
        verify(set())


def qualify_advance(capsule, bootstrap, stage, reference, output, policy_path, common,
                    workers, binaries, configurations, roles, systemd_masks):
    manifest = output / 'provider-review/funded-restore-manifest.json'
    credential_store = output / 'disposable-credential-store' if systemd_masks else None
    args = Namespace(stage=reference, stage_sha256=digest(reference / 'STAGED.json'),
                     policy=policy_path, policy_sha256=digest(policy_path), signer_credential_store=credential_store,
                     advance_from=(str(stage), digest(stage / 'STAGED.json')),
                     advance_evidence=(str(manifest), digest(manifest)))
    tool = runpy.run_path(str(capsule / 'launch-cashu-backup.py'))['verify'](capsule, bootstrap)
    install = runpy.run_path(str(capsule / 'install-cashu-restore.py'))['install']
    older_images = stage / 'fenced-images'
    active = {role: Path(worker['state']) for role, worker in workers.items()}
    saved = output / 'pre-advance-originals'
    advanced = output / 'advanced-fenced-runtime'
    for directory in (saved, advanced):
        directory.mkdir(mode=0o700)

    def invoke(candidate):
        command = ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'), bootstrap,
                   'install-fenced', str(reference), candidate.stage_sha256, str(policy_path), candidate.policy_sha256,
                   '--advance-from', *candidate.advance_from, '--advance-evidence', *candidate.advance_evidence]
        if candidate.signer_credential_store is not None:
            command += ['--signer-credential-store', str(candidate.signer_credential_store)]
        if getattr(candidate, 'paused_gate', None):
            command += ['--paused-gate', candidate.paused_gate, '--paused-approval', *candidate.paused_approval]
        return subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, timeout=120)

    def refused(candidate, expected, **overrides):
        try:
            install(candidate, {**common, **overrides}, tool)
        except ValueError as error:
            if expected not in str(error):
                raise
        else:
            raise ValueError('advancement accepted an unverified configuration')

    def image(directory):
        return {path.name: digest(path) for path in directory.iterdir()}

    def interrupt(roles_changed):
        def stop(role):
            roles_changed.append(role)
            raise InterruptedError('fixture stopped between durable per-role transitions')
        return stop

    archived, published = [], []
    try:
        for role, state in active.items():
            state.rename(saved / role)
            (output / 'installed-fenced-runtime' / role).rename(state)
        before = {role: image(state) for role, state in active.items()}
        refused(Namespace(**{**vars(args), 'advance_from': None, 'advance_evidence': None}),
                'existing target differs')
        refused(Namespace(**{**vars(args), 'advance_evidence': (str(manifest), '00' * 32)}),
                'evidence bytes differ')
        if before != {role: image(state) for role, state in active.items()}:
            raise ValueError('refused advancement touched the older installed images')
        try:
            install(args, common, tool, after_archive=interrupt(archived))
        except InterruptedError:
            if len(archived) != 1 or any(
                    os.path.lexists(state) if role in archived else image(state) != before[role]
                    for role, state in active.items()):
                raise ValueError('interrupted retention left an unexpected installed state')
        else:
            raise ValueError('test did not interrupt the first durable retention')
        changed_evidence = output / 'provider-review/same-evidence-different-digest.json'
        common['write'](changed_evidence, manifest.read_bytes() + b'\n')
        refused(Namespace(**{**vars(args), 'advance_evidence': (str(changed_evidence), digest(changed_evidence))}),
                'differs from the retained operation')
        if systemd_masks:
            with active_process(workers['wallet']):
                refused(args, 'runtime UID still has a process')
        try:
            install(args, common, tool, after_publish=interrupt(published))
        except InterruptedError:
            if published != archived or image(active[published[0]]) != image(reference / 'fenced-images' / published[0]):
                raise ValueError('interrupted publication did not install the expected fenced image')
        else:
            raise ValueError('test did not interrupt the first durable publication')

        if systemd_masks:
            outcome = invoke(args)
            if outcome.returncode:
                raise ValueError('authenticated advancement refused the partially published masked fixture')
            result = json.loads(outcome.stdout)
        else:
            result = install(args, common, tool)
        if (result['result'] != 'CASHU_RESTORE_ADVANCED_FENCED' or result['already_installed'] != published
                or result['resume_authorized'] or result['services_changed']
                or result['signer_clients_verified'] != (['cashu_signer', 'signer'] if systemd_masks else [])
                or result['advance_evidence_sha256'] != args.advance_evidence[1]):
            raise ValueError('advancement result differs from the exact paired evidence')
        retained = {role: Path(path) for role, path in result['retained_paths'].items()}
        if set(retained) != set(roles):
            raise ValueError('advancement omitted a retained role')
        for role, path in retained.items():
            if (image(path) != image(older_images / role) or path.parent.stat().st_mode & 0o777 != 0o700
                    or path.parent.stat().st_uid != 0):
                raise ValueError('advancement changed an older image or exposed its retained directory')
        if install(args, common, tool)['already_installed'] != sorted(roles):
            raise ValueError('completed fenced advancement refused exact retry')
        startup = output.parent / 'advanced-startup'
        startup.mkdir(mode=0o711)
        verified = verify_restored_runtime_fences(reference, startup, workers, binaries, configurations, roles)
        restart = runpy.run_path(str(Path(__file__).with_name('cashu-funded-restart-fixture.py')))
        signer_client = reference / 'checkpoint-evidence/recovery-material/cashu-client.hex'
        with (exercise_paused_gate(install, invoke, args, common, tool, workers, output, systemd_masks),
              ExitStack() as accounts):
            systemd_roles, ciphertext, encrypted = {}, None, None
            if systemd_masks:
                systemd_roles = {role: accounts.enter_context(fixture_role_account(workers[role], output.parent, role))
                                 for role in ('core', 'wallet', 'cashu_signer')}
                ciphertext = credential_store / 'lg-cashu-nostr-client'
                encrypted = (ciphertext, *systemd_roles['cashu_signer'])
            installed_paused_restart = restart['rehearse_installed'](
                output.parent / 'installed-restart', workers, policy_path, signer_client, systemd_roles, ciphertext)
            paused_restart = restart['rehearse'](output.parent / 'restart', workers, binaries, configurations,
                                                 binaries['core_daemon'], signer_client, encrypted)
        common['check_quiescence'](workers, common['Budget'](65536, 30))
        for role, state in active.items():
            if image(state) != image(reference / 'fenced-images' / role):
                raise ValueError('paused restart changed an installed fenced role')
    finally:
        for role, state in active.items():
            if (saved / role).exists():
                if state.exists():
                    state.rename(advanced / role)
                (saved / role).rename(state)
    return {'recovery_fences_verified': verified, 'older_files_retained': True, 'archive_interrupt_recovered': True,
            'publish_interrupt_recovered': True, 'changed_evidence_retry_refused': True, 'exact_retry_verified': True,
            'authenticated_cli_retry_with_actual_masks': systemd_masks,
            'paused_gate': {'prepare_and_restore_interruption_recovered': True,
                            'different_approval_refused': True, 'original_markers_restored': True,
                            'authenticated_cli_with_actual_masks': systemd_masks},
            'installed_paused_restart': installed_paused_restart,
            'paused_restart': paused_restart, 'resume_authorized': False}


def qualify_restore(source, output_root, collected, identity, selected, store,
                    workers, binaries, configurations, native_roles, configured_keys, policy_path,
                    common, systemd_masks, funded_source, capture_reference):
    """Pass the real encrypted collector output through the isolated launcher."""
    scripts = source / 'deploy/scripts'
    capsule = output_root / 'restore-capsule'
    capsule.mkdir(mode=0o700)
    files = runpy.run_path(str(scripts / 'launch-cashu-backup.py'))['FILES']
    for name in files:
        target = capsule / name
        shutil.copyfile(scripts / name, target)
        target.chmod(0o400)
    manifest = capsule / 'TOOL.json'
    manifest.write_text(json.dumps({'version': 1, 'source_commit': '01' * 20,
                                    'files': {name: digest(capsule / name) for name in files}}))
    manifest.chmod(0o600)
    ciphertext = collected / 'checkpoint.tar.age'
    checksum = digest(ciphertext)
    def invoke(destination, digest_override=None, key=identity, collection=collected):
        archive = collection / 'checkpoint.tar.age'
        return subprocess.run(
            ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
             digest(manifest), 'stage', str(archive), digest_override or digest(archive),
             str(collection / 'COMPLETE.json'),
             str(key), str(destination), '--age', str(selected['age']), '--age-sha256',
             digest(selected['age']), '--statectl', str(selected['statectl']),
             '--key-verifier', str(selected['copy']), '--max-bytes', str(1024**3)],
            stdin=subprocess.DEVNULL, capture_output=True, timeout=180,
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
        )

    destination = output_root / 'restore-positive'
    outcome = invoke(destination)
    if outcome.returncode:
        raise ValueError('actual restore staging CLI refused original fixture archive')
    result = json.loads(outcome.stdout)
    roles = sorted(('core', 'membership', 'payout', 'signer', 'wallet', 'cashu_signer'))
    if (result['result'] != 'CASHU_RESTORE_STAGED' or result['fenced_roles'] != roles
            or result['keys_verified'] is not True or result['resume_authorized'] is not False
            or result['restore_authorized'] is not False
            or result['external_reconciliation_verified'] is not False
            or result != json.loads((destination / 'STAGED.json').read_bytes())
            or (destination / 'INCOMPLETE').exists() or (destination / 'decrypted.tar').exists()):
        raise ValueError('restore staging outcome differs from expected scope')
    manifest_path = destination / 'FENCED_IMAGES.json'
    image_manifest = json.loads(manifest_path.read_bytes())
    if (digest(manifest_path) != result['fenced_images_sha256']
            or image_manifest['checkpoint_id'] != result['checkpoint_id']
            or sorted(image_manifest['roles']) != roles):
        raise ValueError('staged image manifest differs from receipt')
    for role in roles:
        directory = destination / 'fenced-images' / role
        marker = ('CASHU_RECOVERY_REQUIRED' if role in ('wallet', 'cashu_signer')
                  else 'CYBERHERD_RECOVERY_REQUIRED')
        if ((directory / marker).read_text().strip() != result['checkpoint_id']
                or directory.stat().st_uid != 0 or directory.stat().st_mode & 0o777 != 0o700
                or any(p.stat().st_uid != 0 or p.stat().st_mode & 0o777 != 0o600
                       for p in directory.iterdir())):
            raise ValueError('staged images lack protected ownership or recovery fence')
        for item in image_manifest['roles'][role]:
            image = directory / item['name']
            if digest(image) != item['sha256'] or image.stat().st_size != item['bytes']:
                raise ValueError('staged image differs from retained manifest')
    image = destination / 'fenced-images/wallet' / store.name
    for name in configured_keys:
        if digest(destination / 'fenced-images/wallet' / name) != digest(
                Path(workers['wallet']['state']) / name):
            raise ValueError('fenced restore lost a configured receiving key')
    with closing(sqlite3.connect(image.as_uri() + '?immutable=1&mode=ro', uri=True)) as db:
        if db.execute('SELECT value FROM collector_fixture_wal').fetchall() != [(b'synthetic committed WAL value',)]:
            raise ValueError('staged native store lost committed WAL content')
    if any(p.stat().st_mode & 0o111 for p in (destination / 'checkpoint-evidence/programs').iterdir()):
        raise ValueError('stager made an archived program executable')
    installation = qualify_install(capsule, digest(manifest), destination, output_root, policy_path, common,
                                   workers, binaries, configurations, native_roles, systemd_masks)

    def invoke_review(output, *options):
        return subprocess.run(
            ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
             digest(manifest), 'review', str(destination), digest(destination / 'STAGED.json'),
             '--output', str(output), *options],
            stdin=subprocess.DEVNULL, capture_output=True, timeout=120,
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
        )

    review_path = output_root / 'restore-worklist.json'
    review = invoke_review(review_path)
    if review.returncode or review_path.stat().st_mode & 0o777 != 0o600:
        raise ValueError('authenticated worklist did not review the sealed checkpoint privately')
    report = json.loads(review_path.read_bytes())
    if (report['version'] != 1 or report['staged_receipt_sha256'] != digest(destination / 'STAGED.json')
            or report['reference_receipt_sha256'] is not None
            or report['selection'] != {'group': None, 'after': None, 'limit': 100}
            or report['resume_authorized'] is not False):
        raise ValueError('restore worklist omitted its exact source or page binding')
    if funded_source is not None:
        observed = verify_funded_stage(destination, report, funded_source, common)
        reference = output_root / 'restore-reference'
        if invoke(reference, collection=capture_reference()).returncode:
            raise ValueError('authenticated stager refused the later funded reference')
        compare = runpy.run_path(str(scripts / 'cashu-funded-provider-fixture.py'))['compare_funded_providers']
        providers = compare(capsule, digest(manifest), destination, reference, funded_source,
                            output_root / 'provider-review', binaries['core'], common)
        signal = funded_source / 'exchange/restore-review-finished.pending'
        common['write_json'](signal, True)
        signal.chmod(0o644)
        signal.rename(signal.with_suffix('.json'))
        advancement = qualify_advance(capsule, digest(manifest), destination, reference, output_root, policy_path,
                                      common, workers, binaries, configurations, native_roles, systemd_masks)
        return {**observed, 'actual_authenticated_launcher': True, 'real_age_archive': True,
                'staged_roles': len(roles), 'committed_wal_preserved': True, 'image_manifest_verified': True,
                'fenced_installation': installation, 'restored_runtime_bytes_unchanged': True,
                'funded_provider_comparison': providers,
                'fenced_advancement': advancement,
                'external_reconciliation_verified': False, 'resume_authorized': False}
    if any(json.loads(review.stdout)['counts'].values()):
        raise ValueError('empty staged checkpoint produced unexpected review work')
    journal = destination / 'fenced-images/wallet/acquisition.sqlite'
    with closing(sqlite3.connect(journal.as_uri() + '?immutable=1&mode=ro', uri=True)) as db:
        mints = dict(db.execute('SELECT mint_id,mint_url FROM cdk_mint_namespaces ORDER BY mint_id'))
    inventory_path = output_root / 'custody-inventory.json'
    inventory_result = invoke_review(inventory_path, '--custody-inventory')
    if inventory_result.returncode or json.loads(inventory_result.stdout)['mint_namespaces'] != 2 or len(mints) != 2:
        raise ValueError('authenticated launcher did not identify both retained mint namespaces')
    inventory = json.loads(inventory_path.read_bytes())
    if ({mint: namespace['mint_url'] for mint, namespace in inventory['mints'].items()} != mints
            or inventory['resume_authorized'] is not False):
        raise ValueError('custody inventory lost its retained issuer scope')
    observations = {}
    for index, (mint, mint_url) in enumerate(mints.items()):
        custody_selection = output_root / f'empty-custody-selection-{index}.json'
        custody = invoke_review(custody_selection, '--custody-mint', mint)
        if custody.returncode or json.loads(custody.stdout)['result'] != 'CASHU_RESTORE_CUSTODY_SELECTED':
            raise ValueError('authenticated launcher did not select an original mint custody namespace')
        custody_file = output_root / f'empty-custody-observation-{index}.json'
        observed = subprocess.run([str(selected['copy']), 'observe-restored-custody', str(custody_selection),
                                   digest(custody_selection), mint_url, str(custody_file)], stdin=subprocess.DEVNULL,
                                  capture_output=True, timeout=15, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
        if observed.returncode or custody_file.stat().st_mode & 0o777 != 0o600:
            raise ValueError('native statectl did not observe the sealed empty custody page')
        custody_receipt = json.loads(custody_file.read_bytes())
        if (custody_receipt['proofs'] or custody_receipt['mint_id'] != mint
                or custody_receipt['total_candidates'] != 0 or custody_receipt['queried_mint'] is not False
                or custody_receipt['resume_authorized'] is not False
                or custody_receipt['selection_sha256'] != digest(custody_selection)):
            raise ValueError('empty custody observer reported a lookup or missing source binding')
        observations[mint] = [{'file': custody_file.name, 'sha256': digest(custody_file)}]

    stage_hash = digest(destination / 'STAGED.json')

    def compare_custody(name, **evidence):
        plan = output_root / (name + '-evidence.json')
        common['write_json'](plan, {'version': 1, 'treasury': None, 'feeder': None, **evidence})
        report = output_root / (name + '-comparison.json')
        outcome = subprocess.run(
            ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
             digest(manifest), 'reconcile', str(destination), stage_hash,
             str(plan), digest(plan), '--output', str(report)],
            stdin=subprocess.DEVNULL, capture_output=True, timeout=120,
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
        )
        return outcome, report

    mint = next(iter(mints))
    compared, custody_report = compare_custody('empty-custody', custody={
        'mint_id': mint, 'staged_receipt_sha256': stage_hash, 'observations': observations[mint]})
    if compared.returncode or custody_report.stat().st_mode & 0o777 != 0o600:
        raise ValueError('authenticated comparison did not accept the complete empty custody observation')
    custody_comparison = json.loads(custody_report.read_bytes())
    if (custody_comparison['custody']['selected_count'] != 0
            or custody_comparison['custody']['observation_sha256'] != [observations[mint][0]['sha256']]
            or custody_comparison['resume_authorized'] is not False):
        raise ValueError('custody comparison result lost the selected namespace or authority boundary')
    complete, complete_path = compare_custody('all-empty-custody', custody_all={
        'staged_receipt_sha256': stage_hash, 'mints': observations})
    if complete.returncode or complete_path.stat().st_mode & 0o777 != 0o600:
        raise ValueError('authenticated comparison refused complete custody evidence')
    complete_report = json.loads(complete_path.read_bytes())
    batch = complete_report['custody_all']
    if (batch['mint_namespaces_compared'] != 2 or batch['selected_count'] != 0
            or set(batch['mints']) != set(mints) or complete_report['resume_authorized'] is not False):
        raise ValueError('complete custody comparison omitted a mint namespace')
    omitted, omitted_path = compare_custody('omitted-empty-custody', custody_all={
        'staged_receipt_sha256': stage_hash, 'mints': {mint: observations[mint]}})
    if (omitted.returncode != 1 or omitted.stdout or omitted_path.exists()
            or omitted.stderr != b'Cashu restore reconciliation review refused. Preserve the fenced checkpoint.\n'):
        raise ValueError('authenticated comparison accepted the omitted empty mint namespace')
    for kind, option in [('strike', '--strike-intent'), ('mint', '--mint-intent'),
                         ('feeder', '--feeder-request')]:
        unknown_selection = output_root / f'unknown-{kind}-selection.json'
        unknown = invoke_review(unknown_selection, option, '00000000-0000-4000-8000-000000000001')
        if (unknown.returncode != 1 or unknown.stdout or unknown_selection.exists()
                or unknown.stderr != b'Cashu restore review refused. Preserve the fenced checkpoint.\n'):
            raise ValueError('authenticated restore launcher selected an absent original operation')
    missing_evidence = output_root / 'missing-original.json'
    missing_evidence.write_text(json.dumps({'version': 1, 'treasury': None,
        'feeder': {'request_id': '00000000-0000-4000-8000-000000000001',
                   'gateway_origin': 'http://127.0.0.1:1/',
                   'observation': {'file': 'absent-provider-output.json', 'sha256': '00' * 32}}}))
    missing_evidence.chmod(0o600)
    comparison_output = output_root / 'missing-original-comparison.json'
    comparison = subprocess.run(
        ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
         digest(manifest), 'reconcile', str(destination), digest(destination / 'STAGED.json'),
         str(missing_evidence), digest(missing_evidence), '--output', str(comparison_output)],
        stdin=subprocess.DEVNULL, capture_output=True, timeout=120,
        env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
    )
    if (comparison.returncode != 1 or comparison.stdout or comparison_output.exists()
            or comparison.stderr != b'Cashu restore reconciliation review refused. Preserve the fenced checkpoint.\n'):
        raise ValueError('actual recovery comparison launcher accepted absent provider evidence')
    # Mutate one disposable stage image after authentication. Review must
    # refuse changed bytes even though its STAGED receipt still matches.
    tampered_output = output_root / 'tampered-worklist.json'
    with image.open('ab') as mutated:
        mutated.write(b'changed')
    try:
        tampered = invoke_review(tampered_output)
        if (tampered.returncode != 1 or tampered.stdout or tampered_output.exists()
                or tampered.stderr != b'Cashu restore review refused. Preserve the fenced checkpoint.\n'):
            raise ValueError('restore review accepted a changed staged image')
    finally:
        with image.open('r+b') as mutated:
            mutated.truncate(image.stat().st_size - len(b'changed'))
    wrong_digest = output_root / 'restore-wrong-digest'
    wrong_key = output_root / 'other-age-identity'
    subprocess.run([str(selected['keygen']), '-o', str(wrong_key)], stdin=subprocess.DEVNULL,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, check=True)
    failed_decrypt = output_root / 'restore-wrong-identity'
    for attempt in (invoke(wrong_digest, '00' * 32), invoke(destination), invoke(failed_decrypt, key=wrong_key)):
        if (attempt.returncode != 1 or attempt.stdout
                or attempt.stderr != b'Cashu restore staging refused. Preserve any incomplete private stage.\n'):
            raise ValueError('restore staging did not refuse cleanly')
    if (wrong_digest.exists() or not (failed_decrypt / 'INCOMPLETE').is_file()
            or (failed_decrypt / 'fenced-images').exists() or (failed_decrypt / 'STAGED.json').exists()
            or digest(ciphertext) != checksum):
        raise ValueError('failed restore altered source or promoted an incomplete copy')
    return {'actual_authenticated_launcher': True, 'real_age_archive': True,
            'staged_roles': len(roles), 'committed_wal_preserved': True, 'image_manifest_verified': True,
            'restored_runtime_fences_refused': installation['exact_images_installed_fenced'],
            'fenced_installation': installation,
            'restored_runtime_roles_isolated': True, 'restored_runtime_bytes_unchanged': True,
            'wallet_fence_checked_before_keys': True, 'cashu_signer_fence_checked_without_client_credential': True,
            'configured_receiving_keys_preserved': len(configured_keys),
            'all_custody_namespaces_compared': batch['mint_namespaces_compared'],
            'omitted_empty_namespace_refused': True,
            'refusal_cases': 3, 'archive_programs_executable': False,
            'offline_worklist_launcher_verified': True, 'worklist_tampered_image_refused': True,
            'worklist_source_and_page_bound': True,
            'unknown_strike_selection_refused': True,
            'unknown_mint_and_feeder_selections_refused': True,
            'comparison_missing_evidence_refused': True,
            'empty_custody_observed_by_native_statectl': True,
            'empty_custody_compared_by_authenticated_launcher': True,
            'external_reconciliation_verified': False, 'resume_authorized': False}


def qualify(source,root,states,uids,binaries,configurations,native_policy,args,cleanup,reference=None):
    config_root=root/'collector-config'; config_root.mkdir(mode=0o700)
    output_root=root/'collector-output'; output_root.mkdir(mode=0o700)
    selected={}
    for name,path,expected in [('copy',args.collector_copy_cli,args.collector_copy_sha256),
                              ('age',args.age_cli,args.age_sha256),
                              ('keygen',args.age_keygen_cli,args.age_keygen_sha256)]:
        if digest(path)!=expected: raise ValueError('explicit collector fixture artifact differs')
        target=root/('collector-'+name); shutil.copyfile(path,target); target.chmod(0o555)
        if digest(target)!=expected: raise ValueError('collector fixture artifact changed')
        selected[name]=target
    selected['statectl']=root/'statectl'
    selected['tar']=Path('/usr/bin/tar').resolve(strict=True)
    # Retain a committed WAL/SHM set after its only connection has exited.
    # This adds synthetic nonfinancial rows to a disposable actual CDK schema.
    store=next(states['wallet'].glob('mint-*.sqlite'))
    scratch=root/'collector-wal-source.sqlite'
    with closing(sqlite3.connect(store.as_uri()+'?mode=ro',uri=True)) as original, closing(sqlite3.connect(scratch)) as copy:
        original.backup(copy)
    with closing(sqlite3.connect(scratch)) as writer:
        writer.execute('PRAGMA journal_mode=WAL');writer.execute('PRAGMA wal_autocheckpoint=0')
        writer.execute('PRAGMA wal_checkpoint(TRUNCATE)')
        writer.execute('CREATE TABLE collector_fixture_wal(value BLOB NOT NULL)')
        writer.execute('INSERT INTO collector_fixture_wal VALUES(?)',[b'synthetic committed WAL value'])
        writer.commit()
        retained={suffix:Path(str(scratch)+suffix).read_bytes() for suffix in ('','-wal','-shm')}
    for suffix,raw in retained.items():
        target=Path(str(store)+suffix);target.write_bytes(raw);target.chmod(0o600)
        os.chown(target,uids['wallet'],uids['wallet'])
    identity=config_root/'disposable-backup-identity'
    created=subprocess.run([str(selected['keygen']),'-o',str(identity)],stdin=subprocess.DEVNULL,
        capture_output=True,timeout=10)
    if created.returncode: raise ValueError('synthetic recovery identity creation failed')
    recipient=subprocess.check_output([str(selected['keygen']),'-y',str(identity)],text=True).strip()
    native_roles={'core':'core','membership':'membership','payout':'payout','signer':'payout_signer',
                  'wallet':'wallet','cashu_signer':'signer'}
    workers={role:{'uid':uids[key],'gid':uids[key],'state':str(states[key]),
        'unit':native_policy['components'][role]['unit'] if role in native_policy['components']
               else f'{root.name}-{role}.service'} for role,key in native_roles.items()}
    cashu_client=config_root/'cashu-client.hex'; cashu_client.write_text('0'*63+'2');cashu_client.chmod(0o600)
    payout_client=config_root/'payout-client.hex'; payout_client.write_text('0'*63+'5\n');payout_client.chmod(0o600)
    helper_files = []
    runtime_binaries = {role: binaries[key] for role, key in native_roles.items()}
    if args.funded_fixture is not None:
        runtime_binaries['core'] = binaries['core_daemon']
        signer_config = json.loads(configurations['signer'].read_bytes())
        invocations = Path(signer_config['nak']['private_directory']) / 'invocations'
        helper_files.append({'source': str(invocations), 'uid': uids['signer'], 'secret': True})
    policy={'version':1,'operator_exclusive':True,'workers':workers,
        'native_policy':str(root/'recovery/maintenance-policy.toml'),
        'core_config':str(configurations['core']),'membership_config':str(configurations['membership']),
        'payout_config':str(configurations['payout']),'payout_signer_config':str(configurations['payout_signer']),
        'wallet_config':str(configurations['wallet']),'cashu_signer_config':str(configurations['signer']),
        'cashu_client_source':str(cashu_client),'payout_client_source':str(payout_client),
        'historical_receiving_keys':[],'extra_files':helper_files,
        'artifacts':{name:{'path':str(path),'sha256':digest(path)} for name,path in selected.items() if name!='keygen'},
        'runtime_artifacts':{role:{'path':str(path),'sha256':digest(path)} for role,path in runtime_binaries.items()},
        'recipient':recipient,'component_bytes':128*1024**2,'total_bytes':1024**3,'deadline_seconds':300}
    policy_path=config_root/'policy.json'
    policy_path.write_text(json.dumps(policy));policy_path.chmod(0o600)
    module=runpy.run_path(str(source/'deploy/scripts/collect-cashu-backup.py'),
        init_globals={'verified_tool_identity':lambda:{'fixture':True,'authenticated_launcher_executed':False}})
    collect=module['collect']; globals_=collect.__globals__
    if args.systemd_masks:
        prepare_systemd_masks(root, workers, module['GLOBAL_LOCK'], cleanup)
    else:
        globals_['GLOBAL_LOCK']=config_root/'maintenance.lock'
    check=module['check_quiescence']
    original_run=subprocess.run
    active=False
    calls=0

    def service_status(command,**kwargs):
        nonlocal calls
        if command[:2]!=['/usr/bin/systemctl','show']:
            raise ValueError('fixture only mocks systemctl show')
        calls+=1; unit=command[-1]
        if unit not in {w['unit'] for w in workers.values()}: raise ValueError('unexpected service identity')
        return types.SimpleNamespace(returncode=0,stdout=(f'Id={unit}\nLoadState=masked\n'
            f'ActiveState={"active" if active else "inactive"}\nSubState=dead\nMainPID=0\nControlPID=0\n'
            'UnitFileState=masked\nControlGroup=\n').encode())

    def checked_status(workers,budget):
        with (nullcontext() if args.systemd_masks else patch.object(subprocess,'run',side_effect=service_status)):
            check(workers,budget)
    globals_['check_quiescence']=checked_status
    original={str(p):digest(p) for directory in states.values() for p in directory.rglob('*') if p.is_file()}
    destination=output_root/'positive'
    result=collect(policy_path,digest(policy_path),destination)
    if (result['result']!='CASHU_COORDINATED_BACKUP_COLLECTED' or not result['quiescence_verified']
            or result['restore_authorized'] or result['resume_authorized']
            or result['recovery_key_possession_verified'] or not result['plaintext_retained']
            or (destination/'INCOMPLETE').exists() or not (destination/'COMPLETE.json').exists()):
        raise ValueError('collector result/marker scope differs')
    ciphertext=destination/'checkpoint.tar.age'
    if digest(ciphertext)!=result['ciphertext_sha256']:raise ValueError('ciphertext receipt differs')
    collection=json.loads((destination/'plaintext/collection.json').read_bytes())
    for item in collection['originals']:
        if digest(destination/'plaintext'/item['path'])!=item['sha256']:
            raise ValueError('retained copy digest differs from collection metadata: '+item['path'])
    restored=root/'collector-decrypted.tar'
    with restored.open('xb') as output:
        os.fchmod(output.fileno(),0o600)
        opened=original_run([str(selected['age']),'--decrypt','-i',str(identity),str(ciphertext)],
            stdout=output,stderr=subprocess.PIPE,stdin=subprocess.DEVNULL,timeout=60)
    if opened.returncode:raise ValueError('actual collected archive decryption failed')
    with tarfile.open(restored,'r:') as archive:
        members={m.name:m for m in archive.getmembers()}
        if any(m.issym() or m.islnk() or not(m.isfile() or m.isdir()) for m in members.values()):
            raise ValueError('unexpected archive entry type')
        for path in (destination/'plaintext').rglob('*'):
            if not path.is_file():continue
            name=str(path.relative_to(destination/'plaintext'))
            if name not in members or digest(path)!=hashlib.sha256(archive.extractfile(members[name]).read()).hexdigest():
                raise ValueError('encrypted archive omitted or changed a retained component')
    after={str(p):digest(p) for directory in states.values() for p in directory.rglob('*') if p.is_file()}
    if original!=after:raise ValueError('collector changed original application bytes')
    _, configured_keys = module['receiving_material'](json.loads(configurations['wallet'].read_bytes()))

    def capture_reference():
        reference_path = output_root / 'funded-reference'
        with reference():
            receipt = collect(policy_path, digest(policy_path), reference_path)
        if (receipt['checkpoint_id'] == result['checkpoint_id'] or receipt['resume_authorized']
                or not receipt['quiescence_verified']):
            raise ValueError('funded reference collection did not verify its own checkpoint')
        return reference_path

    restore_result = qualify_restore(source, output_root, destination, identity, selected, store,
                                    workers, binaries, configurations, native_roles, configured_keys, policy_path,
                                    dict(module, GLOBAL_LOCK=globals_['GLOBAL_LOCK'], check_quiescence=checked_status),
                                    args.systemd_masks, args.funded_fixture, capture_reference)
    key_stage=destination/'plaintext/recovery-material'
    key_negatives=0
    def key_check(success):
        outcome=original_run([str(selected['copy']),'verify-recovery-keys',str(key_stage)],
            stdin=subprocess.DEVNULL,capture_output=True,timeout=3)
        if (outcome.returncode==0)!=success:raise ValueError('unexpected native recovery-key outcome')
        if not success and (outcome.stdout or outcome.stderr!=b'Recovery key verification refused; preserve the incomplete private stage.\n'):
            raise ValueError('recovery-key refusal disclosed private context')
    for name,replacement in [('wallet-seed.bin',bytes([9])*64),('receiving-key.bin',bytes(32)),
                             ('cashu-client.hex',('0'*63+'5').encode()),
                             ('payout-client.hex',('0'*63+'2').encode()),
                             *((name, bytes(32)) for name in configured_keys)]:
        path=key_stage/name; saved=path.read_bytes()
        try:path.write_bytes(replacement);key_check(False);key_negatives+=1
        finally:path.write_bytes(saved)
    bindings_path=key_stage/'key-bindings.json';saved_bindings=bindings_path.read_bytes()
    for field,name,secret,public in [
        ('payout_client','payout-client.hex','2',json.loads(saved_bindings)['cashu_client']),
        ('cashu_client','cashu-client.hex','3',json.loads(saved_bindings)['owner'])]:
        path=key_stage/name;saved=path.read_bytes();changed=json.loads(saved_bindings);changed[field]=public
        try:
            path.write_text('0'*63+secret);bindings_path.write_text(json.dumps(changed))
            key_check(False);key_negatives+=1
        finally:path.write_bytes(saved);bindings_path.write_bytes(saved_bindings)
    cashu_key=key_stage/'cashu-client.hex';saved=cashu_key.read_bytes()
    try:cashu_key.write_bytes(saved+b'\n');key_check(True)
    finally:cashu_key.write_bytes(saved)
    # Exercise real output-file limits and deterministic process timeout without
    # repeating database initialization or changing application state.
    fault=root/'collector-pipeline-faults';fault.mkdir(mode=0o700)
    fault_plain=fault/'plain';fault_plain.mkdir(mode=0o700)
    payload=fault_plain/'synthetic';payload.write_bytes(b'synthetic checkpoint\0'*32768);payload.chmod(0o600)
    before_fault=digest(payload)
    marker=fault/'INCOMPLETE';marker.write_bytes(b'retain interrupted pipeline');marker.chmod(0o600)
    pipeline=module['encrypt'];budget_class=module['Budget']
    try:pipeline(fault_plain,fault/'size-limited.age',selected,recipient,budget_class(payload.stat().st_size+256,5))
    except ValueError:pass
    else:raise ValueError('actual ciphertext size limit was ignored')
    if not(fault/'size-limited.age').exists() or (fault/'size-limited.age').stat().st_size>256:
        raise ValueError('bounded interrupted ciphertext was not retained')
    stall=fault/'stall';stall.write_text('#!/bin/sh\nexec /usr/bin/sleep 60\n');stall.chmod(0o500)
    started=time.monotonic()
    try:pipeline(fault_plain,fault/'timed-out.age',{**selected,'age':stall},recipient,budget_class(4*1024**2,0.2))
    except ValueError:pass
    else:raise ValueError('stalled encryption process was not bounded')
    if time.monotonic()-started>5 or not marker.exists() or digest(payload)!=before_fault:
        raise ValueError('pipeline fault changed retained plaintext or failed to stop')
    negative=0
    active=True
    with active_process(workers['wallet']) if args.systemd_masks else nullcontext():
        try:collect(policy_path,digest(policy_path),output_root/'active-writer')
        except ValueError:negative+=1
        else:raise ValueError('active writer was accepted')
    if (output_root/'active-writer').exists():raise ValueError('active-writer refusal reached copying')
    active=False
    try:collect(policy_path,'aa'*32,output_root/'wrong-policy')
    except ValueError:negative+=1
    else:raise ValueError('wrong reviewed policy hash accepted')
    try:collect(policy_path,digest(policy_path),destination)
    except FileExistsError:negative+=1
    else:raise ValueError('preexisting destination overwritten')
    # A valid but wrong seed must refuse even though all database inventory
    # hashes and registrations still agree. Keep the failed raw/copy evidence.
    seed=states['wallet']/'wallet-seed.bin'; retained=seed.read_bytes()
    seed.write_bytes(bytes([9])*64)
    try:
        try:collect(policy_path,digest(policy_path),output_root/'wrong-seed')
        except ValueError:negative+=1
        else:raise ValueError('wrong seed was accepted with a matching epoch')
        if not(output_root/'wrong-seed/INCOMPLETE').exists():raise ValueError('failed checkpoint evidence lost')
        if (output_root/'wrong-seed/COMPLETE.json').exists():raise ValueError('failed checkpoint marked complete')
    finally:seed.write_bytes(retained)
    return {'result':'CASHU_COORDINATED_COLLECTOR_FIXTURE_VERIFIED',
        'native_database_inventory_and_correlations':True,'native_key_bindings':True,
        'actual_age_archive_roundtrip':True,'original_application_bytes_unchanged':True,
        'committed_wal_copy_hashes_verified':True,
        'refusal_cases':negative,'systemctl_calls_mocked':calls,'systemd_masks_verified':args.systemd_masks,
        'proc_uid_scan_actual':True,
        'native_key_material_refusal_cases':key_negatives,'client_credential_optional_lf_accepted':True,
        'encryption_output_limit_refused':True,'encryption_timeout_stub_refused':True,
        'maintenance_lock_path_is_disposable':True,'authenticated_launcher_executed':False,
        'empty_native_state_only':args.funded_fixture is None,'funded_restore_qualified':False,
        'installed_or_live_acceptance':False,'production_or_physical_side_effects':False,
        'restore_staging': restore_result}
