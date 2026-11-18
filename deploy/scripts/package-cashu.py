#!/usr/bin/env python3
"""Build and verify a source-bound, inactive Linux Cashu review archive."""
import argparse
from collections import defaultdict
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tempfile
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[2]
TARGET = 'x86_64-unknown-linux-gnu'
CORE = ('lightning-goatsd', 'lightning-goatsctl', 'lightning-goats-gateway')
WALLET = ('lightning-goats-cashud', 'lightning-goats-cashu-signerd', 'lightning-goats-cashu-statectl')
BINS = CORE + WALLET
INVENTORY = 'docs/testing/cashu-license-inventory-20261117.json'
SUPPLEMENT = 'docs/licenses/cashu-upstream-supplement.json'
LOCKS = ('Cargo.lock', 'tools/cashu-compat/Cargo.lock')
CAPSULE = 'cashu-backup-capsule'
CAPSULE_MANIFEST = f'{CAPSULE}/TOOL.json'
CAPSULE_TOOLS = (
    'launch-cashu-backup.py', 'collect-cashu-backup.py', 'stage-cashu-restore.py',
    'review-cashu-restore.py', 'reconcile-cashu-restore.py', 'verify-cashu-backup.py',
    'verify-cashu-combined-backup.py', 'apply-cashu-mint-policy.py', 'inactive_file_transaction.py',
    'install-cashu-restore.py',
)
CAPSULE_FILES = tuple(f'{CAPSULE}/{name}' for name in CAPSULE_TOOLS)
PAYLOAD = (
    'LICENSE', 'Cargo.toml', 'tools/cashu-compat/Cargo.toml', *LOCKS, INVENTORY,
    'deploy/config.receive.toml.example', 'deploy/systemd/lightning-goats-cashu.service.example',
    'deploy/systemd/lightning-goats-cashu-signer.service.example',
    'deploy/systemd/lightning-goats-receive.service.d/80-cashu-receipts.conf.example',
    'deploy/tmpfiles.d/lightning-goats-cashu-receipts.conf.example',
    'deploy/nginx/lightning-goats-cashu.conf.example', 'deploy/cashu/backup-collection.json.example',
    'docs/deployment/cashu-wallet-development.md', 'docs/deployment/cashu-backup-collection.md',
    'docs/deployment/cashu-operator-controls.md', 'docs/deployment/cashu-seed-provisioning.md',
    'docs/deployment/cashu-release-candidate.md', 'docs/deployment/audit-remediation.md',
    'docs/deployment/cashu-recovery-inventory.md',
    'web/overlay.html', 'web/overlay.css', 'web/overlay.js',
    'web/vendor/qrcode.js', 'web/vendor/qrcode-LICENSE.txt', 'web/vendor/SOURCES.txt',
    *(f'web/images/{name}.png' for name in (
        'cosmo', 'dexter', 'lightning', 'lightninggoatslogo1', 'newton', 'nova', 'rowan',
    )),
    'deploy/scripts/add-cashu-seed.py', 'deploy/scripts/prepare-cashu-backup-age.py',
    'deploy/scripts/render-cashu-egress.py',
    'deploy/scripts/render-cashu-pilot.py',
)
SHA = re.compile(r'[0-9a-f]{64}')
RELEASE_ID = re.compile(r'[0-9a-f]{40}')
RETAINED_ARCHIVE = re.compile(r'^(/[^\r\n]+?\.(?:rlib|a))\(([^()\r\n]+)\)(?:\s|$)')


def run(*args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, **kwargs)


def file_sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def store_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def unique_fields(fields):
    result = dict(fields)
    if len(result) != len(fields):
        raise ValueError('duplicate backup capsule manifest field')
    return result


def package_identity(package_id, by_manifest, known):
    source, separator, identity = package_id.partition('#')
    if source.startswith('path+file://'):
        manifest = Path(unquote(source.removeprefix('path+file://'))) / 'Cargo.toml'
        return by_manifest.get(manifest.resolve())
    if separator and source.startswith('registry+') and identity in known:
        return identity
    return None


def selected_archives(raw_map):
    header, separator, _ = raw_map.partition('\nDiscarded input sections\n')
    if not separator or not header.startswith('Archive member included to satisfy reference by file (symbol)\n'):
        raise ValueError('missing GNU linker archive selection table')
    result = defaultdict(set)
    for line in header.splitlines():
        if match := RETAINED_ARCHIVE.match(line):
            result[Path(match[1]).resolve()].add(match[2])
    if not result:
        raise ValueError('linker selection table has no archive members')
    return result


def identify_links(raw_map, messages, packages, sysroot, binary):
    by_manifest = {(ROOT / p['manifest']).resolve(): p['name'] + '@' + p['version']
                   for p in packages.values() if p['status'] == 'local_manifest'}
    by_file, by_output = {}, {}
    for message in messages:
        reason = message['reason']
        if reason not in ('compiler-artifact', 'build-script-executed'):
            continue
        identity = package_identity(message['package_id'], by_manifest, packages)
        if reason == 'compiler-artifact':
            for file in message['filenames']:
                if file.endswith('.rlib'):
                    by_file[Path(file).resolve()] = identity
        else:
            by_output[Path(message['out_dir']).resolve()] = identity
    cargo_members = defaultdict(set)
    rust_members, system_members = {}, {}
    rust_lib = sysroot / 'lib/rustlib' / TARGET / 'lib'
    system_roots = [Path('/usr/lib').resolve(), Path('/usr/lib64').resolve()]
    for archive, members in selected_archives(raw_map).items():
        if archive in by_file:
            owner = by_file[archive]
        else:
            owner = next((by_output[p] for p in archive.parents if p in by_output), None)
        if owner:
            cargo_members[owner].update(f'{archive.name}({item})' for item in members)
        elif archive.is_relative_to(rust_lib):
            rust_members[archive.name] = sorted(members)
        elif archive.suffix == '.a' and any(archive.is_relative_to(path) for path in system_roots):
            system_members[archive.name] = sorted(members)
        else:
            raise ValueError(f'archive has no inventoried owner: {archive.name}')
    if not cargo_members or not rust_members:
        raise ValueError('Rust and Cargo archive selections are both required')
    elf = run('readelf', '-dW', str(binary), text=True)
    shared = sorted(set(re.findall(r'\(NEEDED\)\s+Shared library: \[([^\]]+)\]', elf)))
    version_info = run('readelf', '--version-info', str(binary), text=True)
    glibc = {tuple(map(int, v.split('.'))) for v in re.findall(r'Name: GLIBC_([0-9]+(?:\.[0-9]+)+)\b', version_info)}
    if not shared or not glibc:
        raise ValueError('expected dynamically linked GNU/Linux executable')
    return dict(cargo_archive_members={key: sorted(value) for key, value in cargo_members.items()},
                rust_toolchain_archive_members=rust_members, system_archive_members=system_members,
                elf_needed=shared, glibc_required='.'.join(map(str, max(glibc))))


def stage_binary(name, manifest, profile, temporary, stage, packages, sysroot):
    map_path = temporary / f'{name}.map'
    arguments = ['cargo', 'rustc', '--locked', '--offline', '--manifest-path', manifest,
                 '--profile', profile, '--all-features', '--bin', name, '--message-format=json',
                 '--', '-C', 'linker=cc', '-C', 'link-arg=-fuse-ld=bfd',
                 '-C', f'link-arg=-Wl,-Map,{map_path}']
    result = run(*arguments)
    messages = [json.loads(line) for line in result.splitlines()]
    targets = [m for m in messages if m['reason'] == 'compiler-artifact'
               and m['target']['kind'] == ['bin'] and m['target']['name'] == name and m['executable']]
    if len(targets) != 1:
        raise ValueError('expected exactly one linked executable from Cargo')
    source = Path(targets[0]['executable'])
    binary = stage / name
    shutil.copyfile(source, binary)
    binary.chmod(0o755)
    links = identify_links(map_path.read_text(), messages, packages, sysroot, binary)
    map_path.unlink()
    subprocess.run([binary, '--help'], env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, check=True, timeout=15)
    return dict(links, sha256=file_sha256(binary), features=targets[0]['features'])


def file_manifest(stage):
    return {p.relative_to(stage).as_posix(): file_sha256(p)
            for p in sorted(stage.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS'}


def pack(stage, archive, epoch):
    manifest = file_manifest(stage)
    (stage / 'SHA256SUMS').write_text(''.join(f'{digest}  {name}\n' for name, digest in manifest.items()))
    with archive.open('xb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w|', format=tarfile.USTAR_FORMAT) as tar:
            for name in sorted([*manifest, 'SHA256SUMS']):
                path = stage / name
                info = tarfile.TarInfo(name)
                info.size = path.stat().st_size
                info.mode = 0o755 if name in BINS else 0o644
                info.mtime = epoch
                with path.open('rb') as stream:
                    tar.addfile(info, stream)


def build(output, profile):
    if output.exists():
        raise ValueError('choose a new output directory')
    rust = run('rustc', '--version', '--verbose', text=True).strip()
    if f'host: {TARGET}' not in rust.splitlines():
        raise ValueError('this package targets the native x86-64 GNU/Linux toolchain')
    sysroot = Path(run('rustc', '--print', 'sysroot', text=True).strip()).resolve()
    source = run('git', 'rev-parse', 'HEAD', text=True).strip()
    source_tree = run('git', 'rev-parse', 'HEAD^{tree}', text=True).strip()
    epoch = int(run('git', 'show', '-s', '--format=%ct', source))
    if run('git', 'status', '--porcelain', '--untracked-files=normal'):
        raise ValueError('build from a clean exact source checkout')
    packages = {p['name'] + '@' + p['version']: p
                for p in json.loads((ROOT / INVENTORY).read_bytes())['packages']}
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.lg-cashu-package-', dir=output.parent) as directory:
        temporary = Path(directory)
        stage = temporary / 'payload'
        stage.mkdir()
        build_id = temporary / 'result'
        build_id.mkdir()
        subprocess.run(['python3', str(ROOT / 'deploy/scripts/export-cashu-notices.py'),
                        str(ROOT / INVENTORY), str(stage / 'notices'), '--supplement', str(ROOT / SUPPLEMENT)],
                       check=True)
        for name in PAYLOAD:
            path = stage / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(run('git', 'show', f'{source}:{name}'))
        (stage / CAPSULE).mkdir()
        for name in CAPSULE_TOOLS:
            (stage / CAPSULE / name).write_bytes(run('git', 'show', f'{source}:deploy/scripts/{name}'))
        store_json(stage / CAPSULE_MANIFEST, dict(version=1, source_commit=source,
                   files={name: file_sha256(stage / CAPSULE / name) for name in CAPSULE_TOOLS}))
        shutil.copytree(sysroot / 'share/doc/rust/licenses', stage / 'rust-toolchain/licenses')
        shutil.copyfile(sysroot / 'share/doc/rust/COPYRIGHT-library.html',
                        stage / 'rust-toolchain/COPYRIGHT-library.html')
        records = {}
        for name in BINS:
            manifest = 'Cargo.toml' if name in CORE else 'tools/cashu-compat/Cargo.toml'
            records[name] = stage_binary(name, manifest, profile, temporary, stage, packages, sysroot)
        if (run('git', 'rev-parse', 'HEAD', text=True).strip() != source
                or run('git', 'status', '--porcelain', '--untracked-files=normal')):
            raise ValueError('source checkout changed during package build')
        info = dict(version=1, source_commit=source, source_tree=source_tree, target=TARGET,
                    activation='none', profile=profile, rustc=rust, binaries=records,
                    cargo=run('cargo', '--version', text=True).strip(),
                    linker=run('ld.bfd', '--version', text=True).splitlines()[0],
                    cc=run('cc', '--version', text=True).splitlines()[0])
        store_json(stage / 'BUILD-INFO.json', info)
        archive = build_id / f'lightning-goats-cashu-{source[:12]}-{profile}-{TARGET}.tar.gz'
        pack(stage, archive, epoch)
        result = verify(archive, source)
        (build_id / 'SHA256SUMS').write_text(f'{file_sha256(archive)}  {archive.name}\n')
        build_id.rename(output)
        return dict(result, archive=str(output / archive.name))


def verify(archive, source):
    if not RELEASE_ID.fullmatch(source):
        raise ValueError('expected exact source commit')
    manifest_files = {'SHA256SUMS', 'BUILD-INFO.json', 'notices/index.json', INVENTORY, CAPSULE_MANIFEST}
    files, manifests, total = {}, {}, 0
    with tarfile.open(archive, 'r:gz') as tar:
        for member in tar:
            name = PurePosixPath(member.name)
            total += member.size
            if (name.is_absolute() or '..' in name.parts or name.as_posix() != member.name
                    or not member.isfile() or member.name in files or len(files) >= 4096
                    or member.size > 128 * 1024**2 or total > 512 * 1024**2):
                raise ValueError('archive has invalid, duplicate or oversized member')
            expected_mode = 0o755 if member.name in BINS else 0o644
            if member.mode != expected_mode or member.uid or member.gid:
                raise ValueError('archive ownership or mode differs from candidate layout')
            if member.name.startswith(CAPSULE + '/') and member.size > 1024**2:
                raise ValueError('backup capsule member exceeds launcher limit')
            with tar.extractfile(member) as stream:
                if member.name in manifest_files:
                    if member.size > 8 * 1024**2:
                        raise ValueError('oversized archive manifest')
                    manifests[member.name] = stream.read()
                    digest = hashlib.sha256(manifests[member.name]).hexdigest()
                elif member.name in BINS:
                    header = stream.read(20)
                    if (len(header) != 20 or header[:7] != b'\x7fELF\x02\x01\x01'
                            or header[16:18] not in (b'\x02\x00', b'\x03\x00')
                            or header[18:20] != b'\x3e\x00'):
                        raise ValueError('candidate binary must be x86-64 ELF executable')
                    hashed = hashlib.sha256(header)
                    while chunk := stream.read(1024**2):
                        hashed.update(chunk)
                    digest = hashed.hexdigest()
                else:
                    digest = hashlib.file_digest(stream, 'sha256').hexdigest()
                files[member.name] = digest
    expected = {}
    for line in manifests['SHA256SUMS'].decode().splitlines():
        digest, name = line.split('  ', 1)
        if name in expected or not SHA.fullmatch(digest):
            raise ValueError('invalid archive checksum record')
        expected[name] = digest
    if expected != {k: v for k, v in files.items() if k != 'SHA256SUMS'}:
        raise ValueError('archive checksum manifest differs from payload')
    if not set((*PAYLOAD, *BINS, *CAPSULE_FILES, *manifest_files,
                'rust-toolchain/COPYRIGHT-library.html')).issubset(files):
        raise ValueError('review archive is missing required files')
    info = json.loads(manifests['BUILD-INFO.json'])
    if (info['source_commit'] != source or info['target'] != TARGET or info['activation'] != 'none'
            or info['profile'] not in ('dev', 'release') or set(info['binaries']) != set(BINS)
            or not RELEASE_ID.fullmatch(info['source_tree'])):
        raise ValueError('candidate build identity differs')
    capsule = json.loads(manifests[CAPSULE_MANIFEST], object_pairs_hook=unique_fields)
    expected_tools = {name: files[f'{CAPSULE}/{name}'] for name in CAPSULE_TOOLS}
    if (not isinstance(capsule, dict) or type(capsule.get('version')) is not int
            or capsule != {'version': 1, 'source_commit': source, 'files': expected_tools}):
        raise ValueError('backup capsule does not match candidate source or files')
    notices = json.loads(manifests['notices/index.json'])
    inventory = json.loads(manifests[INVENTORY])
    identities = {p['name'] + '@' + p['version'] for p in inventory['packages']}
    if (notices['inventory_sha256'] != files[INVENTORY] or notices['notice_files_missing']
            or identities != {p['name'] + '@' + p['version'] for p in notices['packages']}
            or inventory['lock_sha256'] != {lock: files[lock] for lock in LOCKS}
            or len(notices['packages']) != len(identities)):
        raise ValueError('notice inventory differs from candidate locks')
    notice_files = set()
    for row in notices['packages']:
        if not row['notice_files']:
            raise ValueError('notice inventory contains a package without notice text')
        for path in row['notice_files'].values():
            match = re.fullmatch(r'texts/([a-f0-9]{64})\.txt', path)
            if not match or files.get('notices/' + path) != match[1]:
                raise ValueError('notice text differs from index')
            notice_files.add('notices/' + path)
    rust_files = {file for file in files if file.startswith('rust-toolchain/licenses/')}
    allowed = {*PAYLOAD, *BINS, *manifest_files, *notice_files, *CAPSULE_FILES,
               'rust-toolchain/COPYRIGHT-library.html', *rust_files}
    if not rust_files or set(files) != allowed:
        raise ValueError('candidate contains unexpected files or incomplete toolchain notices')
    for name, record in info['binaries'].items():
        if (record['sha256'] != files[name] or not record['cargo_archive_members']
                or not set(record['cargo_archive_members']).issubset(identities)
                or not record['rust_toolchain_archive_members'] or not record['elf_needed']):
            raise ValueError('binary or link inventory differs from candidate')
    return dict(source_commit=source, target=TARGET, profile=info['profile'], activation='none',
                binaries=len(BINS), notices=len(identities), files=len(files))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    assemble = commands.add_parser('build')
    assemble.add_argument('output', type=Path)
    assemble.add_argument('--profile', choices=('dev', 'release'), default='release')
    verify_command = commands.add_parser('verify')
    verify_command.add_argument('archive', type=Path)
    verify_command.add_argument('source')
    args = parser.parse_args()
    result = build(args.output.resolve(), args.profile) if args.command == 'build' else verify(args.archive, args.source)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
