#!/usr/bin/env python3
"""Prepare a minimal contact website release, then explicitly switch nginx.

Authenticated root-owned launcher only. Preserve the current public files and
flags, replacing three source-pinned browser files and adding exact contact
ingress. Never open a journal, load credentials, stop an application or create
an invoice. Preparation leaves the new release and snippet unused.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import tomllib

spec = importlib.util.spec_from_file_location('contact_site_maintenance', Path(__file__).with_name('maintain-contact-upgrade.py'))
U = importlib.util.module_from_spec(spec)
spec.loader.exec_module(U)
U.verified_tool_identity = lambda: verified_tool_identity()
C, TX, M = U.C, U.TX, U.M
SITE = Path('/etc/nginx/conf.d/lightning-goats.com.conf')
SNIPPET = Path('/etc/nginx/snippets/lightning-goats-contact.conf')
WWW = Path('/var/www/lightning-goats-releases')
PAYLOAD = {'index.html', 'contact.js', 'contact-fee.js', 'contact.conf'}
ANCHOR = '    include /etc/nginx/snippets/lightning-goats-cyberherd-readonly.conf;\n'


def site_flags(text):
    match = re.fullmatch(r'\s*window\.LightningGoatsSite\s*=\s*Object\.freeze\(\{([^{}]*)\}\);\s*', text)
    if not match:
        raise ValueError('existing site flags must be an explicit boolean object')
    values = {}
    for row in match[1].split(','):
        item = re.fullmatch(r'\s*([A-Za-z][A-Za-z0-9]*)\s*:\s*(true|false)\s*', row)
        if not item or item[1] in values:
            raise ValueError('invalid or duplicate existing site flag')
        values[item[1]] = item[2] == 'true'
    required = {'paymentsEnabled', 'feedingAvailable', 'cyberherdEnabled', 'cyberherdAuthEnabled'}
    if set(values) - {'contactPaymentsEnabled'} != required or values.get('contactPaymentsEnabled', False):
        raise ValueError('requires the existing website with paid contact disabled')
    values['contactPaymentsEnabled'] = True
    return 'window.LightningGoatsSite = Object.freeze({ ' + ', '.join(
        key + ': ' + str(value).lower() for key, value in values.items()) + ' });\n'


def site_change(text, source):
    matches = re.findall(r'^    root ([^;]+);$', text, re.MULTILINE)
    if (len(matches) != 1 or text.count(ANCHOR) != 1 or str(SNIPPET) in text
            or re.fullmatch('[0-9a-f]{40}', source) is None):
        raise ValueError('exact original site root and readonly include required')
    original = Path(matches[0])
    if original.parent != WWW or re.fullmatch(r'(?:contact-)?[0-9a-f]{40}', original.name) is None:
        raise ValueError('original public release must be a fixed release-directory child')
    release = WWW / ('contact-' + source)
    changed = text.replace('    root ' + str(original) + ';', '    root ' + str(release) + ';')
    changed = changed.replace(ANCHOR, ANCHOR + '    include ' + str(SNIPPET) + ';\n')
    return original, release, changed


def inventory(root):
    C.protected(root, directory=True)
    result, total = {}, 0
    for path in sorted(root.rglob('*')):
        info = path.lstat()
        if stat.S_ISDIR(info.st_mode):
            C.protected(path, directory=True)
            continue
        C.protected(path)
        total += info.st_size
        if info.st_size > 4 * 1024 * 1024 or total > 32 * 1024 * 1024 or len(result) >= 512:
            raise ValueError('public release exceeds bounds')
        result[str(path.relative_to(root))] = TX.fingerprint(path)
    if not {'index.html', 'site-config.js', 'overlay.html', 'overlay.js', 'overlay.css'} <= set(result):
        raise ValueError('original public release is incomplete')
    return result


def contact_ready():
    C.protected(C.CONFIG)
    config = tomllib.loads(C.CONFIG.read_text())
    if (config.get('service', {}).get('mode') != 'active'
            or config.get('contact', {}).get('recipient') != config['nostr']['bunker_pubkey']
            or not config['contact'].get('inbox_relays')):
        raise ValueError('reviewed private contact configuration must already be active')
    value = C.status(C.UNIT_NAME)
    if value['ActiveState'] != 'active' or value['SubState'] != 'running' or int(value['MainPID']) < 2:
        raise ValueError('existing receive service must be running')
    return TX.fingerprint(C.CONFIG)


def prepare(payload, expected, directory):
    C.root()
    U.private_directory(directory)
    # private_directory requires the destination to exist, so preparation uses
    # a fresh caller-created private carrier and refuses all pre-existing files.
    if list(directory.iterdir()):
        raise ValueError('empty protected preparation directory required')
    C.protected(payload, directory=True, private=True)
    if {p.name for p in payload.iterdir()} != PAYLOAD | {'SOURCE.json'}:
        raise ValueError('exact four-file source payload required')
    if re.fullmatch('[0-9a-f]{64}', expected) is None or U.digest(payload / 'SOURCE.json') != expected:
        raise ValueError('independent source manifest digest required')
    manifest = C.read_json(payload / 'SOURCE.json')
    if (manifest['version'] != 1 or manifest['source_commit'] != verified_tool_identity()['source_commit']
            or set(manifest['files']) != PAYLOAD):
        raise ValueError('site payload and authenticated tool source must match')
    for name, digest in manifest['files'].items():
        C.protected(payload / name)
        if (payload / name).stat().st_size > 512 * 1024 or U.digest(payload / name) != digest:
            raise ValueError('source-pinned site payload differs')
    core = contact_ready()
    C.protected(SITE)
    site_fp = TX.fingerprint(SITE)
    original, release, text = site_change(SITE.read_text(), manifest['source_commit'])
    C.protected(WWW, directory=True)
    before = inventory(original)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    release.mkdir(mode=0o755)
    release.chmod(0o755)
    for name, fingerprint in before.items():
        destination = release / name
        for parent in reversed(destination.relative_to(release).parents):
            path = release / parent
            if not path.exists():
                path.mkdir(mode=0o755)
                path.chmod(0o755)
        TX.copy_durable(original / name, destination, fingerprint)
    for name in ('index.html', 'contact.js', 'contact-fee.js'):
        destination = release / name
        if destination.exists():
            destination.unlink()
        U.Q.I.publish(destination, (payload / name).read_bytes(), 0o644)
    flags = site_flags((original / 'site-config.js').read_text())
    (release / 'site-config.js').unlink()
    U.Q.I.publish(release / 'site-config.js', flags.encode(), 0o644)
    if SNIPPET.exists() or SNIPPET.is_symlink():
        raise ValueError('contact snippet destination must be unused')
    C.protected(SNIPPET.parent, directory=True)
    U.Q.I.publish(SNIPPET, (payload / 'contact.conf').read_bytes(), 0o644)
    U.Q.I.publish(directory / 'site.conf', text.encode(), 0o600)

    def unchanged():
        if contact_ready() != core or inventory(original) != before or TX.fingerprint(SITE) != site_fp:
            raise ValueError('original website or contact configuration changed')

    rows = TX.prepare(directory / 'configuration', [(SITE, directory / 'site.conf', site_fp)], unchanged)
    unchanged()
    C.write_json(directory / 'PREPARED.json', {'version': 1, 'tool': verified_tool_identity(),
        'source_manifest_sha256': expected, 'core_configuration': core, 'original': str(original),
        'original_files': before, 'release': str(release), 'release_files': inventory(release),
        'snippet': TX.fingerprint(SNIPPET), 'configuration': rows,
        'configuration_manifest_sha256': U.digest(directory / 'configuration/TRANSACTION.json'),
        'services_changed': False, 'contact_site_enabled': False})
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'services_changed': False, 'contact_site_enabled': False}


def apply(directory, expected, approved):
    if not approved:
        raise ValueError('explicit contact site activation required')
    C.root()
    U.private_directory(directory)
    if U.digest(directory / 'PREPARED.json') != expected:
        raise ValueError('independently reviewed site preparation digest required')
    a = C.read_json(directory / 'PREPARED.json')
    if a['version'] != 1 or a['tool'] != verified_tool_identity() or (directory / 'INCOMPLETE').exists():
        raise ValueError('complete matching site preparation required')
    transaction = C.read_json(directory / 'configuration/TRANSACTION.json')
    if (U.digest(directory / 'configuration/TRANSACTION.json') != a['configuration_manifest_sha256']
            or transaction['files'] != a['configuration'] or len(transaction['files']) != 1
            or transaction['files'][0]['destination'] != str(SITE)):
        raise ValueError('only the exact nginx site may be replaced')

    def guard():
        if (contact_ready() != a['core_configuration'] or inventory(Path(a['original'])) != a['original_files']
                or inventory(Path(a['release'])) != a['release_files'] or TX.fingerprint(SNIPPET) != a['snippet']):
            raise ValueError('reviewed contact website or core configuration changed')

    with M.exclusive(directory):
        if (directory / 'ACTIVATION-STARTED.json').exists():
            raise ValueError('site activation already began; inspect retained evidence')
        guard()
        C.command('/usr/sbin/nginx', '-t')
        M.note(directory, 'ACTIVATION-STARTED.json', {'database_opened': False})
        TX.execute(directory / 'configuration', guard)
        try:
            C.command('/usr/sbin/nginx', '-t')
        except Exception:
            TX.execute(directory / 'configuration', guard, rollback=True)
            C.command('/usr/sbin/nginx', '-t')
            M.note(directory, 'CONFIGURATION-RESTORED.json', {'reload_attempted': False})
            raise
        C.command('/usr/bin/systemctl', 'reload', 'nginx.service')
        guard()
        M.note(directory, 'ACTIVATED.json', {'contact_site_enabled': True, 'applications_restarted': False,
                                            'journals_opened': False, 'invoice_created': False})
    return {'contact_site_enabled': True, 'applications_restarted': False, 'journals_opened': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    preparing = commands.add_parser('prepare')
    preparing.add_argument('payload', type=Path)
    preparing.add_argument('directory', type=Path)
    preparing.add_argument('--source-manifest-sha256', required=True)
    applying = commands.add_parser('apply')
    applying.add_argument('directory', type=Path)
    applying.add_argument('--review-sha256', required=True)
    applying.add_argument('--activate-contact-site', action='store_true', required=True)
    args = parser.parse_args()
    if args.operation == 'prepare':
        value = prepare(args.payload, args.source_manifest_sha256, args.directory)
    else:
        value = apply(args.directory, args.review_sha256, args.activate_contact_site)
    print(json.dumps(value, sort_keys=True))


if __name__ == '__main__':
    main()
