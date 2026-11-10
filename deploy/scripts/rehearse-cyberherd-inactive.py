#!/usr/bin/env python3
"""TEST ONLY: fresh CyberHerd installer on a disposable systemd CI VM.

Creates actual fixed service users, masks and journals on that disposable VM;
no unit starts, credentials, transfers, public posts or feeder calls. Uses a new
synthetic legacy journal in /run, not any original core state. Never use on VPS.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import shutil
import stat
import subprocess
import tarfile
import tempfile
import uuid


def run(*args):
    result = subprocess.run(args, env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'},
                            capture_output=True, text=True, timeout=300)
    if result.returncode:
        # All selected inputs are synthetic. Retain the actual failure in CI.
        raise RuntimeError(f'test-only subprocess failed: {args[0]}\n{result.stdout[-8000:]}\n{result.stderr[-8000:]}')
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('review', type=Path)
    parser.add_argument('source')
    parser.add_argument('fixture_directory', type=Path)
    parser.add_argument('--disposable-ci-host', action='store_true', required=True)
    args = parser.parse_args()
    if (os.geteuid() != 0 or not args.disposable_ci_host or not Path('/run/systemd/system').is_dir()
            or os.environ.get('GITHUB_ACTIONS') != 'true' or not os.environ.get('GITHUB_RUN_ID', '').isdigit()):
        raise ValueError('disposable root systemd CI VM required')
    # Require the private loopback-only namespace used by the other rehearsal.
    # The inherited sysfs mount can still describe the host namespace. Query
    # netlink in this process's namespace instead; do not weaken isolation.
    interfaces = {link['ifname'] for link in json.loads(run('/usr/sbin/ip', '-j', 'link', 'show'))}
    if interfaces != {'lo'} or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('fresh isolated network namespace required')
    core = pwd.getpwnam('lightning-goats')  # Earlier fresh-canary fixture, no activation.
    if core.pw_uid == 0 or core.pw_gid == 0:
        raise ValueError('separate existing core fixture identity required')
    tool_export = json.loads((args.fixture_directory / 'TOOL-EXPORT.json').read_text())
    if tool_export['source_commit'] != args.source:
        raise ValueError('test tool source mismatch')
    fixture = args.fixture_directory / 'TEST-ONLY-prepare-legacy-core'
    source_tools = args.fixture_directory / 'tool-bundle'
    if hashlib.sha256((source_tools / 'launch-vps-upgrade.py').read_bytes()).hexdigest() != tool_export['launcher_sha256']:
        raise ValueError('test launcher hash differs from pinned export')
    root = Path(tempfile.mkdtemp(prefix='lg-cyberherd-install-rehearsal-', dir='/run'))
    os.chown(root, 0, core.pw_gid)
    root.chmod(0o750)
    tools = root / 'tools'
    shutil.copytree(source_tools, tools)
    tools.chmod(0o700)
    for path in tools.iterdir():
        os.chown(path, 0, 0)
        path.chmod(0o600)
    launcher = ['/usr/bin/python3', '-I', '-S', '-B', str(tools / 'launch-vps-upgrade.py'), tool_export['manifest_sha256']]
    # The CI fixture is authenticated by the pinned exported tool closure, not
    # by executing a helper from the review archive.
    sysusers = json.loads(run(*launcher, '--cyberherd-inactive', 'identities', '--create-identities'))
    assert sysusers['services_started'] is False
    runtime = sysusers['runtime']
    source = root / 'source'
    source.mkdir(mode=0o700)
    os.chown(source, core.pw_uid, core.pw_gid)
    installed_fixture = root / 'TEST-ONLY-prepare-legacy-core'
    shutil.copyfile(fixture, installed_fixture)
    installed_fixture.chmod(0o755)
    prepared = json.loads(run('/usr/bin/setpriv', '--reuid=' + str(core.pw_uid), '--regid=' + str(core.pw_gid),
                              '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
                              str(installed_fixture), str(source / 'core.db')))
    assert prepared['synthetic'] is True and prepared['payments'] == 0 and prepared['engine'] == '3.54.1'
    unit = 'lg-cyberherd-fixture-' + uuid.uuid4().hex + '.service'
    mask = Path('/etc/systemd/system') / unit
    mask.symlink_to('/dev/null')
    run('/usr/bin/systemctl', 'daemon-reload')
    policy = root / 'policy.toml'
    policy.write_text(f'version=1\nmax_component_bytes=67108864\n[core]\ndatabase="{source}/core.db"\nuid={core.pw_uid}\ngid={core.pw_gid}\nunit="{unit}"\n')
    policy.chmod(0o600)
    bundle = root / 'legacy'
    accepted = json.loads(run('/usr/local/bin/lightning-goatsctl', 'backup-legacy-core', '--policy', str(policy),
                              '--destination', str(bundle), '--writers-stopped'))
    assert accepted['accepted'] is True and accepted['resume_authorized'] is False
    # Read only the authenticated examples; no Python from the archive executes.
    spec = importlib.util.spec_from_file_location('test_package', tools / 'verify-cyberherd-package.py')
    package = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(package)
    package.verify(args.review, args.source)
    examples = {}
    with tarfile.open(args.review, 'r:gz') as archive:
        for member in archive:
            name = str(Path(member.name))
            if name in {f'deploy/cyberherd/{kind}.toml.example' for kind in ('config', 'payout', 'signer')}:
                examples[name] = archive.extractfile(member).read().decode()
    owner = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
    client = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
    configs = root / 'configs'
    configs.mkdir(mode=0o755)
    configs.chmod(0o755)
    synthetic_key = root / 'TEST-ONLY-payout-key'
    synthetic_key.write_bytes(b'test-only-no-installed-spend-key')
    synthetic_key.chmod(0o600)
    store = Path('/etc/credstore.encrypted')
    if not store.exists():
        store.mkdir(mode=0o700)
    imported = json.loads(run(*launcher, '--cyberherd-payout-key', '--key-file', str(synthetic_key), '--import-key'))
    assert imported['created'] is True and imported['permissions_checked'] is False and imported['payments_sent'] == 0
    assert imported['credential_fingerprint'] == hashlib.sha256(synthetic_key.read_bytes()).hexdigest()
    import grp
    for role, kind in [('cyberherd', 'config'), ('cyberherd-payout', 'payout'), ('cyberherd-signer', 'signer')]:
        text = examples[f'deploy/cyberherd/{kind}.toml.example']
        text = text.replace('credential_fingerprint = "' + '0' * 64 + '"',
                            'credential_fingerprint = "' + imported['credential_fingerprint'] + '"')
        text = text.replace('account_id = "00000000-0000-0000-0000-000000000000"',
                            'account_id = "00000000-0000-0000-0000-000000000001"')
        text = text.replace('0' * 64, owner).replace('REPLACE_WITH_CLIENT_PUBLIC_KEY', client)
        text = text.replace('00000000-0000-0000-0000-000000000000', prepared['epoch'])
        text = text.replace('core_uid = 0', 'core_uid = ' + str(core.pw_uid))
        text = text.replace('intake_uid = 0', 'intake_uid = ' + str(runtime['cyberherd']['uid']))
        text = text.replace('signer_uid = 0', 'signer_uid = ' + str(runtime['cyberherd-signer']['uid']))
        if role == 'cyberherd-signer':
            text = text.replace('peer_uid = 0', 'peer_uid = ' + str(runtime['cyberherd-payout']['uid']))
            text = text.replace('group_gid = 0', 'group_gid = ' + str(grp.getgrnam('cyberherd-payout-signer').gr_gid))
        path = configs / (role + '.toml')
        path.write_text(text)
        path.chmod(0o644)
    base = Path('/var/lib/lightning-goats-upgrades')
    base.mkdir(mode=0o700, exist_ok=False)
    destination = base / 'ci-inactive-install'
    receipt = json.loads(run(*launcher, '--cyberherd-inactive', 'install', str(args.review), str(configs), str(policy), str(bundle), str(destination),
                             '--source', args.source, '--sha256', hashlib.sha256(args.review.read_bytes()).hexdigest(), '--install-disabled'))
    assert receipt['services_masked'] is True and receipt['services_started'] is False and receipt['credentials_loaded'] is False
    for role in runtime:
        state = Path('/var/lib') / role
        assert stat.S_IMODE(state.stat().st_mode) == 0o700 and state.stat().st_uid == runtime[role]['uid']
        assert os.readlink(Path('/etc/systemd/system') / (role + '.service')) == '/dev/null'
        assert (destination / 'INSTALLED.json').exists() and not (destination / 'INCOMPLETE').exists()
    print(json.dumps({'synthetic': True, 'source_commit': args.source, 'native_engine': prepared['engine'],
                      'actual_nss_identities': 3, 'actual_native_initializers': 3, 'actual_masked_units': 3,
                      'synthetic_payout_ciphertext_created': True,
                      'services_started': False, 'credentials_loaded': False, 'provider_qualified': False,
                      'household_qualified': False, 'live_host_qualified': False}, sort_keys=True))


if __name__ == '__main__':
    main()
