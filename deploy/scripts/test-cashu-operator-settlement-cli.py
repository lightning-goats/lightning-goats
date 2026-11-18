#!/usr/bin/env python3
"""Qualify actual protected planning CLI; synthetic empty ledger, no provider calls.

Requires fresh mount/network namespaces. The ledger API's funded reservation and
replay tests are separate; this exercises native configuration/approval/UID and
stopped-service boundaries without manufacturing owned ecash.
"""
import argparse
from contextlib import closing
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
import tomllib
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--cli-sha256', required=True)
    args = parser.parse_args()
    if os.geteuid() != 0 or any(os.readlink('/proc/self/ns/'+kind) == os.readlink('/proc/1/ns/'+kind)
                                  for kind in ['mnt', 'net']):
        raise ValueError('requires root in fresh mount and network namespaces')
    data = args.cli.resolve(strict=True).read_bytes()
    if len(data) > 128*1024*1024 or hashlib.sha256(data).hexdigest() != args.cli_sha256:
        raise ValueError('selected operator CLI differs from reviewed bytes')
    subprocess.run(['/usr/bin/mount', '--make-rprivate', '/'], check=True)
    used = {u.pw_uid for u in pwd.getpwall()} | {g.gr_gid for g in grp.getgrall()}
    used |= {int(x) for x in subprocess.check_output(['ps','-e','-o','uid='],text=True).split()}
    uid, wallet_uid, ipc_gid = [n for n in range(60000,65000) if n not in used][:3]
    source = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix='lg-settlement-cli-', dir='/run') as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        binary = root/'cli'
        binary.write_bytes(data)
        binary.chmod(0o555)
        state = root/'state'
        state.mkdir(mode=0o700)
        os.chown(state,uid,uid)
        database = state/'ledger.sqlite'
        config = root/'config.toml'
        template = (source/'deploy/config.canary.toml.example').read_text()
        anchor = 'sqlite:///var/lib/lightning-goats/lightning-goats-canary.db'
        if template.count(anchor) != 1:
            raise ValueError('configuration anchor changed')
        epoch, instance, wallet_epoch = [str(uuid.uuid4()) for _ in range(3)]
        config_text = template.replace(anchor,'sqlite://'+str(database)) + f'''

[cashu_receipts]
socket = "/run/core/receipts.sock"
ipc_gid = {ipc_gid}
wallet_uid = {wallet_uid}
wallet_instance = "{instance}"
wallet_epoch = "{wallet_epoch}"
core_epoch = "{epoch}"
deadline_seconds = 5
connections_per_second = 1

[cashu_treasury]
socket = "/run/wallet/treasury.sock"
deadline_seconds = 5
[cashu_treasury.policy]
enabled = true
version = "operator-fixture-v1"
invoice_expiry_seconds = 300
[cashu_treasury.policy.destination]
origin = "https://treasury.example/"
account_id = "{uuid.uuid4()}"
credential_fingerprint = "{'55'*32}"
network = "bitcoin"
[cashu_treasury.policy.limits]
minimum_deposit_sats = 1
maximum_deposit_sats = 1000
maximum_input_sats = 1000
maximum_fee_sats = 10
fee_sats_per_24h = 100
operations_per_24h = 10
input_sats_per_24h = 2000
maximum_in_flight_sats = 1000
[cashu_treasury.policy.approved_mints.mint-a]
minimum_deposit_sats = 1
maximum_deposit_sats = 1000
maximum_input_sats = 1000
maximum_fee_sats = 10
maximum_in_flight_sats = 1000
'''
        config.write_text(config_text)
        os.chown(config,0,uid)
        config.chmod(0o640)
        env = {'PATH':'/usr/bin:/bin','LC_ALL':'C'}
        prefix = ['/usr/bin/setpriv',f'--reuid={uid}',f'--regid={uid}','--clear-groups',
                  '--no-new-privs','--',str(binary),'--config',str(config)]

        def call(*arguments, success=True):
            result = subprocess.run(prefix+list(arguments),env=env,capture_output=True,text=True,timeout=30)
            if (result.returncode == 0) != success:
                raise ValueError('unexpected native CLI result: '+(result.stdout+result.stderr)[-4096:])
            return result

        call('status')  # Native migrations as the disposable ledger owner.
        database.chmod(0o600)
        with closing(sqlite3.connect(database)) as db:
            db.execute('INSERT INTO overlay_identity VALUES(1,?)',[epoch])
            db.commit()
        now = int(time.time())
        decision = dict(version=1,core_epoch=epoch,wallet_instance=instance,wallet_epoch=wallet_epoch,
                        config_sha256=hashlib.sha256(config.read_bytes()).hexdigest(),
                        request=dict(id=str(uuid.uuid4()),mint_id='mint-a',maximum_input_sats=200),
                        reviewed_settlement_planning=True,approved_at=now,expires_at=now+600)
        approval = root/'approval.json'

        def save(value):
            approval.write_text(json.dumps(value))
            os.chown(approval,0,uid)
            approval.chmod(0o640)

        save(decision)
        status = root/'service-state'
        stopped = 'Id=lightning-goats-receive.service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlGroup=\n'
        status.write_text(stopped)
        status.chmod(0o644)
        systemctl = root/'systemctl'
        systemctl.write_text('#!/bin/sh\nexec /bin/cat '+str(status)+'\n')
        systemctl.chmod(0o555)
        subprocess.run(['/usr/bin/mount','--bind',str(systemctl),'/usr/bin/systemctl'],check=True)

        def image():
            with closing(sqlite3.connect(database)) as db:
                return list(db.iterdump())

        original = image()
        arguments = ['plan-cashu-settlement','--approval',str(approval),'--writers-stopped']
        preview = json.loads(call(*arguments).stdout)
        if preview['would_plan'] or preview['apply_requested'] or image() != original:
            raise ValueError('empty-custody preview mutated the ledger')
        with closing(sqlite3.connect(database)) as db:
            db.execute('DELETE FROM overlay_identity')
            db.commit()
        missing_identity = image()
        call(*arguments,'--apply',success=False)
        if image() != missing_identity:
            raise ValueError('missing core epoch was initialized or changed')
        with closing(sqlite3.connect(database)) as db:
            db.execute('INSERT INTO overlay_identity VALUES(1,?)',[epoch])
            db.commit()
        status.write_text(stopped.replace('ActiveState=inactive','ActiveState=active'))
        call(*arguments,'--apply',success=False)
        status.write_text(stopped)
        config.write_text(config_text+'\n# unauthorized change\n')
        call(*arguments,'--apply',success=False)
        config.write_text(config_text)
        for changed in [dict(decision,core_epoch=str(uuid.uuid4())),
                        dict(decision,expires_at=now-1),dict(decision,invoice='forbidden')]:
            save(changed)
            call(*arguments,'--apply',success=False)
        save(decision)
        approval.chmod(0o660)
        call(*arguments,'--apply',success=False)
        approval.chmod(0o640)
        if image() != original:
            raise ValueError('refusal changed financial state')
        applied = json.loads(call(*arguments,'--apply').stdout)
        if applied['would_plan'] or applied['provider_request'] or applied['physical_actuation']:
            raise ValueError('empty-custody apply exceeded scope')
        retained = image()
        call(*arguments,'--apply')
        changed = dict(decision,request=dict(decision['request'],maximum_input_sats=201))
        save(changed)
        call(*arguments,'--apply',success=False)
        if image() != retained:
            raise ValueError('retry or conflicting request changed retained scope')
        reconciliation = dict(version=1, purpose='reconcile_original_cashu_settlement',
            core_epoch=epoch, wallet_instance=instance, wallet_epoch=wallet_epoch,
            intent_id=str(uuid.uuid4()), intent_digest='11'*32,
            receive_request_id=str(uuid.uuid4()), receive_id=str(uuid.uuid4()),
            config_sha256=decision['config_sha256'], approved_at=now, expires_at=now+600)
        reconcile = ['reconcile-cashu-settlement','--approval',str(approval),'--writers-stopped','--apply']
        for change, expected in [
            (dict(purpose='cashu_settlement_planning'), 'approval is invalid'),
            (dict(config_sha256='22'*32), 'configuration differs'),
            ({}, 'unknown treasury intent'),
        ]:
            save(dict(reconciliation, **change))
            if expected not in call(*reconcile, success=False).stderr:
                raise ValueError('native original-settlement refusal differs from fixture')
        save(reconciliation)
        status.write_text(stopped.replace('ActiveState=inactive','ActiveState=active'))
        if 'core must be fully stopped' not in call(*reconcile, success=False).stderr:
            raise ValueError('original-settlement recovery accepted an active core')
        status.write_text(stopped)
        marker = state/'CYBERHERD_RECOVERY_REQUIRED'
        marker.write_text('disposable recovery fence')
        if 'restored journal requires' not in call(*reconcile, success=False).stderr:
            raise ValueError('original-settlement recovery bypassed the recovery fence')
        marker.unlink()
        if image() != retained:
            raise ValueError('original-settlement refusal changed retained scope')
        with closing(sqlite3.connect(database)) as db:
            counts = {table:db.execute('SELECT COUNT(*) FROM '+table).fetchone()[0]
                      for table in ['cashu_sweep_requests','cashu_treasury_intents','feed_attempts','ledger_entries']}
        if counts != dict(cashu_sweep_requests=1,cashu_treasury_intents=0,feed_attempts=0,ledger_entries=0):
            raise ValueError('unexpected financial mutations')

        # An unissued synthetic intent exercises the CLI parser and readonly
        # snapshot without creating wallet holdings, an invoice or a feeder plan.
        destination = tomllib.loads(config_text)['cashu_treasury']['policy']['destination']
        intent = dict(version=1, purpose='cashu_treasury_settlement', id=str(uuid.uuid4()),
                      core_epoch=epoch, wallet_instance=instance, wallet_epoch=wallet_epoch,
                      mint_id='mint-a', treasury_policy='operator-fixture-v1', destination=destination,
                      input_sats=8, deposit_sats=7, maximum_fee_sats=1, invoice_expiry_seconds=300,
                      sources=[dict(ledger_id=1, acquisition_id=str(uuid.uuid4()),
                                    receipt_digest='ab'*32, input_sats=8)])
        encoded = json.dumps(intent, separators=(',', ':'))
        digest = hashlib.sha256(
            ('["lightning-goats/cashu-treasury/v1",' + encoded + ']').encode()
        ).hexdigest()
        with closing(sqlite3.connect(database)) as db:
            db.execute('INSERT INTO cashu_treasury_intents VALUES(?,?,?,?,?,?,?)',
                       [intent['id'], digest, encoded, 'mint-a', 8, 7, now])
            db.commit()
        before_inspection = image()
        inspect = ['inspect-cashu-settlement', '--intent', intent['id']]
        status.write_text(stopped.replace('ActiveState=inactive','ActiveState=active'))
        result = call(*inspect)
        inspected = json.loads(result.stdout)
        if (inspected['intent_id'] != intent['id'] or inspected['core_epoch'] != epoch
                or inspected['issuance_claimed'] or inspected['settlement_complete']
                or inspected['linked_feed_plan'] is not None or digest in result.stdout
                or destination['credential_fingerprint'] in result.stdout):
            raise ValueError('native readonly settlement status differs from its retained evidence')
        call('inspect-cashu-settlement', '--intent', str(uuid.uuid4()), success=False)
        config.chmod(0o660)
        call(*inspect, success=False)
        config.chmod(0o640)
        database.chmod(0o640)
        call(*inspect, success=False)
        database.chmod(0o600)
        marker.write_text('disposable recovery fence')
        call(*inspect, success=False)
        marker.unlink()
        if image() != before_inspection:
            raise ValueError('readonly inspection or a refusal changed the ledger')
        print(json.dumps(dict(result='CASHU_OPERATOR_SETTLEMENT_CLI_VERIFIED',cli_sha256=args.cli_sha256,
            runtime_uid=uid,preview_unchanged=True,active_service_refused=True,
            approval_config_and_scope_refused=True,one_request_retained=True,retry_unchanged=True,
            missing_core_epoch_not_initialized=True,
            original_settlement_scope_missing_intent_and_fence_refused=True,
            original_settlement_read_only_while_core_active=True,
            original_settlement_private_files_and_fence_refused=True,
            synthetic_empty_custody=True,synthetic_systemctl=True,installed_systemd_acceptance=False,
            provider_request=False,physical_actuation=False),sort_keys=True))


if __name__ == '__main__':
    main()
