#!/usr/bin/env python3
"""Exercise protected one-acquisition CLI in fresh mount/network namespaces.

Synthetic keys and unfunded retained acquisition only. Native CDK ownership tests
separately qualify the acquired-output transition; this fixture never contacts a
mint and never touches an installed wallet or operator seed.
"""
import argparse
from contextlib import closing
import fcntl
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import runpy
import sqlite3
import subprocess
import tempfile
import time
import uuid


def install_mint_policy(root, config, candidate, binary, state, uid, other_uids):
    """Apply through the real authenticated launcher in this disposable namespace."""
    scripts = Path(__file__).resolve().parent
    subprocess.run(['/usr/bin/mount', '--make-rprivate', '/'], check=True)
    subprocess.run(['/usr/bin/mount', '-t', 'tmpfs', '-o', 'mode=0755', 'tmpfs', '/var/lib'], check=True)
    Path('/var/lib/lightning-goats-upgrades').mkdir(mode=0o700)
    unit = 'lg-cashu-mint-policy-fixture.service'
    status = root/'wallet-unit-status'
    stub = root/'systemctl'
    stub.write_text('#!/bin/sh\n[ "$1" = show ] && [ "$5" = '+unit+' ] || exit 1\n'
                    'cat '+str(status)+'\n')
    stub.chmod(0o555)
    subprocess.run(['/usr/bin/mount', '--bind', str(stub), '/usr/bin/systemctl'], check=True)

    def set_status(active='inactive', enabled='masked'):
        status.write_text(f'Id={unit}\nLoadState=masked\nActiveState={active}\nSubState=dead\n'
                          f'MainPID=0\nControlPID=0\nUnitFileState={enabled}\nControlGroup=\n')

    def digest(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    capsule = root/'mint-maintenance-capsule'
    capsule.mkdir(mode=0o700)
    launcher = scripts/'launch-cashu-backup.py'
    files = runpy.run_path(str(launcher))['FILES']
    for name in files:
        path = capsule/name
        path.write_bytes((scripts/name).read_bytes())
        path.chmod(0o400)
    manifest = capsule/'TOOL.json'
    manifest.write_text(json.dumps(dict(version=1, source_commit='01'*20,
                                        files={name:digest(capsule/name) for name in files})))
    manifest.chmod(0o400)
    baseline = root/'old-config.json'
    baseline.write_bytes(config.read_bytes())
    for file in (baseline,config):
        os.chown(file, 0, uid)
        file.chmod(0o440)
        os.setxattr(file, 'user.lg_test', b'wallet-config')
    candidate.chmod(0o444)

    private = root/'policy'
    private.mkdir(mode=0o700)
    policy = private/'policy.json'
    roles = sorted(('core','membership','payout','signer','cashu_signer'))
    identities = dict(zip(roles, other_uids, strict=True))
    workers = {'wallet':dict(unit=unit,uid=uid,gid=uid,state=str(state))}
    for role, other in identities.items():
        directory = root/role
        directory.mkdir(mode=0o700)
        os.chown(directory, other, other)
        workers[role] = dict(unit=f'lg-mint-fixture-{role}.service',uid=other,gid=other,state=str(directory))
    artifact = dict(path=str(binary),sha256=digest(binary))
    value = dict(version=1, operator_exclusive=True, workers=workers, wallet_config=str(config),
        native_policy='/unused', core_config='/unused', membership_config='/unused', payout_config='/unused',
        cashu_signer_config='/unused', payout_signer_config='/unused', cashu_client_source='/unused',
        payout_client_source='/unused', historical_receiving_keys=[], extra_files=[],
        artifacts={name:artifact for name in ('copy','statectl','age','tar')},
        runtime_artifacts={role:artifact for role in workers},
        recipient='age1'+'q'*58, component_bytes=4096, total_bytes=4096, deadline_seconds=60)

    def save_policy():
        policy.write_text(json.dumps(value))
        policy.chmod(0o600)

    save_policy()
    old, new = digest(baseline), digest(candidate)
    def apply(*, success=True, policy_hash=None, candidate_hash=new):
        result = subprocess.run(['/usr/bin/python3','-I','-S','-B',str(capsule/launcher.name),
            digest(manifest),'apply-mint-policy',str(policy),policy_hash or digest(policy),
            str(baseline),old,str(candidate),candidate_hash], stdin=subprocess.DEVNULL,
            capture_output=True, text=True, timeout=60, env={'PATH':'/usr/bin:/bin','LC_ALL':'C'})
        if (result.returncode == 0) != success:
            raise ValueError('unexpected authenticated apply result: '+result.stderr[-4096:])
        if success:
            return json.loads(result.stdout)
        if digest(config) != old:
            raise ValueError('refused install changed the original config')

    set_status()
    apply(success=False, policy_hash='0'*64)
    apply(success=False, candidate_hash='0'*64)
    value['runtime_artifacts']['wallet'] = dict(artifact, sha256='0'*64)
    save_policy()
    apply(success=False)
    value['runtime_artifacts']['wallet'] = artifact
    save_policy()
    set_status(active='active')
    apply(success=False)
    set_status(enabled='disabled')
    apply(success=False)
    set_status()
    config.chmod(0o444)
    apply(success=False)
    config.chmod(0o440)
    maintenance = Path('/var/lib/lightning-goats-upgrades/CYBERHERD-OVERLAY-MAINTENANCE.lock')
    with maintenance.open('rb') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        apply(success=False)
    with subprocess.Popen(['/usr/bin/setpriv',f'--reuid={uid}',f'--regid={uid}',
            '--clear-groups','--no-new-privs','--','/bin/sh','-c','echo ready; exec /usr/bin/sleep 30'],
            stdout=subprocess.PIPE, text=True) as running:
        try:
            if running.stdout.readline() != 'ready\n':
                raise ValueError('synthetic wallet UID process did not start')
            apply(success=False)
        finally:
            running.terminate()
            running.wait(timeout=5)
    with (state/'wallet-writer.lock').open('rb') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        apply(success=False)
    marker = state/'CASHU_RECOVERY_REQUIRED'
    marker.write_text('synthetic recovery fence')
    apply(success=False)
    marker.unlink()
    answer = apply()
    replay = apply()
    if (answer['result'] != 'CASHU_MINT_POLICY_CONFIG_INSTALLED' or answer['services_activated']
            or answer['already_installed'] or not replay['already_installed']
            or answer['review']['candidate_sha256'] != new
            or digest(config) != new or digest(baseline) != old
            or config.stat().st_uid != 0 or config.stat().st_gid != uid
            or config.stat().st_mode & 0o777 != 0o440
            or os.getxattr(config, 'user.lg_test') != b'wallet-config'):
        raise ValueError('installed bytes, metadata, or exact retry differ from approval')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--cli-sha256', required=True)
    args = parser.parse_args()
    if os.geteuid() != 0 or any(os.readlink('/proc/self/ns/'+kind) == os.readlink('/proc/1/ns/'+kind)
                                  for kind in ['mnt', 'net']):
        raise ValueError('requires root in fresh mount and network namespaces')
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link', 'show']))
    if [link['ifname'] for link in links] != ['lo']:
        raise ValueError('fixture requires loopback-only network namespace')
    data = args.cli.resolve(strict=True).read_bytes()
    if len(data) > 128*1024*1024 or hashlib.sha256(data).hexdigest() != args.cli_sha256:
        raise ValueError('selected wallet CLI differs from reviewed bytes')
    used = {u.pw_uid for u in pwd.getpwall()} | {g.gr_gid for g in grp.getgrall()}
    used |= {int(x) for x in subprocess.check_output(['ps', '-e', '-o', 'uid='], text=True).split()}
    uid, *other_uids = [n for n in range(60000, 65000) if n not in used][:6]
    core_uid = other_uids[0]
    with tempfile.TemporaryDirectory(prefix='lg-ownership-cli-', dir='/run') as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        binary = root/'cashud'
        binary.write_bytes(data)
        binary.chmod(0o555)
        state = root/'wallet'
        state.mkdir(mode=0o700)
        os.chown(state, uid, uid)
        for name, value in [('wallet-seed.bin', bytes([7])*64), ('receiving-key.bin', bytes(31)+b'\x01')]:
            path = state/name
            path.write_bytes(value)
            os.chown(path, uid, uid)
            path.chmod(0o600)
        instance, epoch, acquisition = [str(uuid.uuid4()) for _ in range(3)]
        core_epoch = str(uuid.uuid4())
        config = root/'config.json'
        config_value = dict(version=1, runtime_uid=uid, core_uid=core_uid, ipc_gid=uid,
            wallet_instance=instance, wallet_epoch=epoch, state_directory=str(state),
            intake_socket=str(root/'ipc/intake.sock'),
            receiving_key='0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
            recipient='f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9',
            descriptors=['synthetic-review-id'], receive_cashu=True,
            keysets=[dict(mint_id='mint-a', id='00deadbeef123456', keys={
                '8':'0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'})],
            body_bytes=65536, proof_count=64, receipt_sats=10000,
            custody_limits=dict(maximum_total_sats=300, maximum_per_mint_sats={'mint-a': 200}),
            deadline_seconds=5, connections_per_second=8,
            treasury_intake=dict(socket=str(root/'ipc/treasury.sock'),core_epoch=core_epoch,
                destination=dict(origin='https://strike.example/',account_id=str(uuid.uuid4()),
                    credential_fingerprint='a'*64,network='bitcoin'),
                policies={'mint-a':'treasury-v1'}),
            mints=[dict(id='mint-a',advertised_url='https://mint.example/Bitcoin',policy_version='v1',
                receive_cashu=True,receive_nutzap=False,bolt11_exit_qualified=True)])
        config.write_text(json.dumps(config_value))
        config.chmod(0o444)
        config_bytes = config.read_bytes()
        prefix = ['/usr/bin/setpriv', f'--reuid={uid}', f'--regid={uid}', '--clear-groups',
                  '--no-new-privs', '--', str(binary)]

        def call(*arguments, success=True):
            result = subprocess.run(prefix+list(arguments), env={'PATH':'/usr/bin:/bin','LC_ALL':'C'},
                                    stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
            if (result.returncode == 0) != success:
                raise ValueError('unexpected native wallet CLI result: '+(result.stdout+result.stderr)[-4096:])
            if 'operator-private-source' in result.stdout+result.stderr:
                raise ValueError('private acquisition source exposed')
            return result

        call('initialize', str(config))
        database = state/'acquisition.sqlite'
        with closing(sqlite3.connect(database)) as db:
            db.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross,state,request) VALUES(?,?,?,'cashu',?,1,8,'acquiring',?)",
                       [acquisition,'mint-a','v1','operator-private-source',b'synthetic unfunded retained request'])
            db.commit()
        now = int(time.time())
        decision = dict(version=1,wallet_instance=instance,wallet_epoch=epoch,acquisition_id=acquisition,
                        mint_id='mint-a',config_sha256=hashlib.sha256(config_bytes).hexdigest(),
                        reviewed_ownership_check=True,approved_at=now,expires_at=now+600)
        approval = root/'approval.json'

        def save(value):
            approval.write_text(json.dumps(value))
            approval.chmod(0o444)

        def image():
            snapshot = {}
            for path in sorted(state.rglob('*.sqlite')):
                with closing(sqlite3.connect(path)) as db:
                    schema = db.execute("SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name").fetchall()
                    rows = {}
                    for kind, name, _, definition in schema:
                        if kind != 'table':
                            continue
                        quoted = '"'+name.replace('"', '""')+'"'
                        # Queue order can depend on rowid. Preserve it while
                        # ignoring only schema-object insertion order: normal
                        # startup recreates identical treasury views.
                        if definition and 'WITHOUT ROWID' in definition.upper():
                            rows[name] = sorted(db.execute('SELECT * FROM '+quoted).fetchall(), key=repr)
                        else:
                            rows[name] = db.execute('SELECT rowid,* FROM '+quoted+' ORDER BY rowid').fetchall()
                    snapshot[str(path.relative_to(state))] = dict(schema=schema, rows=rows)
            return snapshot

        save(decision)
        original = image()
        arguments = ['reconcile-acquisition',str(config),str(approval)]
        result = json.loads(call(*arguments).stdout)
        if (result['state'] != 'pending' or result['acquisition_id'] != acquisition
                or result['workers_started'] or result['swap_or_melt_submitted'] or result['receipt_exported']):
            raise ValueError('unfunded retained operation acquired unsupported authority')
        call(*arguments)
        observed = image()
        if observed != original:
            raise ValueError('pending original-operation check changed retained state')
        with (state/'wallet-writer.lock').open('rb') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            call(*arguments,success=False)
        marker = state/'CASHU_RECOVERY_REQUIRED'
        marker.write_text('synthetic recovery fence')
        call(*arguments,success=False)
        marker.unlink()  # Disposable fixture only; no production fence is removed.
        config.chmod(0o644)
        config.write_bytes(config_bytes+b' ')
        config.chmod(0o444)
        call(*arguments,success=False)
        config.chmod(0o644)
        config.write_bytes(config_bytes)
        config.chmod(0o444)
        for changed in [dict(decision,wallet_epoch=str(uuid.uuid4())),
                        dict(decision,mint_id='mint-b'),dict(decision,acquisition_id=str(uuid.uuid4())),
                        dict(decision,expires_at=now-1),dict(decision,original_swap_replay=True)]:
            save(changed)
            call(*arguments,success=False)
        save(decision)
        approval.chmod(0o666)
        call(*arguments,success=False)
        approval.chmod(0o444)
        for status in ['validated','acquisition_prepared','held_for_reconciliation','credit_export_pending','credited']:
            with closing(sqlite3.connect(database)) as db:
                db.execute('UPDATE acquisitions SET state=? WHERE id=?',[status,acquisition])
                db.commit()
            before = image()
            call(*arguments,success=False)
            if image() != before:
                raise ValueError('refused state was changed')
        with closing(sqlite3.connect(database)) as db:
            db.execute("UPDATE acquisitions SET state='held_for_reconciliation' WHERE id=?", [acquisition])
            db.execute("INSERT INTO operation_holds VALUES(?,'acquiring','missing_owned_outputs')", [acquisition])
            db.commit()
        held = image()
        call(*arguments, success=False)
        save(dict(decision, reviewed_held_ownership_check=True))
        for _ in range(2):
            outcome = json.loads(call(*arguments).stdout)
            if (outcome['state'] != 'held_for_reconciliation' or outcome['acquisition_id'] != acquisition
                    or outcome['swap_or_melt_submitted'] or outcome['receipt_exported'] or image() != held):
                raise ValueError('reviewed hold with insufficient evidence did not remain unchanged')
        with closing(sqlite3.connect(database)) as db:
            db.execute("UPDATE operation_holds SET reason='ambiguous_acquisition' WHERE acquisition_id=?", [acquisition])
            db.commit()
        held = image()
        call(*arguments, success=False)
        if image() != held:
            raise ValueError('out-of-scope hold was changed')
        cleanup_approval = root/'cleanup-approval.json'
        cleanup = dict(version=1, purpose='cleanup_original_treasury_melt', wallet_instance=instance,
            wallet_epoch=epoch, core_epoch=core_epoch, intent_id=str(uuid.uuid4()), mint_id='mint-a',
            config_sha256=hashlib.sha256(config_bytes).hexdigest(), operation_evidence_digest='1'*64,
            approved_at=now, expires_at=now+600)
        cleanup_args = ['cleanup-treasury', str(config), str(cleanup_approval)]
        for updates, expected in [
            (dict(purpose='reconcile_original_treasury_melt', operation_evidence_digest=None), 'approval rejected'),
            (dict(operation_evidence_digest=None), 'approval rejected'),
            (dict(operation_evidence_digest='A'*64), 'approval rejected'),
            (dict(config_sha256='0'*64), 'configuration rejected'),
            ({}, 'failed'),
        ]:
            cleanup_approval.write_text(json.dumps(dict(cleanup, **updates)))
            cleanup_approval.chmod(0o444)
            failure = call(*cleanup_args, success=False).stderr
            if 'treasury reconciliation '+expected not in failure:
                raise ValueError('unexpected scoped cleanup refusal')
        call('reconcile-treasury', str(config), str(cleanup_approval), success=False)
        if image() != held:
            raise ValueError('refused treasury cleanup changed original wallet journal')
        candidate_file = root/'mint-candidate.json'
        mint_a = config_value['mints'][0]
        mint_b = dict(mint_a, id='mint-b', advertised_url='https://second.example/Bitcoin')
        key_b = dict(config_value['keysets'][0], mint_id='mint-b')
        candidate = dict(config_value, mints=[dict(mint_a, receive_cashu=False), mint_b],
                         keysets=[*config_value['keysets'], key_b],
                         custody_limits=dict(maximum_total_sats=300, maximum_per_mint_sats={'mint-a': 200, 'mint-b': 100}))

        def stage_candidate(value):
            candidate_file.write_text(json.dumps(value))
            candidate_file.chmod(0o444)
            return candidate_file.read_bytes()

        review_args = ['review-mint-policy', str(config), str(candidate_file)]
        next_bytes = stage_candidate(candidate)
        review = json.loads(call(*review_args).stdout)
        mints = {entry['mint_id']: entry for entry in review['mints']}
        if (review['applied'] or review['current_sha256'] != hashlib.sha256(config_bytes).hexdigest()
                or review['candidate_sha256'] != hashlib.sha256(next_bytes).hexdigest()
                or mints['mint-a']['receive_cashu'] or mints['mint-a']['uncredited_acquisitions'] != 1
                or not mints['mint-a']['retained_namespace'] or not mints['mint-b']['receive_cashu']
                or mints['mint-b']['retained_namespace'] or image() != held):
            raise ValueError('mint policy review lost pending state or exact candidate binding')
        for rejected in [
            dict(candidate, mints=[mint_b], keysets=[key_b]),
            dict(candidate, deadline_seconds=9),
            dict(candidate, custody_limits=config_value['custody_limits']),
            dict(candidate, custody_limits=dict(maximum_total_sats=301, maximum_per_mint_sats={'mint-a': 200, 'mint-b': 100})),
            dict(candidate, custody_limits=dict(maximum_total_sats=300, maximum_per_mint_sats={'mint-a': 201, 'mint-b': 100})),
            dict(candidate, keysets=[key_b]),
            dict(candidate, mints=[dict(mint_a, advertised_url='https://changed.example'), mint_b]),
            dict(candidate, keysets=[dict(config_value['keysets'][0], keys={
                '8':'02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'}), key_b]),
        ]:
            stage_candidate(rejected)
            call(*review_args, success=False)
        next_bytes = stage_candidate(candidate)
        with (state/'wallet-writer.lock').open('rb') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            call(*review_args, success=False)
        marker.write_text('synthetic recovery fence')
        call(*review_args, success=False)
        marker.unlink()
        if image() != held or config.read_bytes() != config_bytes:
            raise ValueError('mint policy refusal changed original state or configuration')

        # Apply through the authenticated launcher before the disposable restart.
        install_mint_policy(root, config, candidate_file, binary, state, uid, other_uids)
        if image() != held:
            raise ValueError('install changed the original wallet journal')
        with closing(sqlite3.connect(database)) as db:
            db.execute("UPDATE operation_holds SET reason='missing_owned_outputs' WHERE acquisition_id=?", [acquisition])
            db.commit()
        save(dict(decision, config_sha256=hashlib.sha256(next_bytes).hexdigest(), reviewed_held_ownership_check=True))
        answer = json.loads(call(*arguments).stdout)
        with closing(sqlite3.connect(database)) as db:
            retained = db.execute("SELECT id,mint_id,state,request FROM acquisitions").fetchall()
            namespaces = db.execute("SELECT mint_id,ready FROM cdk_mint_namespaces ORDER BY mint_id").fetchall()
        if (answer['state'] != 'held_for_reconciliation' or answer['acquisition_id'] != acquisition
                or retained != [(acquisition, 'mint-a', 'held_for_reconciliation', b'synthetic unfunded retained request')]
                or namespaces != [('mint-a', 1), ('mint-b', 1)]):
            raise ValueError('candidate restart changed the original work or namespace')
        print(json.dumps(dict(result='CASHU_OPERATOR_RECONCILIATION_CLI_VERIFIED',cli_sha256=args.cli_sha256,
            runtime_uid=uid,synthetic_unfunded_retained_operation=True,pending_replay_unchanged=True,
            active_writer_and_restore_fence_refused=True,configuration_scope_and_permissions_refused=True,
            held_prepared_new_and_exported_states_refused=True,mint_contact=False,
            eligible_reviewed_hold_preserved_without_evidence=True,other_hold_reason_refused=True,
            mint_policy_review_preserved_original=True, mint_policy_candidate_restart_verified=True,
            mint_policy_protected_install_and_retry_verified=True, mint_policy_systemctl_mocked=True,
            mint_policy_live_uid_locks_and_drift_refused=True,
            treasury_cleanup_scope_and_missing_original_refused=True,
            acquired_transition_qualified_here=False,workers_started=False,
            sql_definitions_rows_and_rowids_unchanged=True,schema_object_insertion_order_ignored=True),sort_keys=True))


if __name__ == '__main__':
    main()
