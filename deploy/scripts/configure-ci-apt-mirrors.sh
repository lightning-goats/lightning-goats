#!/usr/bin/env bash
# Hosted disposable runners only; keep Ubuntu signing policy and suites intact.
set -euo pipefail
test "${GITHUB_ACTIONS:-}" = true
test "${RUNNER_OS:-}" = Linux
test "$(id -u)" = 0
test -f /etc/apt/apt-mirrors.txt
test ! -L /etc/apt/apt-mirrors.txt
printf '%s\n' 'https://archive.ubuntu.com/ubuntu' 'https://security.ubuntu.com/ubuntu' > /etc/apt/apt-mirrors.txt
