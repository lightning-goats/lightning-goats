#!/usr/bin/env python3
"""Build and check the pinned household nak candidate with a scoped Cashu client."""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import runpy
import shutil
import subprocess
import tarfile
import tempfile


PACKAGE = Path(__file__).resolve().parent
CLIENT_BUILD = PACKAGE.with_name('cashu-nak-stdin')
MUSL_CC = '/usr/bin/musl-gcc'


def diagnostics(result):
    findings = Counter()
    for line in (result.stdout + result.stderr).splitlines():
        if not line or line.startswith(('# ', 'go: downloading ')):
            continue
        match = re.fullmatch(r'(?:\./)?([^:]+):\d+:\d+: (non-constant format string in call to .+)', line)
        if match is None:
            raise ValueError('unexpected Go vet result; see the build log')
        findings[match.groups()] += 1
    if result.returncode not in (0, 1) or bool(result.returncode) != bool(findings):
        raise ValueError('unexpected Go vet exit; see the build log')
    return findings


def require_static_elf(program):
    result = subprocess.check_output(['/usr/bin/readelf', '-W', '-l', '-d', str(program)], text=True)
    if re.search(r'^\s*(?:INTERP\b|0x[0-9a-f]+\s+\(NEEDED\))', result, re.MULTILINE):
        raise ValueError('the portable candidate still requires a dynamic loader or library')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-directory', required=True, type=Path)
    parser.add_argument('--static', action='store_true', help='link the Linux x86-64 candidate with musl')
    args = parser.parse_args()
    if args.static and not Path(MUSL_CC).is_file():
        parser.error('--static requires /usr/bin/musl-gcc')
    root = args.build_directory.resolve()
    root.mkdir(parents=True, exist_ok=True)
    output = root / 'nak-cashu-bunker'
    if output.exists() or output.is_symlink():
        raise ValueError('build destination exists; choose a new directory')

    shared = runpy.run_path(str(CLIENT_BUILD / 'build.py'))
    archive, digest = shared['archive'], shared['digest']
    manifest = json.loads((PACKAGE / 'manifest.json').read_bytes())
    sdk = json.loads((CLIENT_BUILD / 'manifest.json').read_bytes())
    patch = PACKAGE / 'bunker.patch'
    if digest(patch) != manifest['patch_sha256']:
        raise ValueError('reviewed patch digest differs')
    upstream = manifest['upstream_commit']
    source_archive = archive(root, 'nak-household.tar.gz',
                             'https://codeload.github.com/fiatjaf/nak/tar.gz/' + upstream,
                             manifest['source_sha256'])
    sdk_archive = archive(root, 'go.tar.gz', 'https://go.dev/dl/go' + sdk['go_version'] + '.linux-amd64.tar.gz',
                          sdk['go_archive_sha256'])

    with tempfile.TemporaryDirectory(prefix='compile-', dir=root) as temporary:
        work = Path(temporary)
        for path in (source_archive, sdk_archive):
            with tarfile.open(path) as bundle:
                bundle.extractall(work, filter='data')
        source = work / ('nak-' + upstream)
        if digest(source / 'bunker.go') != manifest['upstream_bunker_sha256']:
            raise ValueError('upstream bunker source differs')
        go = str(work / 'go/bin/go')
        environment = {'PATH': '/usr/bin:/bin', 'GOENV': 'off', 'GOTOOLCHAIN': 'local',
                       'CGO_ENABLED': '1', 'GOOS': 'linux', 'GOARCH': 'amd64',
                       'GOMAXPROCS': '2', 'GOMEMLIMIT': '512MiB',
                       'GOCACHE': str(root / 'cache'), 'GOMODCACHE': str(root / 'modules'),
                       'GOPATH': str(root / 'gopath'), 'GOTMPDIR': str(work),
                       'GOPROXY': 'https://proxy.golang.org', 'GOSUMDB': 'sum.golang.org'}
        target_flags = []
        static_flags = ''
        if args.static:
            environment['CC'] = MUSL_CC
            target_flags = ['-tags=netgo,osusergo']
            static_flags = ' -linkmode=external -extldflags=-static'

        def run(*arguments):
            subprocess.run([go, *arguments], cwd=source, env=environment, check=True)

        def vet(label):
            result = subprocess.run([go, 'vet', '-mod=readonly', '-p=2', *target_flags, '.'],
                                    cwd=source, env=environment, capture_output=True, text=True)
            (root / (label + '-go-vet.log')).write_text(result.stdout + result.stderr)
            return diagnostics(result)

        baseline = vet('upstream')
        patch_environment = {'PATH': '/usr/bin:/bin', 'GIT_CEILING_DIRECTORIES': str(work),
                             'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_SYSTEM': '/dev/null'}
        subprocess.run(['git', 'apply', str(patch)], cwd=source, env=patch_environment, check=True)
        files = ('cashu_scope.go', 'cashu_scope_test.go')
        for name in files:
            shutil.copyfile(PACKAGE / name, source / name)
        formatted = subprocess.check_output([work / 'go/bin/gofmt', '-l', *files], cwd=source)
        if formatted:
            raise ValueError('gofmt is required for scoped Go sources')
        if vet('scoped') != baseline:
            raise ValueError('Go vet findings changed from the pinned upstream source')
        test_link = ['-ldflags=' + static_flags.strip()] if static_flags else []
        run('test', '-vet=off', '-mod=readonly', '-p=2', *target_flags, *test_link,
            '-run', '^TestCashuSigningScope$', '.')
        run('build', '-mod=readonly', '-p=2', '-trimpath', '-buildvcs=false', *target_flags,
            '-ldflags=-s -w -X main.version=' + manifest['version'] + static_flags, '-o', str(output), '.')
        if args.static:
            require_static_elf(output)
        result = {**manifest, 'go_version': sdk['go_version'], 'go_archive_sha256': sdk['go_archive_sha256'],
                  'program_sha256': digest(output), 'scope_inputs': {name: digest(PACKAGE / name) for name in files},
                  'target': 'linux-amd64', 'linkage': 'static-musl' if args.static else 'dynamic',
                  'build_metadata': subprocess.check_output([go, 'version', '-m', str(output)],
                                                            env=environment, text=True),
                  'unchanged_upstream_vet_findings': sum(baseline.values()), 'installed': False}
        (root / 'build-result.json').write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
        print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
