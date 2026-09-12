#!/usr/bin/env python3
"""Provision fresh project USER accounts via the local supported Karaf console.

Requires root, pexpect, a protected console-password file and pinned known-hosts.
Plan by default. Never prints token/password, passes secrets in argv, changes an
existing user/token or writes OpenHAB JSONDB. Token output is encrypted directly.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import home_gateway_safety as safety
import pwd
import re
import secrets
import stat
import subprocess
import tempfile
import urllib.error
import urllib.request

ACCOUNTS = [('lightning_goats_gateway', 'lightninggoatsgateway', 'lightning-goats-gateway'),
            ('lightning_goats_gateway_canary', 'lightninggoatsgatewaycanary', 'lightning-goats-gateway-canary')]


def private_file(path):
    st = path.lstat()
    if not stat.S_ISREG(st.st_mode) or st.st_uid != 0 or st.st_mode & 0o077:
        raise ValueError('input must be a root-owned private regular file')
    return path.read_text().strip()


def metadata():
    # Read only metadata for preflight/readback, never modify the backing store.
    data = json.loads(Path('/var/lib/openhab/jsondb/users.json').read_text())
    return {key: value['value']['roles'] for key, value in data.items()}


def apply(password_file, known_hosts, resume=False):
    import pexpect
    if os.geteuid() != 0:
        raise ValueError('provisioning requires root')
    password = private_file(password_file)
    private_file(known_hosts)
    key = Path('/var/lib/systemd/credential.secret').lstat()
    if not stat.S_ISREG(key.st_mode) or key.st_uid != 0 or key.st_mode & 0o077:
        raise ValueError('protected existing systemd host key required')
    users = metadata()
    for user, label, stem in ACCOUNTS:
        if user in users:
            raw = json.loads(Path('/var/lib/openhab/jsondb/users.json').read_text())[user]['value']
            if not resume or users[user] != ['user'] or raw.get('apiTokens') or raw.get('sessions'):
                raise ValueError('existing project user requires separate recovery/rotation review')
        target = Path('/etc/credstore.encrypted') / (stem + '-openhab')
        if target.exists() or target.is_symlink():
            raise ValueError('existing ciphertext requires rotation review')
    directory = Path(tempfile.mkdtemp(prefix='lg-console-', dir='/run'))
    directory.chmod(0o711)
    command_file = directory / 'commands'
    commands = []
    for user, label, stem in ACCOUNTS:
        if user not in users:
            commands.append(f'openhab:users add {user} {secrets.token_hex(32)} user')
        commands.append(f"openhab:users addApiToken {user} {label} ''")
    fd = os.open(command_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write('\n'.join(commands) + '\n')
    account = pwd.getpwnam('openhab')
    os.chown(command_file, account.pw_uid, account.pw_gid)
    child = None
    try:
        child = pexpect.spawn('/usr/bin/ssh', ['-o', 'StrictHostKeyChecking=yes',
            '-o', 'UserKnownHostsFile=' + str(known_hosts), '-o', 'NumberOfPasswordPrompts=1',
            '-p', '8101', 'openhab@127.0.0.1', 'shell:source ' + str(command_file)],
            encoding='utf-8', timeout=30)
        child.logfile = None
        child.expect('[Pp]assword:')
        child.sendline(password)
        child.expect(pexpect.EOF)
        tokens = re.findall(r'oh\.[A-Za-z0-9._-]+', child.before)
        if len(tokens) != 2:
            raise ValueError('console did not return two tokens; inspect project account metadata before recovery')
        current = metadata()
        results = []
        for (user, label, stem), token in zip(ACCOUNTS, tokens):
            if current.get(user) != ['user']:
                raise ValueError('runtime user has unexpected role; token not installed')
            headers = {'Authorization': 'Basic ' + base64.b64encode((token + ':').encode()).decode()}
            statuses = []
            for path in ('items/LightningGoatsCanaryOverride/state', 'rules'):
                request = urllib.request.Request('http://127.0.0.1:8080/rest/' + path, headers=headers)
                opener = safety.local_opener()
                try:
                    with opener.open(request, timeout=5) as response:
                        statuses.append(response.status)
                except urllib.error.HTTPError as error:
                    statuses.append(error.code)
            if statuses[0] != 200 or statuses[1] not in (401, 403):
                raise ValueError('runtime token effective permissions failed')
            target = Path('/etc/credstore.encrypted') / (stem + '-openhab')
            subprocess.run(['systemd-creds', 'encrypt', '--with-key=host', '--name=openhab-token',
                            '-', str(target)], input=token.encode(), check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            target.chmod(0o600)
            results.append({'user': user, 'label': label, 'role': current[user],
                            'item_read_http': statuses[0], 'admin_read_http': statuses[1],
                            'ciphertext': str(target), 'credential_name': 'openhab-token'})
        print(json.dumps({'accounts': results, 'tokens_shared': False}, indent=2))
    finally:
        if child is not None:
            child.close()
        command_file.unlink(missing_ok=True)
        directory.rmdir()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--console-password-file', required=True, type=Path)
    parser.add_argument('--known-hosts', required=True, type=Path)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--resume-empty-project-users', action='store_true',
                        help='Only explicitly reviewed existing USER accounts with no tokens/sessions')
    args = parser.parse_args()
    try:
        if args.apply:
            apply(args.console_password_file, args.known_hosts, args.resume_empty_project_users)
        else:
            print(json.dumps({'apply': False, 'accounts': ACCOUNTS, 'role': 'user',
                              'console': '127.0.0.1:8101', 'activation': False}))
    except Exception as error:
        # Never allow pexpect/subprocess exceptions to print buffered secret output.
        print('Provisioning stopped: ' + type(error).__name__ + '. Inspect sanitized account metadata; do not overwrite resources.')
        raise SystemExit(1) from None
