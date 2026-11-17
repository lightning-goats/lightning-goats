#!/usr/bin/env python3
"""Inventory both locked Rust graphs using checksum-verified cached crate archives.

No downloads or builds. Missing archives remain explicit; a lockfile inventory
includes optional/target packages and does not claim all are linked or shipped.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile
import tomllib


def digest(data):
    return hashlib.sha256(data).hexdigest()


def license_name(path):
    return any(part.lower().startswith(('license', 'licence', 'copying', 'notice'))
               for part in path.parts)


def authors_license(path, data):
    # r-efi ships its actual permission/copyright notice in AUTHORS. A bare
    # contributor list must not count as a license notice merely by its name.
    return (path.name.upper() == 'AUTHORS'
            and b'Permission is hereby granted, free of charge' in data
            and b'THE SOFTWARE IS PROVIDED "AS IS"' in data
            and b'COPYRIGHT:' in data)


def registry_record(package, cache):
    filename = f"{package['name']}-{package['version']}.crate"
    candidates = sorted(cache.glob('*/' + filename))
    archive = next((p for p in candidates if digest(p.read_bytes()) == package['checksum']), None)
    if archive is None:
        return {'status': 'missing_verified_archive', 'checksum': package['checksum']}
    prefix = Path(f"{package['name']}-{package['version']}")
    files = {}
    manifest = None
    vcs = None
    with tarfile.open(archive, 'r:gz') as bundle:
        for entry in bundle:
            path = Path(entry.name)
            if path.is_absolute() or '..' in path.parts:
                raise ValueError('unsafe archive path')
            relative = path.relative_to(prefix)
            if entry.isdir():
                continue
            if not entry.isfile():
                raise ValueError('nonregular archive member')
            if (relative not in [Path('Cargo.toml'), Path('.cargo_vcs_info.json')]
                    and not license_name(relative) and relative.name.upper() != 'AUTHORS'):
                continue
            if entry.size > 1024 * 1024:
                raise ValueError('oversized manifest/license')
            data = bundle.extractfile(entry).read()
            if relative == Path('Cargo.toml'):
                manifest = tomllib.loads(data.decode())['package']
            elif relative == Path('.cargo_vcs_info.json'):
                vcs = json.loads(data)
            elif license_name(relative) or authors_license(relative, data):
                files[str(relative)] = digest(data)
    if manifest is None or manifest['name'] != package['name'] or manifest['version'] != package['version']:
        raise ValueError('archive package identity mismatch')
    return {'status': 'verified_archive', 'checksum': package['checksum'],
            'declared_license': manifest.get('license'), 'license_file': manifest.get('license-file'),
            'license_notice_files_sha256': files,
            'archive_repository': manifest.get('repository'),
            'archive_vcs_info': vcs}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cache', type=Path, default=Path.home() / '.cargo/registry/cache')
    parser.add_argument('--require-complete', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    lock_paths = ['Cargo.lock', 'tools/cashu-compat/Cargo.lock']
    records = {}
    locks = {}
    local = {}
    manifests = [root / 'Cargo.toml', root / 'tools/cashu-compat/Cargo.toml']
    manifests += list((root / 'crates').glob('*/Cargo.toml'))
    manifests += list((root / 'vendor').glob('*/Cargo.toml'))
    manifests += list((root / 'tools/cashu-compat/vendor').glob('*/Cargo.toml'))
    for path in manifests:
        package = tomllib.loads(path.read_text()).get('package', {})
        if 'name' in package:
            local[(package['name'], package['version'])] = (path, package)
    for lock_path in lock_paths:
        raw = (root / lock_path).read_bytes()
        locks[lock_path] = digest(raw)
        for package in tomllib.loads(raw.decode())['package']:
            identity = (package['name'], package['version'], package.get('source', 'local'))
            if identity not in records:
                if identity[2] == 'registry+https://github.com/rust-lang/crates.io-index':
                    record = registry_record(package, args.cache)
                elif identity[2] == 'local' and identity[:2] in local:
                    path, meta = local[identity[:2]]
                    record = {'status': 'local_manifest', 'manifest': str(path.relative_to(root)),
                              'manifest_sha256': digest(path.read_bytes()),
                              'declared_license': meta.get('license'), 'license_file': meta.get('license-file'),
                              'license_notice_files_sha256': {
                                  str(p.relative_to(root)): digest(p.read_bytes())
                                  for p in sorted(path.parent.iterdir()) if p.is_file() and license_name(p)}}
                    if package['name'] in {
                        'lightning-goats', 'lightning-goats-cashu-compat',
                        'lightning-goats-cashu-receipt', 'lightning-goats-nostr-event',
                    }:
                        project_license = root / 'LICENSE'
                        record['license_notice_files_sha256']['LICENSE'] = digest(project_license.read_bytes())
                else:
                    record = {'status': 'unresolved_source'}
                records[identity] = dict(name=identity[0], version=identity[1], source=identity[2], graphs=[], **record)
            records[identity]['graphs'].append(lock_path)
    rows = [records[key] for key in sorted(records)]
    unresolved = [r['name'] + '@' + r['version'] for r in rows
                  if r['status'] not in ['verified_archive', 'local_manifest']
                  or not (r.get('declared_license') or r.get('license_file'))]
    result = {'schema_version': 1, 'lock_sha256': locks, 'scope': 'full lockfile graphs, not a linked-binary inventory',
              'complete': not unresolved, 'unresolved': unresolved,
              'notice_files_missing': [r['name'] + '@' + r['version'] for r in rows
                                       if not r['license_notice_files_sha256']],
              'distribution_notices_complete': False, 'packages': rows}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'packages': len(rows), 'unresolved': len(unresolved), 'complete': not unresolved}))
    if args.require_complete and unresolved:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
