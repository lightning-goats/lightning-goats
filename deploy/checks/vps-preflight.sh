#!/usr/bin/env bash
set -euo pipefail

APP_CONFIG="${APP_CONFIG:-/etc/lightning-goats/config.toml}"
APP_UNIT="${APP_UNIT:-/etc/systemd/system/lightning-goats.service}"
APP_BIN="${APP_BIN:-/usr/local/bin/lightning-goatsd}"
GATEWAY_IP="${GATEWAY_IP:-10.8.0.6}"
GATEWAY_PORT="${GATEWAY_PORT:-8789}"
WEB_ROOT="${WEB_ROOT:-/var/www/lightning-goats}"
FAILURES=0

pass() { printf 'PASS: %s\n' "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; FAILURES=$((FAILURES + 1)); }
require_file() { [[ -f "$1" ]] && pass "file exists: $1" || fail "missing file: $1"; }

check_root_owned_nonwritable() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    fail "missing path for ownership check: $path"
    return
  fi
  local uid gid mode
  uid=$(stat -c '%u' "$path")
  gid=$(stat -c '%g' "$path")
  mode=$(stat -c '%A' "$path")
  [[ "$uid" == 0 && "$gid" == 0 ]] || fail "$path must be root:root (is $uid:$gid)"
  [[ ! -w "$path" || "$uid" == 0 ]] || true
  if find "$path" -maxdepth 0 -perm /022 -print -quit | grep -q .; then
    fail "$path is group/world writable ($mode)"
  else
    pass "$path is root-owned and not group/world writable"
  fi
}

require_file "$APP_CONFIG"
require_file "$APP_UNIT"
require_file "$APP_BIN"

if [[ -f "$APP_CONFIG" ]]; then
  if grep -Eiq '(^|[^a-z])(clnrest|cln-rune|pay_index|clnaddress|lnbits|\[lightning\])' "$APP_CONFIG"; then
    fail "production config contains retired CLN/LNbits terms"
  else
    pass "production config contains no retired CLN/LNbits settings"
  fi
  if grep -Eiq 'openhab[-_ ]?token|rest/rules|:8080' "$APP_CONFIG"; then
    fail "VPS production config contains direct OpenHAB authority"
  else
    pass "VPS production config contains no direct OpenHAB authority"
  fi
  if grep -q 'REPLACE_WITH_' "$APP_CONFIG"; then
    fail "production config still contains REPLACE_WITH_ placeholders"
  else
    pass "production config has no unresolved REPLACE_WITH_ placeholders"
  fi
fi

if [[ -f "$APP_UNIT" ]]; then
  if grep -Eiq 'cln-rune|openhab-token|lnbits' "$APP_UNIT"; then
    fail "VPS systemd unit loads a retired or trusted-side credential"
  else
    pass "VPS systemd unit does not load CLN/LNbits/OpenHAB credentials"
  fi
  grep -q 'strike-api-key' "$APP_UNIT" && pass "Strike credential declared" || fail "Strike credential missing from unit"
fi

check_root_owned_nonwritable "$APP_BIN"
[[ -f "$APP_CONFIG" ]] && check_root_owned_nonwritable "$APP_CONFIG"

if [[ -d "$WEB_ROOT" ]]; then
  [[ -f "$WEB_ROOT/index.html" ]] && pass "public site index present" || fail "public site index missing"
  if find "$WEB_ROOT" -perm /022 -print -quit | grep -q .; then
    fail "public site contains group/world-writable files"
  else
    pass "public site is not group/world writable"
  fi
else
  fail "public site root missing: $WEB_ROOT"
fi

command -v systemd-analyze >/dev/null && systemd-analyze verify "$APP_UNIT" >/dev/null \
  && pass "systemd unit verifies" || fail "systemd unit verification failed"
command -v nginx >/dev/null && nginx -t >/dev/null 2>&1 \
  && pass "nginx configuration validates" || fail "nginx configuration validation failed"

if ss -lntH | awk '{print $4}' | grep -Eq '(^|\])0\.0\.0\.0:8787$|:::8787$'; then
  fail "lightning-goatsd port 8787 is listening on a wildcard address"
elif ss -lntH | awk '{print $4}' | grep -Eq '127\.0\.0\.1:8787$'; then
  pass "lightning-goatsd listener is loopback-only"
else
  fail "expected lightning-goatsd loopback listener 127.0.0.1:8787 not found"
fi

curl --fail --silent --show-error --max-time 3 http://127.0.0.1:8787/healthz >/dev/null \
  && pass "local lightning-goatsd health endpoint works" || fail "local lightning-goatsd health endpoint failed"
curl --fail --silent --show-error --max-time 3 "http://${GATEWAY_IP}:${GATEWAY_PORT}/healthz" >/dev/null \
  && pass "trusted gateway health is reachable over WireGuard" || fail "trusted gateway health is not reachable"

# The new VPS must have only the dedicated trusted-side capability. These tests
# intentionally expect connection failure. Use TCP connect tests rather than
# application credentials so they validate the network boundary itself.
for port in 5000 8080 22 5432; do
  if timeout 2 bash -c "</dev/tcp/${GATEWAY_IP}/${port}" 2>/dev/null; then
    fail "unexpected direct TCP reachability to ${GATEWAY_IP}:${port}"
  else
    pass "direct TCP ${GATEWAY_IP}:${port} is blocked"
  fi
done

if command -v wg >/dev/null; then
  wg show >/dev/null && pass "WireGuard is readable" || fail "WireGuard status failed"
else
  fail "wg command missing"
fi

printf '\nBinary hashes (record these in the deployment log):\n'
sha256sum "$APP_BIN"
[[ -x /usr/local/bin/lightning-goatsctl ]] && sha256sum /usr/local/bin/lightning-goatsctl || true

if (( FAILURES > 0 )); then
  printf '\nPreflight FAILED with %d issue(s).\n' "$FAILURES" >&2
  exit 1
fi
printf '\nPreflight PASSED. No physical feeder action was attempted.\n'
