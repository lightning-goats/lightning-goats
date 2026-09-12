"""Shared fail-closed boundaries for local home deployment helpers."""
import importlib.util
import os
from pathlib import Path
import pwd
import re
import stat
import subprocess
import tomllib
import urllib.request

STEM = 'lightning-goats-gateway-canary'
CONFIG = Path('/etc') / STEM / 'config.toml'
UNIT = Path('/etc/systemd/system') / (STEM + '.service')
BINARY = Path('/usr/local/bin/lightning-goats-gateway')


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def local_opener():
    return urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())


def canonical_files():
    spec = importlib.util.spec_from_file_location('home_install', Path(__file__).with_name('prepare-home-gateway.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    files = module.generated_files(b'fixture')
    return files[str(CONFIG)][0].decode(), files[str(UNIT)][0].decode()


def validate_source(config, unit):
    expected_config, expected_unit = canonical_files()
    if tomllib.loads(config) != tomllib.loads(expected_config):
        raise ValueError('noncanonical canary configuration; separate review required')
    if unit != expected_unit:
        raise ValueError('noncanonical canary unit; separate review required')


def root_regular(path):
    for parent in path.parents:
        info = parent.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError('unsafe installed file parent')
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
        raise ValueError('unsafe installed file')
    return info


def root_file(path):
    root_regular(path)
    return path.read_text()


def properties(name):
    keys = ('FragmentPath', 'DropInPaths', 'NeedDaemonReload', 'ActiveState', 'MainPID',
            'User', 'Group', 'Environment', 'EnvironmentFiles')
    output = subprocess.check_output(['systemctl', 'show', name, *['--property=' + k for k in keys]], text=True)
    return dict(line.split('=', 1) for line in output.splitlines())


def validate_properties(props):
    expected = {'FragmentPath': str(UNIT), 'DropInPaths': '', 'NeedDaemonReload': 'no',
                'User': STEM, 'Group': STEM, 'Environment': 'RUST_LOG=info'}
    if any(props.get(k) != v for k, v in expected.items()) or props.get('EnvironmentFiles', ''):
        raise ValueError('effective canary service drift')


def installed_source():
    config, unit = root_file(CONFIG), root_file(UNIT)
    validate_source(config, unit)
    root_regular(BINARY)
    root_regular(Path("/etc/credstore.encrypted/lightning-goats-gateway-canary-openhab"))
    validate_properties(properties(UNIT.name))
    return config, unit


def verify_running():
    installed_source()
    props = properties(UNIT.name)
    if props.get('ActiveState') != 'active' or not props.get('MainPID', '').isdigit():
        raise ValueError('canary service not active')
    pid = int(props['MainPID'])
    if pid <= 1:
        raise ValueError('missing canary process')
    proc = Path('/proc') / str(pid)
    args = (proc / 'cmdline').read_bytes().split(b'\0')
    if args != [str(BINARY).encode(), b'--config', str(CONFIG).encode(), b'']:
        raise ValueError('canary process command drift')
    if os.readlink(proc / 'exe') != str(BINARY):
        raise ValueError('canary executable drift')
    if proc.stat().st_uid != pwd.getpwnam(STEM).pw_uid:
        raise ValueError('canary process identity drift')
    listeners = subprocess.check_output(['ss', '-H', '-ltnp', 'sport = :8790'], text=True).splitlines()
    if len(listeners) != 1 or listeners[0].split()[3] != '127.0.0.1:8790' or not re.search(r'pid=' + str(pid) + r',', listeners[0]):
        raise ValueError('canary listener not owned by verified service')
