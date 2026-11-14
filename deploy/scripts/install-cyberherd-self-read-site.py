#!/usr/bin/env python3
"""Expose only signed self-status reads and enable the existing browser login.

Authenticated root-owned launcher only. Clone the current public release,
preserve its flags/artwork/contact/payment files and serving metadata, replace
one source-pinned auth client, and add one exact GET proxy. Only nginx reloads;
no application restart, journal, credential or financial operation exists.
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
from pathlib import Path
import re
import tomllib

spec = importlib.util.spec_from_file_location('self_read_site_support', Path(__file__).with_name('install-private-contact-site.py'))
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)
P.verified_tool_identity = lambda: verified_tool_identity()
U, C, TX, M = P.U, P.C, P.TX, P.M
SITE = Path('/etc/nginx/conf.d/lightning-goats.com.conf')
SNIPPET = Path('/etc/nginx/snippets/lightning-goats-cyberherd-self-read.conf')
WWW = Path('/var/www/lightning-goats-releases')
ANCHOR = P.ANCHOR
PAYLOAD = {'self-read.conf', 'cyberherd-auth.js'}
BROWSER = {'cyberherd.html', 'cyberherd.js', 'vendor/nostr.bundle.js', 'vendor/nostr-login.js'}


def site_flags(text):
    match = re.fullmatch(r'\s*window\.LightningGoatsSite\s*=\s*Object\.freeze\(\{([^{}]*)\}\);\s*', text)
    if not match:
        raise ValueError('explicit existing boolean site flags required')
    values = {}
    for row in match[1].split(','):
        item = re.fullmatch(r'\s*([A-Za-z][A-Za-z0-9]*)\s*:\s*(true|false)\s*', row)
        if not item or item[1] in values:
            raise ValueError('invalid or duplicate existing site flag')
        values[item[1]] = item[2] == 'true'
    required = {'paymentsEnabled', 'feedingAvailable', 'cyberherdEnabled', 'cyberherdAuthEnabled'}
    if (set(values) - {'contactPaymentsEnabled'} != required
            or values['cyberherdEnabled'] is not True or values['cyberherdAuthEnabled'] is not False):
        raise ValueError('existing readonly CyberHerd site with auth disabled required')
    return re.sub(r'\bcyberherdAuthEnabled(\s*:\s*)false\b', r'cyberherdAuthEnabled\1true', text, count=1)


def site_change(text, source):
    matches = re.findall(r'^    root ([^;]+);$', text, re.MULTILINE)
    if (len(matches) != 1 or text.count(ANCHOR) != 1 or str(SNIPPET) in text
            or re.fullmatch('[0-9a-f]{40}', source) is None):
        raise ValueError('exact original site root and readonly include required')
    original = Path(matches[0])
    if original.parent != WWW or re.fullmatch(r'(?:contact-)?[0-9a-f]{40}', original.name) is None:
        raise ValueError('fixed original release-directory child required')
    release = WWW / ('self-read-' + source)
    changed = text.replace('    root ' + str(original) + ';', '    root ' + str(release) + ';')
    changed = changed.replace(ANCHOR, ANCHOR + '    include ' + str(SNIPPET) + ';\n')
    return original, release, changed


def ready():
    baseline = U.P.baseline(current_schema=True)
    member = tomllib.loads((U.Q.ETC / 'cyberherd/config.toml').read_text())
    if (member.get('view', {}).get('enabled') is not True
            or member['view'].get('listen') != '127.0.0.1:3340'
            or member['view'].get('auth') != {'public_origin': 'https://lightning-goats.com', 'operators': []}):
        raise ValueError('configured loopback self reads with an empty operator allowlist required')
    return baseline


def prepare(payload, expected, directory):
    C.root()
    U.private_directory(directory)
    if list(directory.iterdir()):
        raise ValueError('empty protected preparation directory required')
    C.protected(payload, directory=True, private=True)
    if {p.name for p in payload.iterdir()} != PAYLOAD | {'SOURCE.json'}:
        raise ValueError('exact source-pinned self-read payload required')
    if re.fullmatch('[0-9a-f]{64}', expected) is None or U.digest(payload / 'SOURCE.json') != expected:
        raise ValueError('independent source manifest digest required')
    manifest = C.read_json(payload / 'SOURCE.json')
    if (manifest['version'] != 1 or manifest['source_commit'] != verified_tool_identity()['source_commit']
            or set(manifest['files']) != PAYLOAD or set(manifest['browser_files']) != BROWSER):
        raise ValueError('self-read site payload and authenticated source must match')
    for name, digest in manifest['files'].items():
        C.protected(payload / name)
        if (payload / name).stat().st_size > 512 * 1024 or U.digest(payload / name) != digest:
            raise ValueError('source-pinned self-read payload differs')
    baseline = ready()
    C.protected(SITE)
    site_fp = TX.fingerprint(SITE)
    original, release, text = site_change(SITE.read_text(), manifest['source_commit'])
    C.protected(WWW, directory=True)
    before = P.inventory(original)
    if not BROWSER | {'cyberherd-auth.js'} <= set(before):
        raise ValueError('existing self-read browser assets required')
    for name, digest in manifest['browser_files'].items():
        if not re.fullmatch('[0-9a-f]{64}', digest) or before[name]['sha256'] != digest:
            raise ValueError('retained browser assets differ from qualified source')
    flags = site_flags((original / 'site-config.js').read_text())
    if SNIPPET.exists() or SNIPPET.is_symlink():
        raise ValueError('self-read snippet destination must be unused')
    C.protected(SNIPPET.parent, directory=True)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    release.mkdir(mode=0o755)
    release.chmod(0o755)
    P.copy_selinux_label(original, release)
    for name, fingerprint in before.items():
        destination = release / name
        for parent in reversed(destination.relative_to(release).parents):
            path = release / parent
            if not path.exists():
                path.mkdir(mode=0o755)
                path.chmod(0o755)
                P.copy_selinux_label(original / parent, path)
        TX.copy_durable(original / name, destination, fingerprint)
    # Only these two file hashes may differ. Retain every original owner, mode,
    # ACL/label attribute and all payment/contact/OBS/vendor bytes elsewhere.
    U.Q.I.publish(directory / 'site-config.js', flags.encode(), 0o600)
    for name, source in [('cyberherd-auth.js', payload / 'cyberherd-auth.js'),
                         ('site-config.js', directory / 'site-config.js')]:
        (release / name).unlink()
        TX.copy_durable(source, release / name, dict(before[name], sha256=U.digest(source)))
    U.Q.I.publish(SNIPPET, (payload / 'self-read.conf').read_bytes(), 0o644)
    P.copy_selinux_label(SITE, SNIPPET)
    U.Q.I.publish(directory / 'site.conf', text.encode(), 0o600)

    def unchanged():
        if ready() != baseline or P.inventory(original) != before or TX.fingerprint(SITE) != site_fp:
            raise ValueError('original website or application identity/configuration changed')

    rows = TX.prepare(directory / 'configuration', [(SITE, directory / 'site.conf', site_fp)], unchanged)
    unchanged()
    C.write_json(directory / 'PREPARED.json', {'version': 1, 'tool': verified_tool_identity(),
        'self_read_site_only': True, 'operator_ingress_enabled': False,
        'source_manifest_sha256': expected, 'baseline': baseline, 'original': str(original),
        'original_files': before, 'release': str(release), 'release_files': P.inventory(release),
        'snippet': TX.fingerprint(SNIPPET), 'configuration': rows,
        'configuration_manifest_sha256': U.digest(directory / 'configuration/TRANSACTION.json'),
        'services_changed': False, 'self_read_site_enabled': False})
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'services_changed': False, 'self_read_site_enabled': False,
            'operator_ingress_enabled': False}


def apply(directory, expected, approved):
    if not approved:
        raise ValueError('explicit self-read site activation required')
    C.root()
    U.private_directory(directory)
    if U.digest(directory / 'PREPARED.json') != expected:
        raise ValueError('independently reviewed self-read preparation digest required')
    a = C.read_json(directory / 'PREPARED.json')
    if (a['version'] != 1 or a['tool'] != verified_tool_identity() or (directory / 'INCOMPLETE').exists()
            or a.get('self_read_site_only') is not True or a.get('operator_ingress_enabled') is not False
            or a.get('self_read_site_enabled') is not False or a.get('services_changed') is not False):
        raise ValueError('complete matching self-read-only site preparation required')
    transaction = C.read_json(directory / 'configuration/TRANSACTION.json')
    if (U.digest(directory / 'configuration/TRANSACTION.json') != a['configuration_manifest_sha256']
            or transaction['files'] != a['configuration'] or len(transaction['files']) != 1
            or transaction['files'][0]['destination'] != str(SITE)):
        raise ValueError('only the exact nginx site may be replaced')

    def guard():
        if (ready() != a['baseline'] or P.inventory(Path(a['original'])) != a['original_files']
                or P.inventory(Path(a['release'])) != a['release_files'] or TX.fingerprint(SNIPPET) != a['snippet']):
            raise ValueError('reviewed self-read website or application identity/configuration changed')

    with M.exclusive(directory):
        if (directory / 'ACTIVATION-STARTED.json').exists():
            raise ValueError('self-read site activation already began; inspect retained evidence')
        guard()
        C.command('/usr/sbin/nginx', '-t')
        M.note(directory, 'ACTIVATION-STARTED.json', {'journals_opened': False, 'operator_ingress_enabled': False})
        TX.execute(directory / 'configuration', guard)
        reload_attempted = False
        try:
            C.command('/usr/sbin/nginx', '-t')
            reload_attempted = True
            C.command('/usr/bin/systemctl', 'reload', 'nginx.service')
        except Exception:
            TX.execute(directory / 'configuration', guard, rollback=True)
            C.command('/usr/sbin/nginx', '-t')
            if reload_attempted:
                C.command('/usr/bin/systemctl', 'reload', 'nginx.service')
            M.note(directory, 'CONFIGURATION-RESTORED.json', {'reload_attempted': reload_attempted,
                'only_nginx_site_restored': True, 'applications_restarted': False})
            raise
        guard()
        M.note(directory, 'ACTIVATED.json', {'self_read_site_enabled': True, 'operator_ingress_enabled': False,
            'applications_restarted': False, 'journals_opened': False, 'invoice_created': False})
    return {'self_read_site_enabled': True, 'operator_ingress_enabled': False,
            'applications_restarted': False, 'journals_opened': False}


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
    applying.add_argument('--activate-self-read-site', required=True, action='store_true')
    args = parser.parse_args()
    if args.operation == 'prepare':
        result = prepare(args.payload, args.source_manifest_sha256, args.directory)
    else:
        result = apply(args.directory, args.review_sha256, args.activate_self_read_site)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
