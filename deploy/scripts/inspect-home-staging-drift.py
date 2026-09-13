#!/usr/bin/env python3
"""Read-only, private staging inventory/check. Never applies network policy.

Capture with --snapshot /protected/new.json; compare with --check that path.
Only --snapshot writes a new exclusive 0600 evidence file. No credentials are
printed or exported. Keep peer/topology metadata private, outside the repository.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def stable_nft(value):
    if isinstance(value, list):
        return [stable_nft(x) for x in value if not (isinstance(x, dict) and 'metainfo' in x)]
    if isinstance(value, dict):
        return {k: ({key: v for key, v in item.items() if key not in ('packets', 'bytes')}
                    if k == 'counter' and isinstance(item, dict) else stable_nft(item))
                for k, item in value.items()}
    return value


def stable_routes(routes):
    # RA lifetimes tick between reads. Preserve the presence of an expiry and
    # all forwarding fields; expired/removed routes still change the inventory.
    return [dict(route, expires='dynamic') if 'expires' in route else route for route in routes]


def inventory():
    if os.geteuid() != 0:
        raise ValueError('root required for complete read-only inventory')
    files = {}
    for name in ('/etc/wireguard/wg0.conf', '/etc/lightning-goats-gateway-canary/config.toml',
                 '/etc/systemd/system/lightning-goats-gateway-canary.service'):
        path = Path(name)
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode):
            raise ValueError('unexpected inventory file type')
        files[name] = {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                       'uid': info.st_uid, 'gid': info.st_gid, 'mode': stat.S_IMODE(info.st_mode)}
    return {'schema': 1, 'files': files,
            'wg_public_key': command('wg', 'show', 'wg0', 'public-key'),
            'wg_listen_port': command('wg', 'show', 'wg0', 'listen-port'),
            'wg_allowed_ips': command('wg', 'show', 'wg0', 'allowed-ips'),
            'wg_endpoints': command('wg', 'show', 'wg0', 'endpoints'),
            'routes_v4': stable_routes(json.loads(command('ip', '-j', '-4', 'route', 'show', 'table', 'all'))),
            'routes_v6': stable_routes(json.loads(command('ip', '-j', '-6', 'route', 'show', 'table', 'all'))),
            'rules_v4': command('ip', '-4', 'rule', 'show'),
            'rules_v6': command('ip', '-6', 'rule', 'show'),
            'nft': stable_nft(json.loads(command('nft', '-j', 'list', 'ruleset'))),
            'wireguard_manager': command('systemctl', 'show', 'wg-quick@wg0.service',
                                        '-p', 'FragmentPath', '-p', 'ActiveState', '-p', 'DropInPaths'),
            'canary': command('systemctl', 'show', 'lightning-goats-gateway-canary.service',
                             '-p', 'FragmentPath', '-p', 'ActiveState', '-p', 'UnitFileState',
                             '-p', 'DropInPaths', '-p', 'NeedDaemonReload')}


def read_snapshot(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o077:
        raise ValueError('snapshot must be root-owned private regular file')
    return json.loads(path.read_text())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--snapshot', type=Path)
    group.add_argument('--check', type=Path)
    args = parser.parse_args()
    current = inventory()
    if args.snapshot:
        for parent in args.snapshot.absolute().parents:
            info = parent.lstat()
            if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
                raise ValueError('snapshot needs protected root-owned parent')
        with os.fdopen(os.open(args.snapshot, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), 'w') as handle:
            json.dump(current, handle, sort_keys=True, indent=2)
            handle.flush()
            os.fsync(handle.fileno())
        print('Private snapshot written; no network changes')
    else:
        expected = read_snapshot(args.check)
        changed = sorted(k for k in current.keys() | expected.keys() if current.get(k) != expected.get(k))
        if changed:
            print('DRIFT: ' + ', '.join(changed))
            raise SystemExit(1)
        print('No inventory drift; not authorization to apply')


if __name__ == '__main__':
    main()
