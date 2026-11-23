#!/usr/bin/env python3
"""Build a separate pinned nak with bounded --stdin crypto input; never install it."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.request


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def archive(root, name, url, expected):
    path = root / name
    if not path.exists():
        with urllib.request.urlopen(url, timeout=60) as response, path.open('xb') as output:
            total = 0
            while block := response.read(1024 * 1024):
                total += len(block)
                if total > 100_000_000:
                    raise ValueError('archive exceeds bound')
                output.write(block)
    if digest(path) != expected:
        raise ValueError('archive digest mismatch')
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-directory', required=True, type=Path)
    args = parser.parse_args()
    root = args.build_directory.resolve()
    root.mkdir(parents=True, exist_ok=True)
    package = Path(__file__).resolve().parent
    manifest = json.loads((package / 'manifest.json').read_text())
    patch = package / 'bounded-stdin.patch'
    if digest(patch) != manifest['patch_sha256']:
        raise ValueError('reviewed patch digest mismatch')
    source_archive = archive(root, 'nak.tar.gz', 'https://codeload.github.com/fiatjaf/nak/tar.gz/' + manifest['upstream_commit'], manifest['source_sha256'])
    sdk_archive = archive(root, 'go.tar.gz', 'https://go.dev/dl/go' + manifest['go_version'] + '.linux-amd64.tar.gz', manifest['go_archive_sha256'])
    output = root / 'nak-cashu-stdin'
    if output.exists():
        raise ValueError('build output already exists; choose a fresh build directory')
    with tempfile.TemporaryDirectory(prefix='compile-', dir=root) as temporary:
        work = Path(temporary)
        for path in (source_archive, sdk_archive):
            with tarfile.open(path) as stream:
                stream.extractall(work, filter='data')
        source = work / ('nak-' + manifest['upstream_commit'])
        if digest(source / 'encrypt_decrypt.go') != manifest['upstream_encrypt_decrypt_sha256']:
            raise ValueError('upstream crypto source differs')
        patch_env={'PATH':'/usr/bin:/bin','GIT_CEILING_DIRECTORIES':str(work),'GIT_CONFIG_GLOBAL':'/dev/null','GIT_CONFIG_SYSTEM':'/dev/null'}
        subprocess.run(['git','apply','--check',str(patch)],cwd=source,env=patch_env,check=True)
        subprocess.run(['git','apply',str(patch)],cwd=source,env=patch_env,check=True)
        go = work / 'go' / 'bin' / 'go'
        environment = {'PATH': '/usr/bin:/bin', 'GOENV': 'off', 'GOTOOLCHAIN': 'local',
                       'CGO_ENABLED': '1', 'GOOS': 'linux', 'GOARCH': 'amd64',
                       'GOMAXPROCS': '1', 'GOMEMLIMIT': '512MiB',
                       'GOCACHE': str(root / 'cache'), 'GOMODCACHE': str(root / 'modules'),
                       'GOPATH': str(root / 'gopath'), 'GOPROXY': 'https://proxy.golang.org',
                       'GOSUMDB': 'sum.golang.org', 'GOTMPDIR': str(work)}
        vet=subprocess.run([str(go),'vet','-mod=readonly','-p=1','.'],cwd=source,env=environment,capture_output=True,text=True)
        (root / 'go-vet.log').write_text(vet.stdout+vet.stderr)
        diagnostics=[line.removeprefix('./') for line in (vet.stdout+vet.stderr).splitlines() if line and not line.startswith(('# ','go: downloading '))]
        expected=(package / 'upstream-vet-baseline.txt').read_text().splitlines()
        if vet.returncode!=1 or sorted(diagnostics)!=sorted(expected):
            raise ValueError('upstream vet diagnostics changed; review before building')
        # Full vet output is retained and checked against the exact pinned upstream
        # baseline. Disable duplicate test-time vet only after that comparison.
        subprocess.run([str(go), 'test', '-vet=off', '-mod=readonly', '-p=1', '-run', '^TestCashuBoundedStdin$', '.'], cwd=source, env=environment, check=True)
        subprocess.run([str(go), 'build', '-mod=readonly', '-p=1', '-trimpath', '-buildvcs=false',
                        '-ldflags=-s -w -X main.version=' + manifest['version'], '-o', str(output), '.'], cwd=source, env=environment, check=True)
        metadata = subprocess.check_output([str(go), 'version', '-m', str(output)], env=environment, text=True)
        result = {**manifest, 'binary_sha256': digest(output), 'build_metadata': metadata,
                  'upstream_vet_baseline': diagnostics, 'vet_exit': vet.returncode, 'cgo_enabled': True, 'c_compiler': subprocess.check_output(['/usr/bin/gcc','--version'],text=True).splitlines()[0], 'installed': False}
        (root / 'build-result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps({'binary_sha256': result['binary_sha256'], 'installed': False}))


if __name__ == '__main__':
    main()
