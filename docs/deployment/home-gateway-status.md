# Home gateway status — 2026-09-12

**Local canary ready; network and production acceptance blocked.** Production
HOLD remains. Both original checkouts and the live physical owner are preserved.
Home tooling/review: [PR #59](https://github.com/lightning-goats/lightning-goats/pull/59).
Coordination: issue #17 comment thread, with durable context in Hexmem task97.
The VPS agent owns daemon/Strike/nginx/website/Nostr/VPS networking; independent
review is requested, not self-certified by the home owner.

```yaml
source_commit: a74890c47a57a13508b3d259929f1f1bec2a5cc3
tooling_commit: b2f42dc39758d21d96068bcb0382db5314960908
home_host: 10.8.0.6
stage: local-canary-ready
canary_url: http://127.0.0.1:8790/
canary_service_active: true
canary_service_enabled_at_boot: false
canary_remote_enabled: false
production_url: http://10.8.0.6:8789/  # reserved future endpoint, not listening
production_service_active: false
production_remote_enabled: null     # Item absent; fails closed, not modified
owner_protocol: feeder_request_v1
owner_script_sha256: 730053e0f3245cb83461e3fe6e3b05d49c8b508631e8cdb4a889c8be8d915978
owner_finality: blocked
canary_protocol: uuid_canary
request_item: GoatFeeder_ManualRequest
result_item: GoatFeeder_ManualResult
credential_name: openhab-token
credential_shared_with_vps: false
verified_runtime_role: user
weather: unavailable
network_policy: proposed
next_vps_action: Review PR59 and reply on issue17 with source/protocol and authenticated staging peer/path proposal; do not connect or seed a network canary yet.
```

The production request/result names above are a future binding, never used for
a command test. Canary uses only `LightningGoatsCanaryRequest`, `Ack`, `Override`,
`RemoteEnabled` and `Count`. Its exact rule digest is
`a8ed99fb3140d785c13c3f628372a0146bddcb85970151a9d7a3af57afbb32ab`.
The real `FeederOverride` stayed unchanged. No production gateway was started.

Installed ELF64 x86-64 gateway SHA-256:
`1833948b9ffb5e24057b9107d6ee044f408565f36322644885297e0bd77b42a5`.
Rust 1.88.0 build on Ubuntu 24.04.4, systemd255, OpenHAB5.2.1; libraries resolve.
NTP synchronized; SQLite is on local ext4. Binary/config/units are root-owned;
locked `lightning-goats-gateway` and `lightning-goats-gateway-canary` users have
no sudo, extra groups or capabilities. Canary systemd permits loopback only.
Production and canary configs/state have separate directories matching those
user names, with `config.toml` and `gateway.db` respectively.

Separate OpenHAB users: `lightning_goats_gateway` and
`lightning_goats_gateway_canary`; labels `lightninggoatsgateway` and
`lightninggoatsgatewaycanary`. OpenHAB rejected hyphenated labels; both resulting
USER tokens passed Item GET200 and administrative rules GET401. Ciphertexts are
root-only under `/etc/credstore.encrypted/lightning-goats-gateway-openhab` and
`.../lightning-goats-gateway-canary-openhab`. Service-context decryption passed.
These are coarse USER rights, not per-Item authorization. Existing implicit
USER role permits unauthenticated Item reads; invalid-token reads return401.
No global API-security setting changed. Rotation/rollback is in the
[home installation guide](https://github.com/lightning-goats/lightning-goats/blob/b2f42dc39758d21d96068bcb0382db5314960908/docs/deployment/home-gateway-installation.md).

| Check | Observed result |
| --- | --- |
| Rust baseline | Format, locked strict Clippy, 150 all-feature tests, release build and RSA reverse-tree passed; exact baseline CI/Security green |
| Home artifacts | 50 deployment tests and 3 JS fixture tests passed; exact final tooling CI must be checked on PR59 |
| Real harmless rule | Two direct same-UUID fixture commands counted separately, both acknowledged |
| Real gateway safety/replay | Remote-OFF POST423 `not_dispatched`; refusal replay remains423 after enabling; confirmed POST200; duplicate and restart replay200 with no extra command |
| Concurrent UUIDs | One confirmed200 and one unresolved refusal423; exactly one additional command |
| Local count/state | Counter2→4 during gateway tests; two acknowledged requests, two durable refusals; remote returned OFF |
| HTTP reads | `/healthz`200; `/v1/feeder/override`200 with false/false; `/v1/temperature`200 with `temperature_f:null`; `/v1/weather`502 `Weather data unavailable` |
| Synthetic invalid credential | Separate temporary system unit: health200, safety502; no command requests; unit removed and evidence state preserved |
| Backup/restore | Quiesced nonempty canary store integrity OK; restored SQL dump matched, including acknowledged requests/refusals; active store never replaced |

Every feeder response includes the matching `request_id` and typed `status`.
GET `/v1/feeder/request/<uuid>` recovers the original UUID without resending;
unknown UUID404, pending202 and ambiguous409 semantics are source/mock-tested.
Canary caps: 5-second minimum, 5-second acknowledgement timeout, 100ms polling,
60/hour. Production inactive config retains 30-second minimum, 20-second timeout,
250ms polling, 10/hour; sizing and activation require approval.

Weather is honestly unavailable: live `/get_received_data` returns 31 normalized
Item-name fields with no observation timestamp and none of the required station
keys. The installed adapter fails closed. The optional real temperature Number
Item is unitless, so it is omitted. No receiver mutation/rebind/restart occurred.
Stale/future/regression and explicit-unit cases pass isolated Rust fixtures;
those do not repair the actual receiver contract.

Remaining: independent PR59 review; approved authenticated staging peer/path and
home policy; coordinated real-daemon/gateway 2340-synthetic-sat cross-host test;
weather observation contract; physical-owner finality and actual-runtime/JDBC
acceptance under existing PR57; production sizing/remote Item/activation and
separately approved physical acceptance. No physical test, payment, DNS change,
WireGuard change or household firewall change occurred. The separate
[staging/final policy plans](https://github.com/lightning-goats/lightning-goats/blob/b2f42dc39758d21d96068bcb0382db5314960908/docs/deployment/home-network-change-plan.md)
preserve legacy access during staging and do not mislabel PR57's full-interface
restriction as legacy-compatible. Network acceptance remains untested.

Sanitized evidence: [home-gateway-20260912.json](../testing/evidence/home-gateway-20260912.json).
