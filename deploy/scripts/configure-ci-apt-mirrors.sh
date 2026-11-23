#!/usr/bin/env bash
# Hosted disposable runners only; keep Ubuntu signing policy and suites intact.
set -euo pipefail
test "${GITHUB_ACTIONS:-}" = true
test "${RUNNER_OS:-}" = Linux
test "$(id -u)" = 0
test -f /etc/apt/apt-mirrors.txt
test ! -L /etc/apt/apt-mirrors.txt
. /etc/os-release
case "${VERSION_CODENAME:-}" in
  jammy|noble) ;;
  *) echo 'Unsupported hosted Ubuntu release' >&2; exit 1 ;;
esac
# Check reachability before selecting a mirror; APT still verifies Ubuntu's
# signed Release metadata and package hashes using the runner's existing keys.
# No IP pinning, DNS override, unsigned repository or TLS exception is used.
for mirror in https://mirrors.edge.kernel.org/ubuntu https://archive.ubuntu.com/ubuntu https://security.ubuntu.com/ubuntu; do
  if curl --fail --silent --show-error --head --connect-timeout 3 --max-time 8 \
      "${mirror}/dists/${VERSION_CODENAME}/InRelease" > /dev/null; then
    printf '%s\n' "$mirror" > /etc/apt/apt-mirrors.txt
    echo "Selected reachable Ubuntu mirror: $mirror"
    exit 0
  fi
done
echo 'No configured Ubuntu mirror is reachable; dependency installation refused' >&2
exit 1
