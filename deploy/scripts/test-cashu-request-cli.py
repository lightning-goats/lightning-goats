#!/usr/bin/env python3
"""Check offline request preparation through the actual CLI in an isolated netns.

Uses protected synthetic public configuration, no keys, wallet state or services.
Emitted descriptors point only at .example hosts and must never receive funds.
"""
import argparse
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import shutil
import subprocess
import tempfile
import uuid


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--long-request', action='store_true', help='qualify four longer mint URLs and a longer HTTPS transport')
    args = parser.parse_args()
    if os.geteuid() != 0 or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('requires root in a fresh network namespace')
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link', 'show']))
    if [link['ifname'] for link in links] != ['lo']:
        raise ValueError('fixture namespace must contain only loopback')
    used = {p.pw_uid for p in pwd.getpwall()} | {g.gr_gid for g in grp.getgrall()}
    used |= {int(x) for x in subprocess.check_output(['ps', '-e', '-o', 'uid='], text=True).split()}
    identities = [value for value in range(61000, 65000) if value not in used][:2]
    if len(identities) != 2:
        raise ValueError('unused fixture identities unavailable')
    runtime_uid, core_uid = identities
    source = args.binary.resolve(strict=True)
    if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
        raise ValueError('invalid explicitly selected artifact')
    # /run has protected root-owned ancestry, unlike world-writable /tmp.
    with tempfile.TemporaryDirectory(prefix='lg-cashu-request-', dir='/run') as temporary:
        root = Path(temporary)
        root.chmod(0o755)
        binary = root / 'cashud'
        shutil.copyfile(source, binary)
        binary.chmod(0o555)
        state = root / 'uninitialized-wallet'
        config = root / 'config.json'
        mint_names = [f'mint-{index}-compatibility-fixture' for index in range(4)] if args.long_request else ['mint-one', 'mint-two']
        config.write_text(json.dumps({
            'version': 1, 'runtime_uid': runtime_uid, 'core_uid': core_uid, 'ipc_gid': runtime_uid,
            'wallet_instance': str(uuid.uuid4()), 'wallet_epoch': str(uuid.uuid4()),
            'state_directory': str(state), 'intake_socket': str(root / 'run/intake.sock'),
            'receiving_key': '0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
            'recipient': 'f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9',
            'descriptors': [], 'keysets': [],
            'mints': [{'id': name, 'advertised_url': f'https://{name}.example',
                       'policy_version': 'offline-fixture-v1', 'receive_cashu': True,
                       'receive_nutzap': False, 'bolt11_exit_qualified': True}
                      for name in mint_names],
            'body_bytes': 65536, 'proof_count': 64, 'receipt_sats': 10000,
            'deadline_seconds': 5, 'connections_per_second': 8,
        }))
        config.chmod(0o444)
        original = config.read_bytes()
        prefix = ['/usr/bin/setpriv', f'--reuid={runtime_uid}', f'--regid={runtime_uid}',
                  '--clear-groups', '--no-new-privs', '--', str(binary)]
        env = {'PATH': '/usr/bin:/bin'}

        def invoke(endpoint, encoding):
            return subprocess.run(prefix + ['prepare-request', str(config), endpoint, encoding],
                                  env=env, stdin=subprocess.DEVNULL, capture_output=True,
                                  text=True, timeout=15)

        endpoint = 'https://lightning-goats.example/c'
        if args.long_request:
            endpoint = 'https://lightning-goats.example/' + 'request-fixture-' * 6 + 'cashu'
        requests = {}
        for encoding in ['creqA', 'creqB']:
            result = invoke(endpoint, encoding)
            if result.returncode or result.stderr:
                raise ValueError('request preparation failed: ' + result.stderr[-2048:])
            value = json.loads(result.stdout)
            if (value['status'] != 'prepared' or value['service_started']
                    or value['descriptor_registered'] or value['encoding'] != encoding):
                raise ValueError('request preparation claimed activation or changed encoding')
            payload = value['request']
            if not (payload.startswith('creqA') if encoding == 'creqA' else payload.lower().startswith('creqb1')):
                raise ValueError('request has unexpected encoding')
            repeat = invoke(endpoint, encoding)
            if repeat.returncode or json.loads(repeat.stdout) != value:
                raise ValueError('repeat preparation changed receiving identity or payload')
            requests[encoding] = value
        if requests['creqA']['id'] != requests['creqB']['id']:
            raise ValueError('encoding selection changed descriptor identity')
        changed = invoke('https://lightning-goats.example/other', 'creqA')
        if changed.returncode or json.loads(changed.stdout)['id'] == requests['creqA']['id']:
            raise ValueError('endpoint change did not version descriptor')
        rejected = invoke('https://fixture-user:fixture-secret@example.com/c', 'creqA')
        if rejected.returncode != 1 or rejected.stdout or 'fixture-secret' in rejected.stderr:
            raise ValueError('private endpoint rejection was not coarse and nonzero')
        malformed = invoke(endpoint, 'automatic')
        if malformed.returncode != 2:
            raise ValueError('encoding selection must be explicit')
        if config.read_bytes() != original or state.exists() or (root / 'run').exists():
            raise ValueError('offline preparation mutated configuration, state or listeners')
        print(json.dumps({'result': 'CASHU_REQUEST_CLI_VERIFIED', 'binary_sha256': digest(binary),
                          'synthetic_only': True, 'long_request': args.long_request, 'state_created': False, 'services_started': False,
                          'requests': requests}, sort_keys=True))


if __name__ == '__main__':
    main()
