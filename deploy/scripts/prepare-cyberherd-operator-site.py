#!/usr/bin/env python3
"""Prepare a separate operator-site transaction; no activation operation.

Authenticated root launcher only. Retains the existing signed self-read route,
project allowlist, three seats, application processes and all financial limits.
Public activation additionally requires installed WAF/native authentication tests.
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

spec = importlib.util.spec_from_file_location('operator_site_support', Path(__file__).with_name('install-private-contact-site.py'))
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)
P.verified_tool_identity = lambda: verified_tool_identity()
U, C, TX, M = P.U, P.C, P.TX, P.M
SITE = Path('/etc/nginx/conf.d/lightning-goats.com.conf')
SNIPPET = Path('/etc/nginx/snippets/lightning-goats-cyberherd-operator.conf')
SELF_READ = Path('/etc/nginx/snippets/lightning-goats-cyberherd-self-read.conf')
WWW = Path('/var/www/lightning-goats-releases')
ANCHOR = P.ANCHOR
PAYLOAD = {'operator.conf', 'cyberherd-operator.html', 'cyberherd-operator.js'}
BROWSER = {'cyberherd.html', 'cyberherd.js', 'cyberherd-auth.js', 'vendor/nostr.bundle.js', 'vendor/nostr-login.js'}


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
            or values['cyberherdEnabled'] is not True or values['cyberherdAuthEnabled'] is not True):
        raise ValueError('existing signed self-read site required')
    return text


def site_change(text, source):
    matches = re.findall(r'^    root ([^;]+);$', text, re.MULTILINE)
    if (len(matches) != 1 or text.count(ANCHOR) != 1 or str(SNIPPET) in text
            or text.count('    include ' + str(SELF_READ) + ';\n') != 1
            or re.fullmatch('[0-9a-f]{40}', source) is None):
        raise ValueError('exact original site root and readonly include required')
    original = Path(matches[0])
    if original.parent != WWW or re.fullmatch(r'self-read-[0-9a-f]{40}', original.name) is None:
        raise ValueError('fixed original release-directory child required')
    release = WWW / ('operator-' + source)
    changed = text.replace('    root ' + str(original) + ';', '    root ' + str(release) + ';')
    changed = changed.replace(ANCHOR, ANCHOR + '    include ' + str(SNIPPET) + ';\n')
    return original, release, changed


def ready():
    baseline = U.P.baseline(current_schema=True)
    member = tomllib.loads((U.Q.ETC / 'cyberherd/config.toml').read_text())
    if (member.get('settings', {}).get('max_members') != 3
            or member.get('view', {}).get('enabled') is not True
            or member['view'].get('listen') != '127.0.0.1:3340'
            or member['view'].get('auth') != {'public_origin': 'https://lightning-goats.com', 'operators': [member['project_pubkey']]}):
        raise ValueError('configured three-seat project operator with signed self reads required')
    return baseline


def prepare(payload, expected, directory):
    C.root()
    U.private_directory(directory)
    if list(directory.iterdir()):
        raise ValueError('empty protected preparation directory required')
    C.protected(payload, directory=True, private=True)
    if {p.name for p in payload.iterdir()} != PAYLOAD | {'SOURCE.json'}:
        raise ValueError('exact source-pinned operator payload required')
    if re.fullmatch('[0-9a-f]{64}', expected) is None or U.digest(payload / 'SOURCE.json') != expected:
        raise ValueError('independent source manifest digest required')
    manifest = C.read_json(payload / 'SOURCE.json')
    if (manifest['version'] != 1 or manifest['source_commit'] != verified_tool_identity()['source_commit']
            or set(manifest['files']) != PAYLOAD or set(manifest['browser_files']) != BROWSER):
        raise ValueError('self-read site payload and authenticated source must match')
    for name, digest in manifest['files'].items():
        C.protected(payload / name)
        if (payload / name).stat().st_size > 512 * 1024 or U.digest(payload / name) != digest:
            raise ValueError('source-pinned operator payload differs')
    baseline = ready()
    C.protected(SITE)
    C.protected(SELF_READ)
    self_read_fingerprint = TX.fingerprint(SELF_READ)
    site_fp = TX.fingerprint(SITE)
    original, release, text = site_change(SITE.read_text(), manifest['source_commit'])
    C.protected(WWW, directory=True)
    before = P.inventory(original)
    if not BROWSER | {'cyberherd-operator.html', 'cyberherd-operator.js'} <= set(before):
        raise ValueError('existing self-read browser assets required')
    for name, digest in manifest['browser_files'].items():
        if not re.fullmatch('[0-9a-f]{64}', digest) or before[name]['sha256'] != digest:
            raise ValueError('retained browser assets differ from qualified source')
    flags = site_flags((original / 'site-config.js').read_text())
    if SNIPPET.exists() or SNIPPET.is_symlink():
        raise ValueError('operator snippet destination must be unused')
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
    # Preserve all flags and unrelated payment/contact/OBS/vendor bytes.
    for name in ('cyberherd-operator.html', 'cyberherd-operator.js'):
        source = payload / name
        (release / name).unlink()
        TX.copy_durable(source, release / name, dict(before[name], sha256=U.digest(source)))
    U.Q.I.publish(SNIPPET, (payload / 'operator.conf').read_bytes(), 0o644)
    P.copy_selinux_label(SITE, SNIPPET)
    U.Q.I.publish(directory / 'site.conf', text.encode(), 0o600)

    def unchanged():
        if (ready() != baseline or P.inventory(original) != before or TX.fingerprint(SITE) != site_fp
                or TX.fingerprint(SELF_READ) != self_read_fingerprint):
            raise ValueError('original website or application identity/configuration changed')

    rows = TX.prepare(directory / 'configuration', [(SITE, directory / 'site.conf', site_fp)], unchanged)
    unchanged()
    C.write_json(directory / 'PREPARED.json', {'version': 1, 'tool': verified_tool_identity(),
        'operator_site_preparation_only': True, 'operator_ingress_enabled': False,
        'source_manifest_sha256': expected, 'baseline': baseline, 'original': str(original),
        'original_files': before, 'release': str(release), 'release_files': P.inventory(release),
        'snippet': TX.fingerprint(SNIPPET), 'configuration': rows,
        'retained_self_read_snippet': self_read_fingerprint,
        'configuration_manifest_sha256': U.digest(directory / 'configuration/TRANSACTION.json'),
        'services_changed': False, 'operator_site_enabled': False})
    (directory / 'LOCK').touch(mode=0o600, exist_ok=False)
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'services_changed': False, 'operator_site_enabled': False,
            'operator_ingress_enabled': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('payload', type=Path)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--source-manifest-sha256', required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(args.payload, args.source_manifest_sha256, args.directory), sort_keys=True))


if __name__ == '__main__':
    main()
