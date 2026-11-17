#!/usr/bin/env python3
"""Export inventoried license texts from checksum-verified archives, without downloads."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import tarfile
import tempfile
from urllib.parse import urlsplit


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git_blob(data):
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()


def github_repository(value):
    """Cargo repository metadata may point to a crate's workspace subdirectory."""
    url = urlsplit(value)
    parts = url.path.strip('/').split('/')
    if (url.scheme != 'https' or url.netloc != 'github.com' or url.query or url.fragment
            or len(parts) < 2 or any(p in ('', '.', '..') for p in parts)
            or (len(parts) > 2 and parts[2] != 'tree')):
        raise ValueError('unsupported upstream notice repository')
    return 'https://github.com/' + parts[0] + '/' + parts[1].removesuffix('.git')


def source_origin(root, package):
    if package['status'] == 'verified_archive':
        return ((package.get('archive_vcs_info') or {}).get('git', {}).get('sha1'),
                package.get('archive_repository'), package['checksum'])
    # The reviewed CDK backport remains a local package. Its upstream notice
    # authority comes from its original archive and exact allowed source patch.
    manifest = 'tools/cashu-compat/vendor/cdk-0.18.3/Cargo.toml'
    if (package['status'] != 'local_manifest' or package['name'] != 'cdk'
            or package['version'] != '0.18.3' or package.get('manifest') != manifest):
        raise ValueError('no reviewed upstream origin for this local package')
    if digest((root / manifest).read_bytes()) != package['manifest_sha256']:
        raise ValueError('local CDK manifest differs from inventory')
    verifier = runpy.run_path(str(root / 'tools/cashu-compat/verify_vendor.py'))
    verifier['verify'](root / 'tools/cashu-compat')
    archive = root / 'tools/cashu-compat/vendor/cdk-0.18.3.crate'
    with tarfile.open(archive, 'r:gz') as bundle:
        vcs = json.load(bundle.extractfile('cdk-0.18.3/.cargo_vcs_info.json'))
    return (vcs['git']['sha1'], 'https://github.com/cashubtc/cdk',
            digest(archive.read_bytes()))


def release_origin(package, row, cache):
    """Recheck a reviewed signed release match for an archive missing VCS metadata."""
    origin = row['release_source']
    identity = package['name'] + '@' + package['version']
    archive_name = identity.replace('@', '-') + '.crate'
    if (package['status'] != 'verified_archive' or package.get('archive_vcs_info') is not None
            or cache is None or set(origin) != {'tag', 'tag_object', 'git_commit', 'git_tree', 'member_git_blobs'}
            or origin['tag'] != identity.replace('@', '-')
            or row['commit'] != origin['git_commit']
            or any(not re.fullmatch('[a-f0-9]{40}', value) for value in
                   (origin['tag_object'], origin['git_tree'], row['commit']))):
        raise ValueError('release provenance cannot replace an archive VCS binding')
    archive = next((p for p in sorted(cache.glob('*/' + archive_name))
                    if digest(p.read_bytes()) == package['checksum']), None)
    if archive is None:
        raise ValueError('reviewed release requires its checksum-verified archive')
    actual = {}
    names = set()
    prefix = Path(archive_name.removesuffix('.crate'))
    with tarfile.open(archive, 'r:gz') as bundle:
        for member in bundle:
            relative = Path(member.name).relative_to(prefix)
            if member.isdir():
                continue
            if (not member.isfile() or '..' in relative.parts or member.size > 16 * 1024 * 1024
                    or relative in names):
                raise ValueError('invalid release archive member')
            names.add(relative)
            if relative in (Path('Cargo.lock'), Path('Cargo.toml')):
                # Cargo generates these from the original tracked manifest.
                continue
            with bundle.extractfile(member) as source:
                actual[str(relative)] = git_blob(source.read())
    if ('Cargo.toml.orig' not in actual or actual != origin['member_git_blobs']):
        raise ValueError('reviewed release source does not match the verified archive')
    return row['commit'], package['archive_repository'], package['checksum']


def load_supplement(root, raw_inventory, inventory, path, cache=None):
    """Accept only notice bytes bound to the archive's exact upstream commit."""
    if path is None:
        return {}
    document = json.loads(path.read_bytes())
    if (document.get('schema_version') != 1
            or document.get('inventory_sha256') != digest(raw_inventory)):
        raise ValueError('supplement does not match the selected inventory')
    packages = {p['name'] + '@' + p['version']: p for p in inventory['packages']}
    result = {}
    for row in document['packages']:
        identity = row['package']
        package = packages.get(identity)
        if package is None or identity in result or not row['files']:
            raise ValueError('unknown, duplicate or empty supplementary package')
        if 'release_source' in row:
            commit, origin_repository, checksum = release_origin(package, row, cache)
        else:
            commit, origin_repository, checksum = source_origin(root, package)
        if (not commit or row['commit'] != commit or row['checksum'] != checksum):
            raise ValueError('supplement is not bound to the verified source archive')
        repository = github_repository(origin_repository or '')
        texts = {}
        for notice in row['files']:
            relative = notice['file']
            target = root / relative
            if (not relative.startswith('docs/licenses/upstream/')
                    or target.resolve() != target or not target.is_file()
                    or target.stat().st_size > 1024 * 1024
                    or relative in texts):
                raise ValueError('invalid supplementary notice file')
            url = notice['url']
            if not url.startswith(repository + '/blob/' + commit + '/'):
                raise ValueError('notice URL does not match pinned upstream repository')
            data = target.read_bytes()
            if (digest(data) != notice['sha256']
                    or git_blob(data) != notice['git_blob_sha1']):
                raise ValueError('supplementary notice bytes changed')
            texts[relative] = (data, notice)
        result[identity] = texts
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inventory', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--allow-incomplete', action='store_true')
    parser.add_argument('--supplement', type=Path,
                        help='reviewed exact-commit notice supplement for omitted archive files')
    parser.add_argument('--cache', type=Path, default=Path.home() / '.cargo/registry/cache')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    raw = args.inventory.read_bytes()
    inventory = json.loads(raw)
    supplement = load_supplement(root, raw, inventory, args.supplement, args.cache)
    missing = [p['name'] + '@' + p['version'] for p in inventory['packages']
               if not p['license_notice_files_sha256']
               and p['name'] + '@' + p['version'] not in supplement]
    if missing and not args.allow_incomplete:
        parser.error('notice files missing; use --allow-incomplete only for a development export')
    if args.output.exists():
        parser.error('output must not exist')
    for relative, expected in inventory['lock_sha256'].items():
        if digest((root / relative).read_bytes()) != expected:
            parser.error('inventory lockfile differs from current source')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.cashu-notices-', dir=args.output.parent) as temporary:
        stage = Path(temporary) / 'export'
        (stage / 'texts').mkdir(parents=True)
        rows = []
        for package in inventory['packages']:
            expected = package['license_notice_files_sha256']
            texts = {}
            if package['status'] == 'verified_archive':
                name = package['name'] + '-' + package['version']
                candidates = sorted(args.cache.glob('*/' + name + '.crate'))
                archive = next((p for p in candidates if digest(p.read_bytes()) == package['checksum']), None)
                if archive is None:
                    raise ValueError('verified source archive missing')
                with tarfile.open(archive, 'r:gz') as bundle:
                    for relative, checksum in expected.items():
                        member = bundle.getmember(name + '/' + relative)
                        if not member.isfile() or member.size > 1024 * 1024:
                            raise ValueError('invalid inventoried notice member')
                        texts[relative] = bundle.extractfile(member).read()
            elif package['status'] == 'local_manifest':
                for relative in expected:
                    path = root / relative
                    if path.resolve() != path or not path.is_file():
                        raise ValueError('invalid local notice path')
                    texts[relative] = path.read_bytes()
            else:
                raise ValueError('unresolved inventory source')
            references = {}
            for relative, data in texts.items():
                checksum = digest(data)
                if checksum != expected[relative]:
                    raise ValueError('notice checksum differs from inventory')
                target = 'texts/' + checksum + '.txt'
                (stage / target).write_bytes(data)
                references[relative] = target
            identity = package['name'] + '@' + package['version']
            upstream = []
            for relative, (data, provenance) in supplement.get(identity, {}).items():
                target = 'texts/' + digest(data) + '.txt'
                (stage / target).write_bytes(data)
                references[relative] = target
                upstream.append(provenance)
            rows.append({'name': package['name'], 'version': package['version'],
                         'source': package['source'], 'license': package['declared_license'],
                         'notice_files': references, 'upstream_notices': upstream})
        result = {'inventory_sha256': digest(raw), 'scope': inventory['scope'],
                  'supplement_sha256': digest(args.supplement.read_bytes()) if args.supplement else None,
                  'distribution_ready': False, 'notice_files_missing': missing,
                  'packages': rows}
        (stage / 'index.json').write_text(json.dumps(result, indent=2) + '\n')
        stage.rename(args.output)
    print(json.dumps({'exported_packages': len(rows), 'missing_notices': len(missing),
                      'distribution_ready': False}))


if __name__ == '__main__':
    main()
