#!/usr/bin/env python3
"""Exercise the actual release CLI using disposable DB/UID and namespace-local systemctl.

Requires fresh mount and network namespaces. Never inspects or stops a host unit.
Service-state responses are synthetic; this is not installed systemd acceptance.
"""
import argparse
from contextlib import closing
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import shutil
import sqlite3
import subprocess
import tempfile
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', required=True, type=Path)
    parser.add_argument('--cli-sha256', required=True)
    args = parser.parse_args()
    if os.geteuid() != 0 or any(os.readlink('/proc/self/ns/' + kind) == os.readlink('/proc/1/ns/' + kind) for kind in ['mnt', 'net']):
        raise ValueError('requires root in fresh mount and network namespaces')
    data = args.cli.resolve(strict=True).read_bytes()
    if len(data) > 128 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != args.cli_sha256:
        raise ValueError('selected CLI differs from reviewed bytes')
    subprocess.run(['/usr/bin/mount', '--make-rprivate', '/'], check=True)
    used = {p.pw_uid for p in pwd.getpwall()} | {g.gr_gid for g in grp.getgrall()}
    used |= {int(v) for v in subprocess.check_output(['ps', '-e', '-o', 'uid='], text=True).split()}
    uid = next(v for v in range(60000, 65000) if v not in used)
    source = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix='lg-release-cli-', dir='/run') as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        binary = root / 'cli'
        binary.write_bytes(data)
        binary.chmod(0o555)
        state = root / 'state'
        state.mkdir(mode=0o700)
        os.chown(state, uid, uid)
        database = state / 'ledger.sqlite'
        config = root / 'config.toml'
        template = (source / 'deploy/config.canary.toml.example').read_text()
        old = 'sqlite:///var/lib/lightning-goats/lightning-goats-canary.db'
        if template.count(old) != 1:
            raise ValueError('configuration anchor changed')
        config.write_text(template.replace(old, 'sqlite://' + str(database)))
        os.chown(config, 0, uid)
        config.chmod(0o640)
        prefix = ['/usr/bin/setpriv', f'--reuid={uid}', f'--regid={uid}', '--clear-groups', '--no-new-privs', '--', str(binary), '--config', str(config)]
        env = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}

        def call(*arguments, success=True):
            r = subprocess.run(prefix + list(arguments), env=env, capture_output=True, text=True, timeout=20)
            if (r.returncode == 0) != success:
                raise ValueError('unexpected CLI result: ' + (r.stdout + r.stderr)[-4096:])
            return r

        call('status')  # Actual native migrations, under the disposable runtime owner.
        database.chmod(0o600)
        epoch, plan = str(uuid.uuid4()), str(uuid.uuid4())
        with closing(sqlite3.connect(database)) as connection:
            connection.execute('INSERT INTO overlay_identity VALUES(1,?)', [epoch])
            connection.execute('INSERT INTO feed_financial_plans(id,threshold_sats) VALUES(?,1000)', [plan])
            connection.commit()
        approval = root / 'approval.json'
        now = int(time.time())
        approval.write_text(json.dumps(dict(version=1, epoch=epoch, plan_id=plan,
            threshold_sats=1000, reviewed_financial_release=True, approved_at=now, expires_at=now+600)))
        os.chown(approval, 0, uid)
        approval.chmod(0o640)
        status = root / 'service-state'
        response = 'Id=lightning-goats-receive.service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlGroup=\n'
        status.write_text(response)
        status.chmod(0o644)
        systemctl = root / 'systemctl'
        systemctl.write_text('#!/bin/sh\nexec /bin/cat ' + str(status) + '\n')
        systemctl.chmod(0o555)
        subprocess.run(['/usr/bin/mount', '--bind', str(systemctl), '/usr/bin/systemctl'], check=True)

        def image():
            with closing(sqlite3.connect(database)) as connection:
                return list(connection.iterdump())

        original = image()
        arguments = ['release-financial-plan', '--approval', str(approval), '--writers-stopped']
        preview = json.loads(call(*arguments).stdout)
        if not preview['would_release'] or preview['changed'] or image() != original:
            raise ValueError('preview changed financial state')
        status.write_text(response.replace('ActiveState=inactive', 'ActiveState=active'))
        call(*arguments, '--apply', success=False)
        if image() != original:
            raise ValueError('active-service refusal changed financial state')
        status.write_text(response)
        applied = json.loads(call(*arguments, '--apply').stdout)
        if not applied['changed'] or applied['custody_released'] or applied['provider_request'] or applied['physical_actuation']:
            raise ValueError('release exceeded scope')
        repeated = json.loads(call(*arguments, '--apply').stdout)
        if repeated['changed']:
            raise ValueError('repeat release changed state')
        with closing(sqlite3.connect(database)) as connection:
            row = connection.execute('SELECT state,disposition FROM feed_financial_plans WHERE id=?', [plan]).fetchone()
        if row != ('released', 'operator_release') or database.stat().st_uid != uid:
            raise ValueError('release state or owner changed unexpectedly')
        print(json.dumps({'result': 'CASHU_FINANCIAL_RELEASE_CLI_VERIFIED', 'cli_sha256': args.cli_sha256,
            'preview_unchanged': True, 'active_service_refused': True, 'apply_once': True,
            'runtime_uid': uid, 'synthetic_systemctl': True, 'installed_systemd_acceptance': False,
            'real_payment': False, 'physical_actuation': False}, sort_keys=True))


if __name__ == '__main__':
    main()
