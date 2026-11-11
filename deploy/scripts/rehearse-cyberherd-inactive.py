#!/usr/bin/env python3
"""TEST ONLY: fresh CyberHerd installer on a disposable systemd CI VM.

Creates actual fixed service users, masks and journals on that disposable VM.
A harmless core stand-in proves the upgrade first; subsequent isolated native
worker qualification uses separate synthetic journals, zero budgets and a shadow
core. No real credential, transfer, public post or physical feeder is used.
Never use this disposable-runner orchestrator on a VPS.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import shutil
import stat
import subprocess
import tarfile
import tempfile
import tomllib


def run(*args):
    result = subprocess.run(args, env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'},
                            capture_output=True, text=True, timeout=300)
    if result.returncode:
        # All selected inputs are synthetic. Retain the actual failure in CI.
        raise RuntimeError(f'test-only subprocess failed: {args[0]}\n{result.stdout[-8000:]}\n{result.stderr[-8000:]}')
    return result.stdout


def core_upgrade(launcher, root, source, runtime, archive, commit):
    """Actual transaction, against new synthetic state and a harmless sleep unit.

    All paths are fixed by the production coordinator, so this function is ONLY
    reached after main's disposable-VM and fresh-namespace guards. The earlier
    canary installation must still have its original empty state directory.
    """
    state = Path('/var/lib/lightning-goats')
    assert state.stat().st_uid == runtime.pw_uid and not list(state.iterdir())
    config = Path('/etc/lightning-goats/config.receive.toml')
    unit = Path('/etc/systemd/system/lightning-goats-receive.service')
    assert not config.exists() and not config.is_symlink() and not unit.exists() and not unit.is_symlink()
    # Import only already-authenticated frozen tools, never archive Python.
    spec = importlib.util.spec_from_file_location('core_rehearsal_preflight', root / 'tools/preflight-release.py')
    preflight = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(preflight)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    preflight.preflight(archive, commit, digest)
    payload = root / 'core-payload'
    payload.mkdir(mode=0o700)
    preflight.RELEASE.verify_archive(archive, commit, payload)
    config_bytes = (payload / 'deploy/config.receive.toml.example').read_bytes()
    with config.open('xb') as output:
        output.write(config_bytes)
    config.chmod(0o644)
    with (source / 'core.db').open('rb') as incoming, (state / 'lightning-goats.db').open('xb') as output:
        shutil.copyfileobj(incoming, output)
        output.flush()
        os.fchown(output.fileno(), runtime.pw_uid, runtime.pw_gid)
        os.fchmod(output.fileno(), 0o600)
        os.fsync(output.fileno())
    # Stand-ins prove that both actual binary hashes change. The unit itself is
    # a non-admin sleep process, not any payment/network/feeder program.
    for binary in ('lightning-goatsd', 'lightning-goatsctl'):
        target = Path('/usr/local/bin') / binary
        target.write_text('#!/bin/sh\nexit 97\n')
        target.chmod(0o755)
    unit_bytes = b'[Unit]\nDescription=TEST ONLY harmless legacy core stand-in\n[Service]\nUser=lightning-goats\nGroup=lightning-goats\nNoNewPrivileges=yes\nExecStart=/usr/bin/sleep infinity\n'
    with unit.open('xb') as output:
        output.write(unit_bytes)
    unit.chmod(0o644)
    run('/usr/bin/systemctl', 'daemon-reload')
    run('/usr/bin/systemctl', 'start', 'lightning-goats-receive.service')
    base = Path('/var/lib/lightning-goats-upgrades')
    base.mkdir(mode=0o700, exist_ok=False)
    transaction = base / 'ci-core-upgrade'
    plan = json.loads(run(*launcher, '--cyberherd-core', 'prepare', str(archive), commit, digest, str(transaction)))
    assert plan['service_changed'] is False and plan['state_opened'] is False
    upgraded = json.loads(run(*launcher, '--cyberherd-core', 'apply', str(transaction), '--writers-stopped'))
    assert upgraded['upgraded'] is True and upgraded['core_masked'] is True and upgraded['core_started'] is False
    assert os.readlink(unit) == '/dev/null'
    for binary in ('lightning-goatsd', 'lightning-goatsctl'):
        assert hashlib.sha256((Path('/usr/local/bin') / binary).read_bytes()).digest() == hashlib.sha256((payload / binary).read_bytes()).digest()
    comparison = json.loads((transaction / 'COMPARISON.json').read_text())
    assert comparison['native']['accepted'] is True and comparison['native']['candidate_compared'] is True
    restored = json.loads(run(*launcher, '--cyberherd-core', 'finish', str(transaction), '--restore-original-unit'))
    assert restored['original_unit_restored'] is True and restored['core_started'] is False
    assert restored['explicit_core_restart_required'] is True
    assert unit.read_bytes() == unit_bytes and config.read_bytes() == config_bytes
    observed = dict(line.split('=', 1) for line in run('/usr/bin/systemctl', 'show', 'lightning-goats-receive.service',
                                                     '-p', 'ActiveState', '-p', 'SubState', '-p', 'MainPID').splitlines())
    assert observed == {'ActiveState': 'inactive', 'SubState': 'dead', 'MainPID': '0'}
    info = (state / 'lightning-goats.db').stat()
    assert info.st_uid == runtime.pw_uid and info.st_gid == runtime.pw_gid and stat.S_IMODE(info.st_mode) == 0o600
    evidence = json.loads((transaction / 'legacy/MANIFEST.json').read_text())
    assert evidence['report']['tables']['settled_payments']['rows'] == 1
    assert evidence['report']['tables']['feed_attempts']['rows'] == 1
    assert evidence['report']['tables']['message_outbox']['rows'] == 1
    return transaction / 'policy.toml', transaction / 'legacy'


def overlay_maintenance(launcher, root, fixture_directory, installation, core_archive, review_archive, source_commit):
    """Actual executor/systemd/native backup, ONLY inside main's disposable VM.

    Both actual application services join this loopback-only namespace. Existing
    fixed-path journals contain only the synthetic paid/pending/signed fixture.
    No real provider, relay, credential, gateway or production host is involved.
    """
    import http.server
    import threading
    if (os.geteuid() != 0 or os.environ.get('GITHUB_ACTIONS') != 'true'
            or not os.environ.get('GITHUB_RUN_ID', '').isdigit()
            or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net')
            or {v['ifname'] for v in json.loads(run('/usr/sbin/ip', '-j', 'link', 'show'))} != {'lo'}):
        raise ValueError('overlay rehearsal requires the guarded disposable isolated CI host')
    tools = root / 'tools'
    spec = importlib.util.spec_from_file_location('ci_overlay_worker_support', tools / 'qualify-cyberherd-workers.py')
    w = importlib.util.module_from_spec(spec); spec.loader.exec_module(w)
    run('/usr/sbin/ip', 'link', 'set', 'lo', 'up')
    namespace = Path('/proc') / str(os.getpid()) / 'ns/net'
    assert os.readlink(namespace) != os.readlink('/proc/1/ns/net')
    counts = {'post': 0}
    class Gateway(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_GET(self):
            payload = {'override_enabled': True, 'remote_enabled': False} if self.path == '/v1/feeder/override' else {}
            data = json.dumps(payload).encode()
            self.send_response(200 if self.path in ('/v1/feeder/override', '/healthz') else 404)
            self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data)))
            self.end_headers(); self.wfile.write(data)
        def do_POST(self):
            counts['post'] += 1; self.send_response(405); self.end_headers()
    gateway = http.server.ThreadingHTTPServer(('127.0.0.1', 18789), Gateway)
    thread = threading.Thread(target=gateway.serve_forever, daemon=True); thread.start()
    config_path = Path('/etc/lightning-goats/config.receive.toml')
    unit_path = Path('/etc/systemd/system/lightning-goats-receive.service')
    member_config = tomllib.loads(Path('/etc/cyberherd/config.toml').read_text())
    core = pwd.getpwnam('lightning-goats')
    value = tomllib.loads(config_path.read_text())
    value['service'] = {'mode': 'shadow', 'listen': '127.0.0.1:8787'}
    value['gateway']['url'] = 'http://127.0.0.1:18789/'
    value['informational'].update(interface_info_enabled=False, weather_enabled=False)
    value['nostr']['bunker_pubkey'] = member_config['project_pubkey']
    config_bytes = w.toml_text(value).encode(); config_path.write_bytes(config_bytes)
    template = (root / 'core-payload/deploy/systemd/lightning-goats-receive.service').read_text()
    lines = ['[Unit]', 'Description=TEST ONLY native overlay maintenance core', '[Service]',
             'Type=simple', 'User=lightning-goats', 'Group=lightning-goats',
             'ExecStart=/usr/local/bin/lightning-goatsd --config /etc/lightning-goats/config.receive.toml',
             'StateDirectory=lightning-goats', 'StateDirectoryMode=0700',
             'RuntimeDirectory=lightning-goats', 'RuntimeDirectoryMode=0700',
             'UMask=0077', 'Restart=no', 'TimeoutStopSec=30',
             'NetworkNamespacePath=' + str(namespace),
             'ReadWritePaths=/var/lib/lightning-goats /run/lightning-goats']
    lines += [name + '=' + data for name, data in w.properties(template)]
    for name in ('strike-api-key', 'strike-webhook-secret'):
        credential = root / (name + '.overlay-synthetic.cred')
        w.encrypt(credential, name, b'SYNTHETIC-OVERLAY-NO-EXTERNAL-ACCOUNT')
        lines.append('LoadCredentialEncrypted=' + name + ':' + str(credential))
    unit_bytes = ('\n'.join(lines) + '\n').encode()
    unit_path.write_bytes(unit_bytes); unit_path.chmod(0o644)
    # This existing, protected override is frozen in the review, verified before
    # start and kept unchanged by rollback. The vendor unit stays byte-identical.
    dropdir = Path('/etc/systemd/system/cyberherd.service.d'); dropdir.mkdir(mode=0o755)
    drop = dropdir / '90-ci-network.conf'
    drop.write_text('[Service]\nNetworkNamespacePath=' + str(namespace) + '\n'); drop.chmod(0o644)
    # Exercise the inherited Fedora-style policy as well as role overrides.
    common_dir = Path('/etc/systemd/system/service.d')
    common_created = not common_dir.exists()
    common_dir.mkdir(mode=0o755, exist_ok=True)
    common = common_dir / '90-ci-timeout-abort.conf'
    common_text = '[Service]\nTimeoutStopFailureMode=abort\n'
    with common.open('x') as out: out.write(common_text)
    common.chmod(0o644)
    payout = tomllib.loads(Path('/etc/cyberherd-payout/config.toml').read_text())
    components = {'core': {'database': '/var/lib/lightning-goats/lightning-goats.db',
                            'uid': core.pw_uid, 'gid': core.pw_gid, 'unit': 'lightning-goats-receive.service'}}
    for role, component, filename in [('cyberherd', 'membership', 'cyberherd.db'),
                                       ('cyberherd-payout', 'payout', 'payout.db'), ('cyberherd-signer', 'signer', 'signer.db')]:
        identity = pwd.getpwnam(role)
        components[component] = {'database': '/var/lib/' + role + '/' + filename,
                                 'uid': identity.pw_uid, 'gid': identity.pw_gid, 'unit': role + '.service'}
    policy = {'version': 1, 'owner': member_config['project_pubkey'], 'epoch': member_config['core']['epoch'],
              'provider': payout['policy']['scope']['provider'], 'max_component_bytes': 128*1024*1024, 'components': components}
    policy_path = Path('/etc/cyberherd/state-maintenance.toml')
    with policy_path.open('x') as output: output.write(w.toml_text(policy))
    policy_path.chmod(0o600)
    run('/usr/bin/install', '-d', '-o', 'root', '-g', 'root', '-m', '0755', '/usr/local/libexec')
    checker_dir = Path('/usr/local/libexec/lg-ci-overlay-check'); checker_dir.mkdir(mode=0o755)
    checker = checker_dir / 'CHECK-ONLY-overlay-stage'
    shutil.copyfile(fixture_directory / checker.name, checker); checker.chmod(0o755)
    checker_sha = hashlib.sha256(checker.read_bytes()).hexdigest()
    base = Path('/var/lib/lightning-goats-upgrades')
    review, execution = base / 'ci-overlay-review', base / 'ci-overlay-execution'
    try:
        run('/usr/bin/systemctl', 'daemon-reload'); run('/usr/bin/systemctl', 'start', 'lightning-goats-receive.service')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:8787/healthz'), 'actual maintenance core')
        before_epoch = member_config['core']['epoch']
        for name in ('lightning-goats-receive.service',):
            pid = run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip()
            assert os.readlink('/proc/' + pid + '/ns/net') == os.readlink(namespace)
        prepared = json.loads(run(*launcher, '--cyberherd-overlay-stage', 'prepare', str(installation), str(checker), str(review),
                                  '--validator-sha256', checker_sha, '--message-start-after', '0'))
        assert prepared['prepared'] is True and prepared['services_changed'] is False
        review_sha = hashlib.sha256((review / 'PREPARED.json').read_bytes()).hexdigest()
        planned = json.loads(run(*launcher, '--cyberherd-overlay-maintenance', 'plan', str(review), str(execution), '--review-sha256', review_sha))
        assert planned['planned'] is True and planned['services_changed'] is False
        applied = json.loads(run(*launcher, '--cyberherd-overlay-maintenance', 'apply', str(execution), '--exclusive-maintenance'))
        assert applied['four_journal_backup_verified'] is True and applied['services_started'] is False
        original = json.loads((execution / 'backup/MANIFEST.json').read_text())
        assert original['policy']['epoch'] == before_epoch and original['report']['pending_feeds'] == 1
        assert original['report']['rows']['core']['settled_payments'] == 1
        resumed = json.loads(run(*launcher, '--cyberherd-overlay-maintenance', 'resume', str(execution), '--approve-resumption'))
        assert resumed == {'core_running': True, 'membership_running': True, 'payouts_enabled': False}
        page = w.http_json('http://127.0.0.1:3340/api/v1/cyberherd')
        assert page['roster']['owner'] == policy['owner'] and page['roster']['total_members'] == 0
        for name in ('lightning-goats-receive.service', 'cyberherd.service'):
            pid = run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip()
            assert os.readlink('/proc/' + pid + '/ns/net') == os.readlink(namespace)
        process_before = {name: run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip()
                          for name in ('lightning-goats-receive.service', 'cyberherd.service')}
        # Preparation only, using actual protected binaries/configurations and
        # both authenticated build archives. It must preserve both live PIDs.
        contact = json.loads(run(*launcher, '--contact-upgrade-prepare', str(core_archive), str(review_archive),
                                 source_commit, hashlib.sha256(core_archive.read_bytes()).hexdigest(),
                                 hashlib.sha256(review_archive.read_bytes()).hexdigest(), str(base / 'ci-contact-upgrade-prepare')))
        assert contact == {'prepared': True, 'activation_supported': False, 'services_changed': False,
                           'journals_opened': False, 'credentials_loaded': False, 'contact_enabled': False}
        for name, pid in process_before.items():
            assert run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip() == pid
        assert w.http_json('http://127.0.0.1:8787/healthz')['status'] == 'ok'
        rolled = json.loads(run(*launcher, '--cyberherd-overlay-maintenance', 'rollback', str(execution), '--exclusive-maintenance'))
        assert rolled['database_restored'] is False and rolled['writers_masked'] is True
        assert config_path.read_bytes() == config_bytes and drop.read_text().endswith(str(namespace) + '\n')
        final = base / 'ci-overlay-after-rollback'
        run('/usr/local/bin/cyberherd-statectl', '--config', str(policy_path), 'snapshot', '--destination', str(final), '--writers-stopped')
        run('/usr/local/bin/cyberherd-statectl', '--config', str(policy_path), 'verify', '--bundle', str(final))
        after = json.loads((final / 'MANIFEST.json').read_text())
        assert after['policy'] == original['policy']
        assert after['report']['pending_feeds'] == 1
        for table in ('settled_payments', 'ledger_entries', 'feed_attempts', 'message_outbox'):
            assert after['report']['rows']['core'][table] == original['report']['rows']['core'][table]
        restored = json.loads(run(*launcher, '--cyberherd-overlay-maintenance', 'resume-core', str(execution), '--approve-resumption'))
        assert restored == {'core_running': True, 'membership_running': False, 'payouts_enabled': False}
        assert unit_path.read_bytes() == unit_bytes and config_path.read_bytes() == config_bytes
        assert counts['post'] == 0
        return {'accepted': True, 'actual_systemd_apply_resume_rollback': True, 'actual_native_backup_verified': True,
                'native_core_and_membership_started': True, 'same_epoch_and_paid_pending_signed_counts': True,
                'contact_upgrade_preparation_preserved_running_processes': True,
                'all_application_networking_loopback_isolated': True, 'gateway_post_count': 0,
                'synthetic_credentials_only': True, 'provider_qualified': False, 'live_host_qualified': False}
    except Exception:
        # Only synthetic state/credentials exist in this guarded disposable VM.
        # Never add this diagnostic to a production tool or read live journals.
        import sys
        for name in ('lightning-goats-receive.service', 'cyberherd.service'):
            result = subprocess.run(['/usr/bin/journalctl', '--no-pager', '-u', name, '-n', '30', '-o', 'cat'],
                                    capture_output=True, text=True, timeout=10)
            print('DISPOSABLE SYNTHETIC UNIT ' + name + '\n' + result.stdout[-8000:], file=sys.stderr)
        raise
    finally:
        run('/usr/bin/systemctl', 'stop', 'cyberherd.service', 'lightning-goats-receive.service')
        assert common.read_text() == common_text
        common.unlink()
        if common_created: common_dir.rmdir()
        run('/usr/bin/systemctl', 'daemon-reload')
        gateway.shutdown(); gateway.server_close(); thread.join(timeout=2)


def contact_upgrade_maintenance(launcher, root, fixture_directory, core_archive, review_archive, source_commit):
    """Actual 23->24 executor on disposable synthetic state, never a live host.

    The old core is a separately compiled harmless ELF stand-in. Both migration
    rounds, native backup/comparison, UID execution and the resumed new core are
    actual installed programs. All service networking stays in this fresh netns.
    """
    import http.server
    import threading
    if (os.geteuid() != 0 or os.environ.get('GITHUB_ACTIONS') != 'true'
            or not os.environ.get('GITHUB_RUN_ID', '').isdigit()
            or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net')
            or {v['ifname'] for v in json.loads(run('/usr/sbin/ip', '-j', 'link', 'show'))} != {'lo'}):
        raise ValueError('contact rehearsal requires the guarded disposable isolated CI host')
    tools = root / 'tools'
    spec = importlib.util.spec_from_file_location('ci_contact_worker_support', tools / 'qualify-cyberherd-workers.py')
    w = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(w)
    base = Path('/var/lib/lightning-goats-upgrades')
    overlay = base / 'ci-overlay-review'
    config = Path('/etc/lightning-goats/config.receive.toml')
    member = Path('/etc/cyberherd/config.toml')
    unit = Path('/etc/systemd/system/lightning-goats-receive.service')
    member_mask = Path('/etc/systemd/system/cyberherd.service')
    program = Path('/usr/local/bin/lightning-goatsd')
    original = {str(path): path.read_bytes() for path in (config, member, unit)}
    for name in ('lightning-goats-receive', 'cyberherd', 'cyberherd-payout', 'cyberherd-signer'):
        assert run('/usr/bin/systemctl', 'show', name + '.service', '-p', 'MainPID', '--value').strip() == '0'
    guard = Path('/run/lightning-goats-contact-fixture')
    guard.mkdir(mode=0o755)
    (guard / 'ALLOW').write_text('LIGHTNING_GOATS_DISPOSABLE_CONTACT_UPGRADE_CI_v1\n' + os.readlink('/proc/self/ns/net') + '\n')
    (guard / 'ALLOW').chmod(0o444)
    fixture = root / 'TEST-ONLY-contact-upgrade-fixture'
    shutil.copyfile(fixture_directory / fixture.name, fixture)
    fixture.chmod(0o755)
    runtime = pwd.getpwnam('lightning-goats')
    counts = {'post': 0}
    gateway_safety = {'override_enabled': True, 'remote_enabled': False}
    class Gateway(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_GET(self):
            value = dict(gateway_safety) if self.path == '/v1/feeder/override' else {}
            data = json.dumps(value).encode()
            self.send_response(200 if self.path in ('/v1/feeder/override', '/healthz') else 404)
            self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data)))
            self.end_headers(); self.wfile.write(data)
        def do_POST(self):
            counts['post'] += 1
            self.send_response(405); self.end_headers()
    gateway = http.server.ThreadingHTTPServer(('127.0.0.1', 18789), Gateway)
    thread = threading.Thread(target=gateway.serve_forever, daemon=True)
    thread.start()
    finished = False
    try:
        stripped = json.loads(run('/usr/bin/setpriv', '--reuid=' + str(runtime.pw_uid), '--regid=' + str(runtime.pw_gid),
                                  '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
                                  str(fixture), '--strip-empty-contact-schema'))
        assert stripped == {'synthetic': True, 'schema': 23, 'payments': 1, 'signed_messages': 1, 'production_supported': False}
        config.write_bytes((overlay / 'core.toml').read_bytes())
        member.write_bytes((overlay / 'membership.toml').read_bytes())
        unit.write_bytes((overlay / unit.name).read_bytes())
        shutil.copyfile(fixture, program)
        program.chmod(0o755)
        assert member_mask.is_symlink() and os.readlink(member_mask) == '/dev/null'
        member_mask.unlink()
        run('/usr/bin/systemctl', 'daemon-reload')
        run('/usr/bin/systemctl', 'start', 'lightning-goats-receive.service', 'cyberherd.service')
        old_digest = hashlib.sha256(program.read_bytes()).hexdigest()
        def old_core_ready():
            pid = run('/usr/bin/systemctl', 'show', 'lightning-goats-receive.service', '-p', 'MainPID', '--value').strip()
            if not pid.isdecimal() or int(pid) < 2: return False
            return hashlib.sha256(Path('/proc', pid, 'exe').read_bytes()).hexdigest() == old_digest
        w.await_ready(old_core_ready, 'harmless old-core ELF stand-in')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd'), 'native pre-upgrade membership view')
        review, execution = base / 'ci-contact-maintenance-review', base / 'ci-contact-maintenance-execution'
        prepared = json.loads(run(*launcher, '--contact-upgrade-prepare', str(core_archive), str(review_archive),
                                  source_commit, hashlib.sha256(core_archive.read_bytes()).hexdigest(),
                                  hashlib.sha256(review_archive.read_bytes()).hexdigest(), str(review)))
        assert prepared['services_changed'] is False and prepared['journals_opened'] is False
        frozen = json.loads((review / 'PREPARED.json').read_text())
        before_policy = frozen['baseline']['policy']
        review_sha = hashlib.sha256((review / 'PREPARED.json').read_bytes()).hexdigest()
        planned = json.loads(run(*launcher, '--contact-upgrade-maintenance', 'plan', str(review), str(execution),
                                 '--review-sha256', review_sha))
        assert planned == {'planned': True, 'services_changed': False, 'journals_opened': False}
        upgraded = json.loads(run(*launcher, '--contact-upgrade-maintenance', 'apply', str(execution), '--exclusive-maintenance'))
        assert upgraded == {'upgraded': True, 'four_journal_backup_verified': True, 'clone_compared': True,
                            'live_migration_compared': True, 'services_started': False, 'contact_enabled': False}
        before = json.loads((execution / 'backup/MANIFEST.json').read_text())
        after = json.loads((execution / 'migrated/MANIFEST.json').read_text())
        assert before['policy'] == after['policy'] == before_policy
        assert before['report']['pending_feeds'] == after['report']['pending_feeds'] == 1
        for table in ('settled_payments', 'ledger_entries', 'feed_attempts', 'message_outbox'):
            assert before['report']['rows']['core'][table] == after['report']['rows']['core'][table]
        for table in ('private_contact_intents', 'private_contact_fees', 'private_contact_delivery'):
            assert table not in before['report']['rows']['core'] and after['report']['rows']['core'][table] == 0
        assert hashlib.sha256(program.read_bytes()).hexdigest() != old_digest
        resumed = json.loads(run(*launcher, '--contact-upgrade-maintenance', 'resume', str(execution), '--approve-resumption'))
        assert resumed == {'core_running': True, 'membership_running': True, 'contact_enabled': False, 'payouts_enabled': False}
        assert w.http_json('http://127.0.0.1:8787/healthz')['status'] == 'ok'
        page = w.http_json('http://127.0.0.1:3340/api/v1/cyberherd')
        assert page['roster']['owner'] == before_policy['owner']
        for name in ('lightning-goats-receive.service', 'cyberherd.service'):
            pid = run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip()
            assert os.readlink('/proc/' + pid + '/ns/net') == os.readlink('/proc/self/ns/net')
        assert counts['post'] == 0 and 'contact' not in tomllib.loads(config.read_text())
        # Separate contact-only activation, still on this guarded disposable VM.
        # Active mode is established as synthetic fixture setup, BEFORE freezing
        # the activation review. No production mode/signing authority is inferred.
        run('/usr/bin/systemctl', 'stop', 'cyberherd.service', 'lightning-goats-receive.service')
        active = tomllib.loads(config.read_text())
        active['service']['mode'] = 'active'
        # This path deliberately does not exist: the contact activation check
        # must not need a signing subprocess, bunker or public Nostr operation.
        active['nostr']['nak_path'] = '/usr/local/bin/TEST-ONLY-no-contact-signer'
        assert not Path(active['nostr']['nak_path']).exists()
        config.write_text(w.toml_text(active))
        credential = root / 'nostr-client-key.contact-synthetic.cred'
        w.encrypt(credential, 'nostr-client-key', b'SYNTHETIC-CONTACT-NO-BUNKER')
        with unit.open('a') as output:
            output.write('LoadCredentialEncrypted=nostr-client-key:' + str(credential) + '\n')
        run('/usr/bin/systemctl', 'daemon-reload')
        run('/usr/bin/systemctl', 'start', 'lightning-goats-receive.service', 'cyberherd.service')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:8787/healthz'), 'synthetic active core before contact activation')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd'), 'membership before contact activation')
        before_config = tomllib.loads(config.read_text())
        validator = root / 'CHECK-ONLY-private-contact-stage'
        shutil.copyfile(fixture_directory / validator.name, validator)
        validator.chmod(0o755)
        activation = base / 'ci-private-contact-activation'
        prepared_contact = json.loads(run(*launcher, '--private-contact-activation', 'prepare', str(activation),
                                           '--validator', str(validator), '--validator-sha256', hashlib.sha256(validator.read_bytes()).hexdigest(),
                                           '--inbox-relay', 'wss://relay.damus.io/'))
        assert prepared_contact == {'prepared': True, 'services_changed': False, 'journals_opened': False, 'contact_enabled': False}
        activation_sha = hashlib.sha256((activation / 'PREPARED.json').read_bytes()).hexdigest()
        applied_contact = json.loads(run(*launcher, '--private-contact-activation', 'apply', str(activation),
                                          '--review-sha256', activation_sha, '--exclusive-maintenance'))
        assert applied_contact == {'configured': True, 'current_four_journal_backup_verified': True,
                                   'financial_images_unchanged': True, 'services_started': False}
        activated_contact = json.loads(run(*launcher, '--private-contact-activation', 'resume', str(activation), '--approve-resumption'))
        assert activated_contact == {'core_running': True, 'membership_running': True, 'contact_enabled': True, 'payouts_enabled': False}
        import urllib.error
        import urllib.request
        request = urllib.request.Request('http://127.0.0.1:8787/api/v1/contact', data=b'{}', method='POST',
                                         headers={'Content-Type': 'application/json', 'Origin': active['lnurl']['public_base_url'].rstrip('/')})
        try:
            urllib.request.urlopen(request, timeout=10)
            raise AssertionError('invalid contact must never request an invoice')
        except urllib.error.HTTPError as error:
            assert error.code == 400
            assert json.loads(error.read()) == {'error': 'invalid private contact'}
        after_config = tomllib.loads(config.read_text())
        assert after_config.pop('contact') == {'recipient': before_policy['owner'], 'inbox_relays': ['wss://relay.damus.io/']}
        assert after_config == before_config and counts['post'] == 0
        for name in ('lightning-goats-receive.service', 'cyberherd.service'):
            pid = run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip()
            assert os.readlink('/proc/' + pid + '/ns/net') == os.readlink('/proc/self/ns/net')
        for name in ('cyberherd-payout.service', 'cyberherd-signer.service'):
            assert os.readlink('/etc/systemd/system/' + name) == '/dev/null'
            assert run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip() == '0'
        # Actual native signed operator HTTP and replay across a member restart.
        # This remains disposable synthetic state in the attested loopback netns.
        # No production configuration, real operator key or financial gate opens.
        run('/usr/bin/systemctl', 'stop', 'cyberherd.service')
        operator_config = tomllib.loads(member.read_text())
        assert 'auth' not in operator_config['view']
        operator_config['view']['auth'] = {'public_origin': 'https://example.com',
            'operators': ['c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5']}
        member.write_text(w.toml_text(operator_config))
        operator = root / 'TEST-ONLY-installed-operator-client'
        shutil.copyfile(fixture_directory / operator.name, operator)
        operator.chmod(0o755)
        member_user = pwd.getpwnam('cyberherd')
        client_state = guard / 'operator-client'
        client_state.mkdir(mode=0o700)
        os.chown(client_state, member_user.pw_uid, member_user.pw_gid)
        client = ['/usr/bin/setpriv', '--reuid=' + str(member_user.pw_uid), '--regid=' + str(member_user.pw_gid),
                  '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all',
                  '--no-new-privs', str(operator)]
        run('/usr/bin/systemctl', 'start', 'cyberherd.service')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd'), 'operator-enabled native member')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:8787/api/v1/status')['feeder_override_active'] is True,
                      'confirmed synthetic override before operator tests')
        original_operator_pid = run('/usr/bin/systemctl', 'show', 'cyberherd.service', '-p', 'MainPID', '--value').strip()
        exercised_operator = json.loads(run(*client, 'exercise'))
        assert exercised_operator == {'exercise_accepted': True, 'real_native_http': True, 'synthetic_keys_only': True,
            'settings_and_template_applied': True, 'guest_origin_body_and_pause_refusals': True,
            'same_command_receipt_retained': True}
        run('/usr/bin/systemctl', 'stop', 'cyberherd.service')
        run('/usr/bin/systemctl', 'start', 'cyberherd.service')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd'), 'restarted operator-enabled native member')
        restarted_operator_pid = run('/usr/bin/systemctl', 'show', 'cyberherd.service', '-p', 'MainPID', '--value').strip()
        assert original_operator_pid != restarted_operator_pid
        replayed_operator = json.loads(run(*client, 'after-restart'))
        assert replayed_operator == {'restart_accepted': True, 'actual_unexpired_nonce_replay_refused': True,
            'settings_and_template_retained': True, 'same_command_receipt_retained': True,
            'changed_command_conflict_refused': True, 'roster_empty': True, 'member_percent_zero': True,
            'provider_operations': 0, 'publications': 0}
        assert counts['post'] == 0
        # Program-only maintenance of the now-current schema, including active
        # contact and retained native operator receipts. Both installed and
        # selected verifiers must accept the same actual four-journal backup
        # before old/new status commands touch only a separate UID-owned copy.
        # Programs have the same build bytes in this CI phase; actual different
        # program replacement is covered separately by protected-file fixtures.
        current_review = base / 'ci-current-core-review'
        current_execution = base / 'ci-current-core-execution'
        preserved = {path: hashlib.sha256(path.read_bytes()).hexdigest()
                     for path in (config, member, unit)}
        current_prepared = json.loads(run(*launcher, '--current-core-prepare', str(core_archive), str(review_archive),
            source_commit, hashlib.sha256(core_archive.read_bytes()).hexdigest(),
            hashlib.sha256(review_archive.read_bytes()).hexdigest(), str(current_review)))
        assert current_prepared == {'prepared': True, 'activation_supported': False, 'services_changed': False,
            'journals_opened': False, 'credentials_loaded': False, 'contact_enabled': True}
        current_sha = hashlib.sha256((current_review / 'PREPARED.json').read_bytes()).hexdigest()
        assert json.loads(run(*launcher, '--current-core-maintenance', 'plan', str(current_review), str(current_execution),
            '--review-sha256', current_sha)) == {'planned': True, 'services_changed': False, 'journals_opened': False}
        current_applied = json.loads(run(*launcher, '--current-core-maintenance', 'apply', str(current_execution),
            '--exclusive-maintenance'))
        assert current_applied == {'upgraded': True, 'four_journal_backup_verified': True, 'clone_compared': True,
            'live_migration_performed': False, 'financial_images_unchanged': True, 'services_started': False,
            'contact_enabled': True}
        assert not (current_execution / 'MIGRATION-STARTED.json').exists()
        assert not (current_execution / 'migrated').exists()
        current_backup = json.loads((current_execution / 'backup/MANIFEST.json').read_text())
        assert current_backup['policy'] == before_policy
        assert current_backup['report']['pending_feeds'] == 1
        assert current_backup['report']['rows']['core']['private_contact_intents'] == 0
        current_resumed = json.loads(run(*launcher, '--current-core-maintenance', 'resume', str(current_execution),
            '--approve-resumption'))
        assert current_resumed == {'core_running': True, 'membership_running': True,
            'contact_enabled': True, 'payouts_enabled': False}
        assert preserved == {path: hashlib.sha256(path.read_bytes()).hexdigest() for path in preserved}
        assert counts['post'] == 0
        assert w.http_json('http://127.0.0.1:8787/healthz')['status'] == 'ok'
        assert w.http_json('http://127.0.0.1:3340/api/v1/cyberherd')['roster']['members'] == []
        for name in ('cyberherd-payout.service', 'cyberherd-signer.service'):
            assert os.readlink('/etc/systemd/system/' + name) == '/dev/null'
            assert run('/usr/bin/systemctl', 'show', name, '-p', 'MainPID', '--value').strip() == '0'
        # Only this namespace's harmless gateway changes state. The retained
        # unknown synthetic feeder UUID must keep the real core GET-only even
        # when operator membership safety reads become ready. No physical or
        # provider authority exists in this fixture.
        financial_before = w.http_json('http://127.0.0.1:8787/api/v1/status')
        assert isinstance(financial_before['unresolved_feed_attempt'], str)
        gateway_safety.update(override_enabled=False, remote_enabled=True)
        w.await_ready(lambda: w.http_json('http://127.0.0.1:8787/api/v1/status')['feeder_override_active'] is False,
                      'synthetic gateway ready before membership commands')
        ready_commands = json.loads(run(*client, 'ready'))
        assert ready_commands == {'ready_commands_accepted': True, 'activation_and_score_applied': True,
            'stale_revision_refused': True, 'explicit_unban_required': True,
            'manual_score_lifetime_zero': True, 'member_percent_zero': True}
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd')['roster']['total_members'] == 1,
                      'actual public roster after operator activation')
        roster = w.http_json('http://127.0.0.1:3340/api/v1/cyberherd')['roster']
        assert roster['members'][0]['amount_sats'] == '21' and roster['members'][0]['points'] == '210'
        assert roster['members'][0]['share_micro_percent'] == 0 and roster['member_percent'] == 0
        before_ready_restart = run('/usr/bin/systemctl', 'show', 'cyberherd.service', '-p', 'MainPID', '--value').strip()
        run('/usr/bin/systemctl', 'stop', 'cyberherd.service')
        run('/usr/bin/systemctl', 'start', 'cyberherd.service')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd'), 'restarted active synthetic member')
        assert run('/usr/bin/systemctl', 'show', 'cyberherd.service', '-p', 'MainPID', '--value').strip() != before_ready_restart
        ready_restarted = json.loads(run(*client, 'after-ready-restart'))
        assert ready_restarted == {'ready_restart_accepted': True, 'active_score_and_cycle_retained': True,
            'original_command_receipt_retained': True, 'manual_score_lifetime_zero': True}
        gateway_safety.update(override_enabled=True, remote_enabled=False)
        w.await_ready(lambda: w.http_json('http://127.0.0.1:8787/api/v1/status')['feeder_override_active'] is True,
                      'synthetic gateway paused before controls and reset')
        paused_commands = json.loads(run(*client, 'paused-controls'))
        assert paused_commands == {'paused_controls_accepted': True, 'new_score_refused_while_paused': True,
            'original_score_receipt_retrievable': True, 'remove_ban_unban_work_while_paused': True,
            'manual_reset_preserves_ban': True, 'settings_and_template_retained': True}
        before_controls_restart = run('/usr/bin/systemctl', 'show', 'cyberherd.service', '-p', 'MainPID', '--value').strip()
        run('/usr/bin/systemctl', 'stop', 'cyberherd.service')
        run('/usr/bin/systemctl', 'start', 'cyberherd.service')
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd'), 'restarted reset synthetic member')
        assert run('/usr/bin/systemctl', 'show', 'cyberherd.service', '-p', 'MainPID', '--value').strip() != before_controls_restart
        controls_restarted = json.loads(run(*client, 'after-controls-restart'))
        assert controls_restarted == {'controls_restart_accepted': True, 'reset_and_revision_retained': True,
            'cross_cycle_receipts_retained_without_mutation': True, 'final_member_inactive': True,
            'manual_score_lifetime_zero': True}
        w.await_ready(lambda: w.http_json('http://127.0.0.1:3340/api/v1/cyberherd')['roster']['total_members'] == 0,
                      'actual empty public roster after manual reset')
        financial_after = w.http_json('http://127.0.0.1:8787/api/v1/status')
        for key in ('feed_credit_sats', 'threshold_sats', 'feeds_due', 'remainder_sats', 'unresolved_feed_attempt'):
            assert financial_after[key] == financial_before[key]
        assert counts['post'] == 0
        finished = True
        return {'accepted': True, 'old_core_was_harmless_elf_standin': True,
                'actual_native_schema_23_backup': True, 'actual_runtime_uid_clone_migration': True,
                'actual_native_four_journal_comparisons': True, 'actual_live_schema_24_migration': True,
                'actual_new_native_core_and_membership_resumed': True, 'all_original_paid_and_signed_rows_preserved': True,
                'epoch_and_financial_policy_preserved': True, 'gateway_post_count': counts['post'],
                'contact_enabled': False, 'synthetic_credentials_only': True,
                'installed_private_contact_activation': {'accepted': True, 'actual_native_config_checker': True,
                    'actual_current_four_journal_backup_verified': True, 'financial_images_unchanged_during_apply': True,
                    'only_contact_configuration_added': True, 'actual_active_core_and_membership_resumed': True,
                    'invalid_contact_refused_before_provider': True, 'outgoing_roles_masked': True,
                    'gateway_post_count': counts['post'], 'signing_qualified': False, 'paid_delivery_qualified': False},
                'installed_operator_http': {'accepted': True, 'exercise': exercised_operator,
                    'after_actual_member_restart': replayed_operator, 'member_pid_changed': True,
                    'operator_identity_was_synthetic': True, 'public_network_used': False,
                    'gateway_post_count': counts['post'], 'production_operator_routes_enabled': False},
                'installed_current_schema_patch': {'accepted': True, 'actual_four_journal_native_backup': True,
                    'both_compiled_schema_verifiers_accepted': True, 'actual_uid_copy_status_compared': True,
                    'live_database_images_unchanged': True, 'live_migration_performed': False,
                    'actual_core_and_member_resumed': True, 'contact_configuration_retained': True,
                    'outgoing_roles_masked': True, 'gateway_post_count': counts['post'],
                    'different_program_builds_used': False, 'live_host_qualified': False},
                'installed_operator_membership_controls': {'accepted': True, 'ready': ready_commands,
                    'after_ready_member_restart': ready_restarted, 'paused': paused_commands,
                    'after_reset_member_restart': controls_restarted,
                    'actual_public_score_and_zero_share': True, 'final_public_roster_empty': True,
                    'core_credit_and_original_unknown_uuid_unchanged': True, 'gateway_post_count': counts['post'],
                    'provider_operations': 0, 'public_network_used': False, 'live_host_qualified': False},
                'all_application_networking_loopback_isolated': True, 'live_host_qualified': False}
    except Exception:
        # Disposable synthetic fixture only. Never use this diagnostic or the
        # fixture's schema transform against a production host or journal.
        import sys
        for name in ('lightning-goats-receive.service', 'cyberherd.service'):
            result = subprocess.run(['/usr/bin/journalctl', '--no-pager', '-u', name, '-n', '25', '-o', 'cat'],
                                    capture_output=True, text=True, timeout=10)
            print('DISPOSABLE CONTACT FIXTURE ' + name + '\n' + result.stdout[-8000:], file=sys.stderr)
        raise
    finally:
        run('/usr/bin/systemctl', 'stop', 'cyberherd.service', 'lightning-goats-receive.service')
        if not member_mask.exists() and not member_mask.is_symlink(): member_mask.symlink_to('/dev/null')
        assert member_mask.is_symlink() and os.readlink(member_mask) == '/dev/null'
        if finished:
            # Configuration-only fixture cleanup. Never roll back database bytes
            # or the compatible new native programs after schema migration.
            for name, data in original.items(): Path(name).write_bytes(data)
        run('/usr/bin/systemctl', 'daemon-reload')
        # Synthetic client evidence is retained privately; never remove a live
        # auth nonce or command record to regain acceptance after a restart.
        if (guard / 'operator-client').exists():
            (guard / 'operator-client').rename(root / 'operator-client-evidence')
        (guard / 'ALLOW').unlink(); guard.rmdir()
        gateway.shutdown(); gateway.server_close(); thread.join(timeout=2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('review', type=Path)
    parser.add_argument('source')
    parser.add_argument('fixture_directory', type=Path)
    parser.add_argument('--core-archive', type=Path, required=True)
    parser.add_argument('--disposable-ci-host', action='store_true', required=True)
    args = parser.parse_args()
    if (os.geteuid() != 0 or not args.disposable_ci_host or not Path('/run/systemd/system').is_dir()
            or os.environ.get('GITHUB_ACTIONS') != 'true' or not os.environ.get('GITHUB_RUN_ID', '').isdigit()):
        raise ValueError('disposable root systemd CI VM required')
    # Require the private loopback-only namespace used by the other rehearsal.
    # The inherited sysfs mount can still describe the host namespace. Query
    # netlink in this process's namespace instead; do not weaken isolation.
    interfaces = {link['ifname'] for link in json.loads(run('/usr/sbin/ip', '-j', 'link', 'show'))}
    if interfaces != {'lo'} or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('fresh isolated network namespace required')
    core = pwd.getpwnam('lightning-goats')  # Earlier fresh-canary fixture, no activation.
    if core.pw_uid == 0 or core.pw_gid == 0:
        raise ValueError('separate existing core fixture identity required')
    tool_export = json.loads((args.fixture_directory / 'TOOL-EXPORT.json').read_text())
    if tool_export['source_commit'] != args.source:
        raise ValueError('test tool source mismatch')
    fixture = args.fixture_directory / 'TEST-ONLY-prepare-legacy-core'
    source_tools = args.fixture_directory / 'tool-bundle'
    if hashlib.sha256((source_tools / 'launch-vps-upgrade.py').read_bytes()).hexdigest() != tool_export['launcher_sha256']:
        raise ValueError('test launcher hash differs from pinned export')
    root = Path(tempfile.mkdtemp(prefix='lg-cyberherd-install-rehearsal-', dir='/run'))
    os.chown(root, 0, 0)
    # The authenticated tool closure requires root:root ancestors. Only its
    # synthetic fixture binary/configs need traversal by the non-root core UID;
    # tool, backup, key and source children retain their own private modes.
    root.chmod(0o755)
    tools = root / 'tools'
    shutil.copytree(source_tools, tools)
    tools.chmod(0o700)
    for path in tools.iterdir():
        os.chown(path, 0, 0)
        path.chmod(0o600)
    launcher = ['/usr/bin/python3', '-I', '-S', '-B', str(tools / 'launch-vps-upgrade.py'), tool_export['manifest_sha256']]
    # The CI fixture is authenticated by the pinned exported tool closure, not
    # by executing a helper from the review archive.
    sysusers = json.loads(run(*launcher, '--cyberherd-inactive', 'identities', '--create-identities'))
    assert sysusers['services_started'] is False
    runtime = sysusers['runtime']
    source = root / 'source'
    source.mkdir(mode=0o700)
    os.chown(source, core.pw_uid, core.pw_gid)
    installed_fixture = root / 'TEST-ONLY-prepare-legacy-core'
    shutil.copyfile(fixture, installed_fixture)
    installed_fixture.chmod(0o755)
    prepared = json.loads(run('/usr/bin/setpriv', '--reuid=' + str(core.pw_uid), '--regid=' + str(core.pw_gid),
                              '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
                              str(installed_fixture), str(source / 'core.db'), '--paid-fixture'))
    assert prepared['synthetic'] is True and prepared['payments'] == 1 and prepared['engine'] == '3.54.1'
    policy, bundle = core_upgrade(launcher, root, source, core, args.core_archive, args.source)
    assert json.loads((bundle / 'MANIFEST.json').read_text())['report']['epoch'] == prepared['epoch']
    # Read only the authenticated examples; no Python from the archive executes.
    spec = importlib.util.spec_from_file_location('test_package', tools / 'verify-cyberherd-package.py')
    package = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(package)
    package.verify(args.review, args.source)
    examples = {}
    with tarfile.open(args.review, 'r:gz') as archive:
        for member in archive:
            name = str(Path(member.name))
            if name in {f'deploy/cyberherd/{kind}.toml.example' for kind in ('config', 'payout', 'signer')}:
                examples[name] = archive.extractfile(member).read().decode()
    owner = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
    client = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
    configs = root / 'configs'
    configs.mkdir(mode=0o755)
    configs.chmod(0o755)
    synthetic_key = root / 'TEST-ONLY-payout-key'
    synthetic_key.write_bytes(b'test-only-no-installed-spend-key')
    synthetic_key.chmod(0o600)
    store = Path('/etc/credstore.encrypted')
    if not store.exists():
        store.mkdir(mode=0o700)
    imported = json.loads(run(*launcher, '--cyberherd-payout-key', '--key-file', str(synthetic_key), '--import-key'))
    assert imported['created'] is True and imported['permissions_checked'] is False and imported['payments_sent'] == 0
    assert imported['credential_fingerprint'] == hashlib.sha256(synthetic_key.read_bytes()).hexdigest()
    import grp
    for role, kind in [('cyberherd', 'config'), ('cyberherd-payout', 'payout'), ('cyberherd-signer', 'signer')]:
        text = examples[f'deploy/cyberherd/{kind}.toml.example']
        text = text.replace('credential_fingerprint = "' + '0' * 64 + '"',
                            'credential_fingerprint = "' + imported['credential_fingerprint'] + '"')
        text = text.replace('account_id = "00000000-0000-0000-0000-000000000000"',
                            'account_id = "00000000-0000-0000-0000-000000000001"')
        text = text.replace('0' * 64, owner).replace('REPLACE_WITH_CLIENT_PUBLIC_KEY', client)
        text = text.replace('00000000-0000-0000-0000-000000000000', prepared['epoch'])
        text = text.replace('core_uid = 0', 'core_uid = ' + str(core.pw_uid))
        text = text.replace('intake_uid = 0', 'intake_uid = ' + str(runtime['cyberherd']['uid']))
        text = text.replace('signer_uid = 0', 'signer_uid = ' + str(runtime['cyberherd-signer']['uid']))
        if role == 'cyberherd':
            old = 'herd = "' + owner + '"'
            assert text.count(old) == 1 and text.count('\nstart_after = 0\n') == 1
            text = text.replace(old, '\n'.join(user + ' = "' + owner + '"' for user in ('herd', 'dexter', 'rowan', 'cosmo', 'newton', 'nova')))
            # This disposable fixture has exactly one historical malformed event.
            # Bind the fresh intake after it before initialization, never reset it.
            text = text.replace('\nstart_after = 0\n', '\nstart_after = 1\n')
        if role == 'cyberherd-signer':
            text = text.replace('peer_uid = 0', 'peer_uid = ' + str(runtime['cyberherd-payout']['uid']))
            text = text.replace('group_gid = 0', 'group_gid = ' + str(grp.getgrnam('cyberherd-payout-signer').gr_gid))
        path = configs / (role + '.toml')
        path.write_text(text)
        path.chmod(0o644)
    base = Path('/var/lib/lightning-goats-upgrades')
    destination = base / 'ci-inactive-install'
    receipt = json.loads(run(*launcher, '--cyberherd-inactive', 'install', str(args.review), str(configs), str(policy), str(bundle), str(destination),
                             '--source', args.source, '--sha256', hashlib.sha256(args.review.read_bytes()).hexdigest(), '--install-disabled'))
    assert receipt['services_masked'] is True and receipt['services_started'] is False and receipt['credentials_loaded'] is False
    for role in runtime:
        state = Path('/var/lib') / role
        assert stat.S_IMODE(state.stat().st_mode) == 0o700 and state.stat().st_uid == runtime[role]['uid']
        assert os.readlink(Path('/etc/systemd/system') / (role + '.service')) == '/dev/null'
        assert (destination / 'INSTALLED.json').exists() and not (destination / 'INCOMPLETE').exists()
    qualification_path = base / 'ci-installed-role-qualification'
    try:
        qualification = json.loads(run(*launcher, '--cyberherd-qualify', '--installation', str(destination),
                                       '--destination', str(qualification_path), '--probe-disabled-roles'))
    except Exception:
        # This harness is guarded to a disposable CI host with synthetic state.
        # Production qualifier diagnostics stay in its private transaction.
        failure = qualification_path / 'FAILURE.json'
        if failure.is_file():
            import sys
            print(failure.read_text()[-18000:], file=sys.stderr)
        raise
    assert qualification['accepted'] is True and qualification['actual_installed_os_policy_probes'] == 3
    assert qualification['application_workers_started'] is False and qualification['real_credentials_loaded'] is False
    workers_path = base / 'ci-installed-worker-qualification'
    try:
        workers = json.loads(run(*launcher, '--cyberherd-workers', '--installation', str(destination),
                                 '--core-transaction', str(base / 'ci-core-upgrade'),
                                 '--destination', str(workers_path), '--create-isolated-fixture'))
    except Exception:
        failure = workers_path / 'FAILURE.json'
        if failure.is_file():
            import sys
            print(failure.read_text()[-24000:], file=sys.stderr)
        raise
    assert workers['accepted'] is True and workers['actual_installed_rust_workers'] == 4
    assert workers['production_workers_started'] is False and workers['real_credentials_loaded'] is False
    maintenance = overlay_maintenance(launcher, root, args.fixture_directory, destination,
                                      args.core_archive, args.review, args.source)
    contact = contact_upgrade_maintenance(launcher, root, args.fixture_directory,
                                          args.core_archive, args.review, args.source)
    print(json.dumps({'synthetic': True, 'source_commit': args.source, 'native_engine': prepared['engine'],
                      'actual_nss_identities': 3, 'actual_native_initializers': 3, 'actual_masked_units': 3,
                      'synthetic_payout_ciphertext_created': True,
                      'installed_role_qualification': qualification,
                      'installed_worker_qualification': workers,
                      'installed_overlay_maintenance': maintenance,
                      'installed_contact_upgrade_maintenance': contact,
                      'actual_core_upgrade_transaction': True, 'harmless_core_fixture_started': True,
                      'payment_daemon_started': True, 'cyberherd_units_started': True,
                      'runtime_credentials_loaded': 'synthetic_only', 'provider_qualified': False,
                      'household_qualified': False, 'live_host_qualified': False}, sort_keys=True))


if __name__ == '__main__':
    main()
