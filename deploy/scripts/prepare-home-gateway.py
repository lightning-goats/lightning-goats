#!/usr/bin/env python3
"""Fresh, inactive home installation. Plan by default; never activates any service.

Run from a reviewed checkout after exact-source CI/Security and local build gates.
Existing installations require a separately reviewed upgrade, not this helper.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import grp
import stat
import subprocess

ROOT = Path(__file__).resolve().parents[2]
USERS = ('lightning-goats-gateway', 'lightning-goats-gateway-canary')


def generated_files(binary):
    files = {'/usr/local/bin/lightning-goats-gateway': (binary, 0o755)}
    for canary, user in zip((False, True), USERS):
        suffix = '.canary' if canary else ''
        config = (ROOT / f'deploy/gateway/config{suffix}.toml.example').read_text()
        # Both are loopback until a separately reviewed network transition.
        config = config.replace('10.8.0.6:', '127.0.0.1:')
        config = '\n'.join(line for line in config.splitlines()
                           if not line.startswith('temperature_item =')) + '\n'
        if canary:
            config = config.replace('override_item = "FeederOverride"',
                                    'override_item = "LightningGoatsCanaryOverride"')
        files[f'/etc/{user}/config.toml'] = (config.encode(), 0o644)
        unit_name = user + '.service'
        unit = (ROOT / 'deploy/systemd' / unit_name).read_text()
        if canary:
            unit = unit.replace('User=lightning-goats-gateway\n', f'User={user}\n')
            unit = unit.replace('Group=lightning-goats-gateway\n', f'Group={user}\n')
            unit = unit.replace('/etc/lightning-goats-gateway/config.canary.toml',
                                f'/etc/{user}/config.toml')
            unit = unit.replace('ConfigurationDirectory=lightning-goats-gateway\n',
                                f'ConfigurationDirectory={user}\n')
            unit = unit.replace('lightning-goats-gateway-openhab',
                                'lightning-goats-gateway-canary-openhab')
        unit = '\n'.join(line for line in unit.splitlines()
                         if not line.startswith('ConfigurationDirectory=')
                         and line != 'IPAddressAllow=10.8.0.0/24') + '\n'
        files[f'/etc/systemd/system/{unit_name}'] = (unit.encode(), 0o644)
    return files


def safe_parent(path):
    for parent in path.parents:
        if parent.exists() or parent.is_symlink():
            st = parent.lstat()
            if not stat.S_ISDIR(st.st_mode) or st.st_uid != 0 or st.st_mode & 0o022:
                raise ValueError(f'unsafe installation parent: {parent}')


def preflight(files):
    for user in USERS:
        for lookup in (pwd.getpwnam, grp.getgrnam):
            try:
                lookup(user)
            except KeyError:
                continue
            raise ValueError(f'existing account/group requires upgrade review: {user}')
        for path in (Path('/etc') / user, Path('/var/lib') / user,
                     Path('/run') / user,
                     Path('/etc/systemd/system') / (user + '.service.d'),
                     Path('/etc/credstore.encrypted') / (user + '-openhab')):
            if path.exists() or path.is_symlink():
                raise ValueError(f'existing project resource: {path}')
        loaded = subprocess.check_output(['systemctl', 'show', user + '.service',
                                         '-p', 'LoadState', '--value'], text=True).strip()
        if loaded != 'not-found':
            raise ValueError(f'existing service requires upgrade review: {user}')
    for name in files:
        path = Path(name)
        safe_parent(path)
        if path.exists() or path.is_symlink():
            raise ValueError(f'refusing to replace {path}')


def install(files):
    # Preflight all resources before creating any; partial failure stays inactive.
    preflight(files)
    for user in USERS:
        subprocess.run(['useradd', '--system', '--user-group', '--no-create-home',
                        '--home-dir', '/nonexistent', '--shell', '/usr/sbin/nologin', user], check=True)
        Path('/etc', user).mkdir(mode=0o755)
        state = Path('/var/lib', user)
        state.mkdir(mode=0o700)
        account = pwd.getpwnam(user)
        os.chown(state, account.pw_uid, account.pw_gid)
    for name, (content, mode) in files.items():
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
        with os.fdopen(fd, 'wb') as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
    subprocess.run(['systemctl', 'daemon-reload'], check=True)
    for user in USERS:
        state = subprocess.check_output(['systemctl', 'show', user + '.service',
                                         '-p', 'ActiveState', '--value'], text=True).strip()
        if state != 'inactive':
            raise ValueError(f'unexpected service state: {user} {state}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--sha256', required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    if args.binary.is_symlink() or not args.binary.is_file():
        raise ValueError('binary must be a regular non-symlink file')
    binary = args.binary.read_bytes()
    if hashlib.sha256(binary).hexdigest() != args.sha256:
        raise ValueError('binary hash mismatch')
    if not binary.startswith(b'\x7fELF'):
        raise ValueError('binary is not ELF')
    files = generated_files(binary)
    plan = {name: {'sha256': hashlib.sha256(data).hexdigest(), 'mode': oct(mode)}
            for name, (data, mode) in files.items()}
    if args.apply:
        if os.geteuid() != 0:
            raise ValueError('--apply requires root')
        install(files)
    print(json.dumps({'applied': args.apply, 'services_started': False,
                      'services_enabled': False, 'files': plan}, indent=2))


if __name__ == '__main__':
    main()
