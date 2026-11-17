#!/usr/bin/env python3
"""Run real CDK/core financial composition with synthetic mint/provider fixtures.

Requires root in a fresh loopback-only network namespace and explicitly selected
native test artifacts. Baseline modes use a shared disposable non-root identity.
Optional receipt/treasury modes exercise production socket adapters across
separate UIDs; HTTP mode adds actual public admission. Providers remain synthetic
and physical confirmation is simulated in every mode.
"""
import argparse
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import runpy
import shutil
import subprocess
import tempfile
import time

CORE = 'strike::tests::cashu_treasury_tests::combined_fixture::combined_settlement_core_child'
WALLET = 'acquisition::tests::core_settlement_fixture::combined_settlement_wallet_child'
MULTI_MINT_WALLET = 'acquisition::tests::core_settlement_fixture::multi_mint::combined_multi_mint_wallet_child'


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core-test', type=Path, required=True)
    parser.add_argument('--wallet-test', type=Path, required=True)
    parser.add_argument('--lost-reply', action='store_true', help='lose original melt response and verify Strike before paused wallet recovery')
    parser.add_argument('--fees', action='store_true', help='qualify receive fees, melt input fee and returned change conservation')
    parser.add_argument('--no-change', action='store_true', help='with --fees, consume the bounded fee reserve and prove original credit feeds without a supplemental donation')
    parser.add_argument('--receipt-ipc', action='store_true', help='use distinct runtime UIDs and real receipt delivery; treasury remains file-coordinated')
    parser.add_argument('--treasury-ipc', action='store_true', help='require receipt IPC and use authenticated treasury Retain/Status for instructions and reports')
    parser.add_argument('--http-intake', action='store_true', help='send ordinary contribution through real HTTP and authenticated wallet intake')
    parser.add_argument('--cashu-ts-sender', action='store_true', help='use pinned Cashu-TS offline wallet selection and a separate Node test sender process')
    parser.add_argument('--sender-installation', type=Path)
    parser.add_argument('--sender-files-sha256')
    parser.add_argument('--sender-node', type=Path, default=Path('/usr/bin/node'))
    parser.add_argument('--repeat-request', action='store_true', help='acquire two distinct ordinary payments to one descriptor before settlement')
    parser.add_argument('--settle-change', action='store_true', help='settle retained one-sat change together with a distinct 16-sat Cashu contribution through a second independently verified Strike receive')
    parser.add_argument('--multi-mint', action='store_true', help='settle independently acquired contributions from two configured mint namespaces before one synthetic feeding')
    args = parser.parse_args()
    if args.multi_mint and (not args.fees or not args.no_change or not args.http_intake or args.repeat_request or args.settle_change):
        parser.error('--multi-mint requires its own --fees --no-change --http-intake mode')
    if args.settle_change and (not args.fees or args.no_change or args.repeat_request or not args.http_intake):
        parser.error('--settle-change requires --fees --http-intake, returned change, and its own mode')
    if args.repeat_request and not args.http_intake:
        parser.error('--repeat-request requires --http-intake')
    if args.cashu_ts_sender and (not args.http_intake or args.sender_installation is None or not args.sender_files_sha256):
        parser.error('--cashu-ts-sender requires --http-intake and pinned prepared dependencies')
    if args.http_intake and not args.treasury_ipc:
        parser.error('--http-intake requires --treasury-ipc')
    if args.treasury_ipc and not args.receipt_ipc:
        parser.error('--treasury-ipc requires --receipt-ipc')
    if args.no_change and not args.fees:
        parser.error('--no-change requires --fees')
    if os.geteuid() != 0 or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('requires root in a fresh network namespace')
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link', 'show']))
    if [link['ifname'] for link in links] != ['lo']:
        raise ValueError('fixture namespace must contain only loopback')
    subprocess.run(['/usr/sbin/ip', 'link', 'set', 'lo', 'up'], check=True)
    used = {p.pw_uid for p in pwd.getpwall()} | {g.gr_gid for g in grp.getgrall()}
    used |= {int(x) for x in subprocess.check_output(['ps', '-e', '-o', 'uid='], text=True).split()}
    available = [value for value in range(61000,65000) if value not in used]
    identity = available[0]
    wallet_uid = available[1] if args.receipt_ipc else identity
    ipc_gid = available[2] if args.receipt_ipc else identity
    with tempfile.TemporaryDirectory(prefix='lg-cashu-combined-') as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        exchange = root / 'exchange'
        exchange.mkdir(mode=0o770 if args.receipt_ipc else 0o700)
        exchange.chmod(0o770 if args.receipt_ipc else 0o700)
        os.chown(exchange, identity, ipc_gid)
        if args.receipt_ipc:
            for name,uid,mode in [('core-private',identity,0o700),('wallet-private',wallet_uid,0o700),('core-run',identity,0o710),('wallet-run',wallet_uid,0o710)]:
                directory=root/name
                directory.mkdir(mode=mode)
                directory.chmod(mode)
                os.chown(directory,uid,ipc_gid)
        sender_inventory = None
        sender_environment = {}
        if args.cashu_ts_sender:
            source_root = Path(__file__).resolve().parents[2]
            helper = source_root / 'tools/cashu-client-compat'
            copier = runpy.run_path(str(helper / 'copy-prepared.py'))['copy_prepared']
            sender_inventory = copier(args.sender_installation, root / 'sender-packages', args.sender_files_sha256, (helper / 'packages.json').read_bytes())
            node = root / 'sender-node'
            shutil.copyfile(args.sender_node.resolve(strict=True), node)
            node.chmod(0o555)
            version_command = ['/usr/bin/setpriv', f'--reuid={wallet_uid}', f'--regid={ipc_gid}', '--clear-groups', '--no-new-privs', '--', str(node), '--version']
            if subprocess.check_output(version_command, text=True).strip() != 'v22.23.1':
                raise ValueError('unqualified sender Node version')
            script = root / 'sender.mjs'
            shutil.copyfile(helper / 'send-fixture.mjs', script)
            script.chmod(0o555)
            sender_environment = {'LG_CASHU_COMBINED_SENDER_NODE':str(node),
                                  'LG_CASHU_COMBINED_SENDER_SCRIPT':str(script),
                                  'LG_CASHU_COMBINED_SENDER_INSTALLATION':str(root / 'sender-packages')}
        env = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LG_CASHU_COMBINED_FIXTURE': str(exchange),
               'LG_CASHU_COMBINED_LOST_REPLY': '1' if args.lost_reply else '0',
               'LG_CASHU_COMBINED_FEES': '1' if args.fees else '0',
               'LG_CASHU_COMBINED_NO_CHANGE': '1' if args.no_change else '0',
               'LG_CASHU_COMBINED_RECEIPT_IPC': '1' if args.receipt_ipc else '0',
               'LG_CASHU_COMBINED_WALLET_UID':str(wallet_uid),
               'LG_CASHU_COMBINED_IPC_GID':str(ipc_gid),
               'LG_CASHU_COMBINED_TREASURY_IPC':'1' if args.treasury_ipc else '0',
               'LG_CASHU_COMBINED_HTTP_INTAKE':'1' if args.http_intake else '0'}
        env['LG_CASHU_COMBINED_SETTLE_CHANGE'] = '1' if args.settle_change else '0'
        env['LG_CASHU_COMBINED_MULTI_MINT'] = '1' if args.multi_mint else '0'
        env['LG_CASHU_COMBINED_REPEAT_REQUEST'] = '1' if args.repeat_request else '0'
        env.update(sender_environment)
        prefix = ['/usr/bin/setpriv', f'--reuid={identity}', f'--regid={identity}',
                  '--clear-groups', '--no-new-privs', '--']
        processes, logs, hashes = [], [], {}
        try:
            for name, source, test in [('core', args.core_test, CORE), ('wallet', args.wallet_test, MULTI_MINT_WALLET if args.multi_mint else WALLET)]:
                uid = identity if name=='core' else wallet_uid
                prefix = ['/usr/bin/setpriv',f'--reuid={uid}',f'--regid={ipc_gid}',
                          '--clear-groups','--no-new-privs','--']
                child_env = dict(env)
                if args.receipt_ipc:
                    child_env['LG_CASHU_COMBINED_PRIVATE']=str(root/(name+'-private'))
                    child_env['TMPDIR']=str(root/(name+'-private'))
                source = source.resolve(strict=True)
                if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
                    raise ValueError('invalid explicitly selected artifact')
                binary = root / (name + '-test')
                shutil.copyfile(source, binary)
                binary.chmod(0o555)
                hashes[name] = digest(binary)
                listed = subprocess.run(prefix + [str(binary), '--list'], env=child_env,
                                        capture_output=True, text=True, timeout=15, check=True)
                if f'{test}: test' not in listed.stdout.splitlines():
                    raise ValueError(f'{name} fixture missing from selected artifact')
                log = (root / (name + '.log')).open('w+')
                logs.append(log)
                process = subprocess.Popen(prefix + [str(binary), test, '--exact', '--ignored', '--nocapture'],
                                           env=child_env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                processes.append(process)
            deadline = time.monotonic() + 150
            while any(process.poll() is None for process in processes):
                if any(process.poll() not in (None, 0) for process in processes):
                    raise ValueError('combined fixture child failed')
                if time.monotonic() >= deadline:
                    raise ValueError('combined fixture deadline exceeded')
                time.sleep(0.05)
            if any(process.returncode != 0 for process in processes):
                raise ValueError('combined fixture failed')
            completion = (['COMBINED_CORE_TWO_SETTLEMENTS_ONE_SYNTHETIC_FEED', 'COMBINED_WALLET_THREE_ACQUISITIONS_TWO_MELTS']
                          if args.settle_change else ['COMBINED_CORE_ONE_SETTLEMENT_ONE_SYNTHETIC_FEED', 'COMBINED_WALLET_TWO_ACQUISITIONS_ONE_MELT'])
            if args.multi_mint:
                completion = ['COMBINED_CORE_TWO_SETTLEMENTS_ONE_SYNTHETIC_FEED', 'COMBINED_WALLET_TWO_MINTS_TWO_ACQUISITIONS_TWO_MELTS']
            for index, (log, marker) in enumerate(zip(logs, completion)):
                log.seek(0)
                output = log.read()
                markers = [marker]
                if args.multi_mint and index == 0:
                    markers.append('COMBINED_CORE_TWO_MINTS_FINANCIAL_PLAN_VERIFIED')
                if index == 1:
                    markers.append('COMBINED_WALLET_HEALTH_ACCOUNTING_VERIFIED')
                if args.repeat_request:
                    markers.append(['COMBINED_CORE_REUSABLE_REQUEST_TWO_PAYMENTS_VERIFIED','COMBINED_WALLET_REUSABLE_REQUEST_TWO_PAYMENTS_VERIFIED'][index])
                if args.cashu_ts_sender and index == 1:
                    markers.append('COMBINED_WALLET_CASHU_TS_SENDER_VERIFIED')
                if args.http_intake:
                    markers.append(['COMBINED_CORE_PUBLIC_HTTP_FINANCIAL_IPC_VERIFIED','COMBINED_WALLET_PUBLIC_HTTP_FINANCIAL_IPC_VERIFIED'][index])
                if args.treasury_ipc:
                    markers.append(['COMBINED_CORE_FULL_FINANCIAL_IPC_VERIFIED','COMBINED_WALLET_FULL_FINANCIAL_IPC_VERIFIED'][index])
                if args.receipt_ipc:
                    markers.append(['COMBINED_CORE_CDK_RECEIPT_SOCKET_VERIFIED','COMBINED_WALLET_CDK_RECEIPT_SOCKET_VERIFIED'][index])
                if args.lost_reply:
                    markers.append(['COMBINED_CORE_EARLY_STRIKE_WAITS_FOR_WALLET_VERIFIED',
                                    'COMBINED_WALLET_LOST_REPLY_PAUSED_RECOVERY_VERIFIED'][index])
                if args.fees:
                    markers.append(['COMBINED_CORE_FEES_CHANGE_CONSERVED',
                                    'COMBINED_WALLET_REAL_FEES_CHANGE_VERIFIED'][index])
                if args.settle_change:
                    markers.append(['COMBINED_CORE_RESIDUAL_CHANGE_SETTLED', 'COMBINED_WALLET_RESIDUAL_CHANGE_SETTLED'][index])
                if args.no_change:
                    markers.append(['COMBINED_CORE_FEE_ONLY_ORIGINAL_CREDIT_CONSUMED',
                                    'COMBINED_WALLET_FEE_ONLY_ZERO_CHANGE_VERIFIED'][index])
                if any(marker not in output for marker in markers):
                    raise ValueError('combined fixture completion marker missing')
            result = json.loads((exchange / 'core-result.json').read_text())
            expected = {'credit_sats': 0, 'contributions': 3, 'synthetic_confirmed_feeds': 1,
                        'treasury_deposit_sats': 400, 'new_transfer_credit_sats': 0}
            if args.fees:
                expected = {'credit_sats': 1, 'contributions': 4, 'synthetic_confirmed_feeds': 1,
                            'treasury_deposit_sats': 398, 'new_transfer_credit_sats': 0,
                            'original_granted_credit_sats': 1000, 'receive_fees_sats': 2,
                            'conversion_fee_sats': 1, 'retained_change_sats': 1,
                            'supplemental_native_sats': 1}
            if args.no_change:
                expected.update(credit_sats=0, contributions=3, conversion_fee_sats=2,
                                retained_change_sats=0, supplemental_native_sats=0)
            if args.repeat_request:
                expected['contributions'] += 1
                if args.fees:
                    expected['treasury_deposit_sats'] -= 1
                    expected['receive_fees_sats'] += 1
                    expected['conversion_fee_sats'] += 1
            if args.settle_change:
                expected = dict(credit_sats=16, contributions=4, synthetic_confirmed_feeds=1,
                    treasury_deposit_sats=413, new_transfer_credit_sats=0, original_granted_credit_sats=1016,
                    receive_fees_sats=3, conversion_fee_sats=3, retained_change_sats=0,
                    supplemental_native_sats=0, settlement_operations=2)
            if args.multi_mint:
                expected = dict(credit_sats=0, contributions=3, synthetic_confirmed_feeds=1,
                    treasury_deposit_sats=396, new_transfer_credit_sats=0, original_granted_credit_sats=1000,
                    receive_fees_sats=2, conversion_fee_sats=4, retained_change_sats=0,
                    supplemental_native_sats=0, settlement_operations=2, configured_mints=2)
            if result != expected:
                raise ValueError('combined fixture result differs from expected conservation')
            print(json.dumps({'result': 'CASHU_COMBINED_SETTLEMENT_VERIFIED', 'artifacts_sha256': hashes,
                              'financial_result': result, 'live_provider': False, 'wallet_health_verified': True,
                              'nonzero_fees': args.fees, 'returned_change': args.fees and not args.no_change,
                              'lost_melt_reply': args.lost_reply, 'strike_before_wallet_report': args.lost_reply,
                              'wallet_recovery_while_paused': args.lost_reply,
                              'physical_actuation': False, 'distinct_uid_ipc': args.treasury_ipc, 'distinct_uid_receipt_ipc': args.receipt_ipc, 'treasury_ipc': args.treasury_ipc, 'http_intake': args.http_intake, 'tls_verified': False, 'independent_sender': False, 'cashu_ts_offline_sender': args.cashu_ts_sender, 'repeat_request': args.repeat_request, 'residual_change_settled': args.settle_change, 'multiple_mints': args.multi_mint, 'sender_inventory': sender_inventory, 'sender_node_sha256': digest(root / 'sender-node') if args.cashu_ts_sender else None, 'sender_script_sha256': digest(root / 'sender.mjs') if args.cashu_ts_sender else None}, sort_keys=True))
        except Exception:
            for log in logs:
                log.flush()
                log.seek(0)
                print(log.read()[-8192:])
            raise
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
            for log in logs:
                log.close()


if __name__ == '__main__':
    main()
