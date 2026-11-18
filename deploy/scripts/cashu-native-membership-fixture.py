"""Disposable native CyberHerd journals for the combined-backup fixtures.

Initialization commands only; disabled configuration and zero payout/signature
budgets. Never supplies credentials, starts workers or touches host services.
"""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import tomllib
import uuid


def checksum(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(source, root, states, uids, call, core_epoch, owner, client):
    zero = '0' * 64
    account = str(uuid.uuid4())
    configurations = {}
    replacements = {
        'membership': {
            '"/var/lib/cyberherd"': json.dumps(str(states['membership'])),
            '"/run/lightning-goats-bridge/core.sock"': json.dumps(str(root / 'unused-core.sock')),
            zero: owner, 'core_uid = 0': f"core_uid = {uids['core']}",
            '00000000-0000-0000-0000-000000000000': core_epoch,
        },
        'payout': {
            '"/var/lib/cyberherd-payout"': json.dumps(str(states['payout'])),
            '"/run/lightning-goats-bridge/core.sock"': json.dumps(str(root / 'unused-core.sock')),
            '"/run/cyberherd-plans/plans.sock"': json.dumps(str(root / 'unused-plans.sock')),
            '"/run/cyberherd-signer/signer.sock"': json.dumps(str(root / 'unused-payout-sign.sock')),
            'intake_uid = 0': f"intake_uid = {uids['membership']}",
            'signer_uid = 0': f"signer_uid = {uids['payout_signer']}",
            'core_uid = 0': f"core_uid = {uids['core']}",
            'epoch = "00000000-0000-0000-0000-000000000000"': f'epoch = "{core_epoch}"',
            'account_id = "00000000-0000-0000-0000-000000000000"': f'account_id = "{account}"',
            f'owner = "{zero}"': f'owner = "{owner}"',
            f'credential_fingerprint = "{zero}"': 'credential_fingerprint = "' + 'aa' * 32 + '"',
        },
        'payout_signer': {
            '"/var/lib/cyberherd-signer"': json.dumps(str(states['payout_signer'])),
            '"/var/lib/cyberherd-signer/nak"': json.dumps(str(states['payout_signer'] / 'nak')),
            '"/run/cyberherd-signer/signer.sock"': json.dumps(str(root / 'unused-payout-sign.sock')),
            '"/usr/local/bin/nak"': json.dumps(str(root / 'unused-nak')),
            'group_gid = 0': f"group_gid = {uids['payout_signer']}",
            'peer_uid = 0': f"peer_uid = {uids['payout']}",
            zero: owner, 'REPLACE_WITH_CLIENT_PUBLIC_KEY': client,
        },
    }
    templates = {'membership': 'config.toml.example', 'payout': 'payout.toml.example',
                 'payout_signer': 'signer.toml.example'}
    for name, template in templates.items():
        text = (source / 'deploy/cyberherd' / template).read_text()
        for old, new in replacements[name].items():
            if old not in text:
                raise ValueError('native fixture configuration anchor changed')
            text = text.replace(old, new)
        path = root / (name + '.toml')
        path.write_text(text)
        path.chmod(0o444)
        call(name, uids[name], '--config', path, '--initialize-state')
        configurations[name] = path
    policy = {'version': 1, 'owner': owner, 'epoch': core_epoch,
              'provider': {'environment': 'strike-production', 'account_id': account,
                           'credential_fingerprint': 'aa' * 32},
              'max_component_bytes': 128 * 1024 * 1024, 'components': {}}
    for role, name, filename in [('core', 'core', 'ledger.sqlite'),
                                 ('membership', 'membership', 'cyberherd.db'),
                                 ('payout', 'payout', 'payout.db'),
                                 ('signer', 'payout_signer', 'signer.db')]:
        policy['components'][role] = {'database': str(states[name] / filename),
                                      'uid': uids[name], 'gid': uids[name],
                                      'unit': f'{root.name}-{role}.service'}
    return configurations, policy


def combine(root, snapshot, policy, statectl, statectl_sha256, verify):
    recovery = snapshot.parent
    native = recovery / 'native'
    native.mkdir(mode=0o700)
    inventory = json.loads((snapshot / 'inventory.json').read_bytes())
    images = []
    for role, component in policy['components'].items():
        target = native / (role + '.db')
        if role == 'core':
            row = next(r for r in inventory['components'] if r['role'] == 'core_ledger')
            shutil.copyfile(snapshot / row['path'], target)
        else:
            with closing(sqlite3.connect(Path(component['database']).as_uri() + '?mode=ro', uri=True)) as source:
                with closing(sqlite3.connect(target)) as destination:
                    source.backup(destination)
        target.chmod(0o600)
        images.append({'name': target.name, 'bytes': target.stat().st_size,
                       'sha256': checksum(target)})
    policy_path = recovery / 'maintenance-policy.toml'
    lines = [f'version = {policy["version"]}', f'owner = "{policy["owner"]}"',
             f'epoch = "{policy["epoch"]}"', f'max_component_bytes = {policy["max_component_bytes"]}',
             '', '[provider]']
    for key, value in policy['provider'].items():
        lines.append(f'{key} = {json.dumps(value)}')
    for role, component in policy['components'].items():
        lines.extend(['', f'[components.{role}]'])
        for key, value in component.items():
            lines.append(f'{key} = {json.dumps(value)}')
    policy_path.write_text('\n'.join(lines) + '\n')
    policy_path.chmod(0o600)
    if tomllib.loads(policy_path.read_text()) != policy:
        raise ValueError('native fixture policy serialization differs')
    manifest = {'version': 2, 'purpose': 'cashu_four_journal_correlation_review',
                'id': inventory['checkpoint_id'], 'policy': policy,
                'source_policy_sha256': checksum(policy_path),
                'cashu_inventory_sha256': checksum(snapshot / 'inventory.json'), 'images': images}
    native_manifest = native / 'MANIFEST.json'
    native_manifest.write_text(json.dumps(manifest))
    native_manifest.chmod(0o600)
    envelope = {'version': 1, 'checkpoint_id': inventory['checkpoint_id'],
                'cashu_inventory_sha256': manifest['cashu_inventory_sha256'],
                'native_manifest_sha256': checksum(native_manifest),
                'maintenance_policy_sha256': checksum(policy_path)}
    combined = recovery / 'combined.json'
    combined.write_text(json.dumps(envelope))
    combined.chmod(0o600)
    result = verify(recovery, statectl, statectl_sha256)
    # Native schema checks must distinguish payout signing from Cashu signing.
    payout_signer = native / 'signer.db'
    original = payout_signer.read_bytes()
    cashu_signer = next(row for row in inventory['components'] if row['role'] == 'signing_broker')
    payout_signer.write_bytes((snapshot / cashu_signer['path']).read_bytes())
    image = next(row for row in manifest['images'] if row['name'] == 'signer.db')
    image['bytes'], image['sha256'] = payout_signer.stat().st_size, checksum(payout_signer)
    original_manifest = native_manifest.read_bytes()
    original_envelope = combined.read_bytes()
    native_manifest.write_text(json.dumps(manifest))
    envelope['native_manifest_sha256'] = checksum(native_manifest)
    combined.write_text(json.dumps(envelope))
    try:
        verify(recovery, statectl, statectl_sha256)
    except ValueError:
        pass
    else:
        raise ValueError('Cashu broker substituted for payout signer was accepted')
    payout_signer.write_bytes(original)
    native_manifest.write_bytes(original_manifest)
    combined.write_bytes(original_envelope)
    verify(recovery, statectl, statectl_sha256)
    result['cashu_and_payout_signer_substitution_refused'] = True
    result['funded_restore_qualified'] = False
    return result
