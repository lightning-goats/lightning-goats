#!/usr/bin/env bash
set -euo pipefail

GATEWAY_CONFIG="${GATEWAY_CONFIG:-/etc/lightning-goats-gateway/config.toml}"
GATEWAY_UNIT="${GATEWAY_UNIT:-/etc/systemd/system/lightning-goats-gateway.service}"
GATEWAY_BIN="${GATEWAY_BIN:-/usr/local/bin/lightning-goats-gateway}"
GATEWAY_IP="${GATEWAY_IP:-10.8.0.6}"
GATEWAY_PORT="${GATEWAY_PORT:-8789}"
FAILURES=0

pass() { printf 'PASS: %s\n' "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; FAILURES=$((FAILURES + 1)); }

for path in "$GATEWAY_CONFIG" "$GATEWAY_UNIT" "$GATEWAY_BIN"; do
  [[ -f "$path" ]] && pass "file exists: $path" || fail "missing file: $path"
done

if [[ -f "$GATEWAY_CONFIG" ]]; then
  grep -q 'url = "http://127.0.0.1:8080/' "$GATEWAY_CONFIG" \
    && pass "OpenHAB access is loopback-only" || fail "OpenHAB gateway URL is not expected loopback origin"
  grep -q 'url = "http://127.0.0.1:5000/get_received_data"' "$GATEWAY_CONFIG" \
    && pass "weather source is exact local read endpoint" || fail "weather source is not exact local read endpoint"
  if grep -Eq '(^|/)weather([?"/]|$)' "$GATEWAY_CONFIG" && ! grep -q 'get_received_data' "$GATEWAY_CONFIG"; then
    fail "gateway config appears to reference legacy mutating weather endpoint"
  fi
  if grep -q 'REPLACE_WITH_' "$GATEWAY_CONFIG"; then
    fail "gateway config still contains unresolved REPLACE_WITH_ placeholders"
  else
    pass "gateway config has no unresolved REPLACE_WITH_ placeholders"
  fi
  grep -q 'request_item = "GoatFeeder_ManualRequest"' "$GATEWAY_CONFIG" \
    && pass "production gateway reuses correlated feeder owner request Item" \
    || fail "production gateway is not bound to GoatFeeder_ManualRequest"
fi

if [[ -f "$GATEWAY_UNIT" ]]; then
  grep -q 'LoadCredentialEncrypted=openhab-token:' "$GATEWAY_UNIT" \
    && pass "OpenHAB token is injected only into trusted gateway" || fail "gateway OpenHAB credential declaration missing"
  command -v systemd-analyze >/dev/null && systemd-analyze verify "$GATEWAY_UNIT" >/dev/null \
    && pass "gateway systemd unit verifies" || fail "gateway systemd unit verification failed"
fi

if ss -lntH | awk '{print $4}' | grep -q "${GATEWAY_IP}:${GATEWAY_PORT}$"; then
  pass "gateway listens on ${GATEWAY_IP}:${GATEWAY_PORT}"
else
  fail "expected gateway listener ${GATEWAY_IP}:${GATEWAY_PORT} not found"
fi

curl --fail --silent --show-error --max-time 3 "http://${GATEWAY_IP}:${GATEWAY_PORT}/healthz" >/dev/null \
  && pass "gateway health endpoint works" || fail "gateway health endpoint failed"
curl --fail --silent --show-error --max-time 3 http://127.0.0.1:5000/get_received_data >/dev/null \
  && pass "legacy weather read endpoint is locally available" || fail "local weather read endpoint failed"

# These are read-only. They do not command or actuate the feeder.
curl --fail --silent --show-error --max-time 3 "http://${GATEWAY_IP}:${GATEWAY_PORT}/v1/feeder/override" >/dev/null \
  && pass "gateway can read feeder safety state" || fail "gateway feeder-safety read failed"
curl --fail --silent --show-error --max-time 3 "http://${GATEWAY_IP}:${GATEWAY_PORT}/v1/weather" >/dev/null \
  && pass "gateway sanitized weather endpoint works" || fail "gateway sanitized weather endpoint failed"

printf '\nGateway binary hash (record in deployment log):\n'
sha256sum "$GATEWAY_BIN"

if command -v ufw >/dev/null; then
  printf '\nCurrent UFW status (review source-specific gateway rules):\n'
  ufw status numbered || true
fi

if (( FAILURES > 0 )); then
  printf '\nGateway preflight FAILED with %d issue(s).\n' "$FAILURES" >&2
  exit 1
fi
printf '\nGateway preflight PASSED. No physical feeder action was attempted.\n'
