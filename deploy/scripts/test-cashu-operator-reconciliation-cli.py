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
import sqlite3
import subprocess
import tempfile
import time
import uuid


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
    uid, core_uid = [n for n in range(60000, 65000) if n not in used][:2]
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
        config = root/'config.json'
        config_value = dict(version=1, runtime_uid=uid, core_uid=core_uid, ipc_gid=uid,
            wallet_instance=instance, wallet_epoch=epoch, state_directory=str(state),
            intake_socket=str(root/'ipc/intake.sock'),
            receiving_key='0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
            recipient='f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9',
            descriptors=[], keysets=[], body_bytes=65536, proof_count=64, receipt_sats=10000,
            deadline_seconds=5, connections_per_second=8,
            mints=[dict(id='mint-a',advertised_url='https://mint.example/Bitcoin',policy_version='v1',
                receive_cashu=False,receive_nutzap=False,bolt11_exit_qualified=True)])
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
        print(json.dumps(dict(result='CASHU_OPERATOR_RECONCILIATION_CLI_VERIFIED',cli_sha256=args.cli_sha256,
            runtime_uid=uid,synthetic_unfunded_retained_operation=True,pending_replay_unchanged=True,
            active_writer_and_restore_fence_refused=True,configuration_scope_and_permissions_refused=True,
            held_prepared_new_and_exported_states_refused=True,mint_contact=False,
            eligible_reviewed_hold_preserved_without_evidence=True,other_hold_reason_refused=True,
            acquired_transition_qualified_here=False,workers_started=False,
            sql_definitions_rows_and_rowids_unchanged=True,schema_object_insertion_order_ignored=True),sort_keys=True))


if __name__ == '__main__':
    main()
