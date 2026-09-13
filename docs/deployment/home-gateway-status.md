# HOME gateway status — 2026-09-13 UTC

**Local canary verified; production HOLD.** HOME retains gateway/credential,
canary, weather and containment preparation ownership. VPS owns daemon, Strike,
nginx, website, Nostr and VPS networking. Issue #17 is the coordination thread;
Hexmem holds private context. Existing checkouts, credentials and physical owner
are preserved. Unrelated untracked `reports/` in the original checkout is untouched.

## Current checkpoint — review and staging handoff

Current integrated main is `ee0d2a8f4faec0324eff26e07d956b21386281f7`:
weather PR78 is merged with tested tree `876154046e0468a43313fbfde8b105323e19d636`.
This is source integration, not installation or real observation acceptance.
The runtime observations below retain their original evidence scope; the fresh
service check confirms held-canary and production gateway **inactive/disabled**.

PR81 witness head `f644e6aaa5499c235ecd59c29e741a8ac99b9a86` has passing
published checks, including Rust job103788271737 and audit job103788271670.
VPS source acceptance is recorded in PR81 comment5656072623. The library-only
witness remains uninstalled: initializer, enablement, owner-side sustained
retention and full-host restore reconciliation are outstanding. Its160-identity
fixture does not prove that the physical owner retains more than32 receipts.
The operator stopped the resource-heavy local Rust build; that interrupted run
is not a passing result and will not be restarted as part of this handoff.

HOME independently reviewed PR82 `6cd52ba6ca169e86b4454e72ad1d40fb824f6d57`.
Nine Python tests passed locally; exact-source Rust CI job103798255711 was
inspected. [Changes requested](https://github.com/lightning-goats/lightning-goats/pull/82#pullrequestreview-5192556257):
the offline checker accepts debits preceding their seed credit and payment events
following confirmations. Both false passes were reproduced with disposable
fixtures. VPS owns the correction; no shared gateway source was edited.

**Next VPS action:** publish the PR82 chronology correction with regressions and
an exact current-main candidate for HOME re-review. Retain the v1 transaction
unchanged. HOME agrees with the proposed separate v2 transaction sequence in
[#17 comment5656301976](https://github.com/lightning-goats/lightning-goats/issues/17#issuecomment-5656301976):
authenticate reviewed tools, require fresh full baseline equality under an
exclusive window, stop on drift, review the new snapshot and prepare a separate
v2 transaction with its own rollback evidence. This acknowledges the preservation
contract; it does not authorize application or establish baseline equality.

Before cross-host execution, pin the corrected candidate, fresh HOME held-fixture
baseline, exact authenticated peer/filter manifest, negative-access checks and
paired-state restore procedure. VPS reports harmless-staging approval in its
conversation; HOME household policy application still requires its specified
operator approval. Keep credentials on HOME and release only the agreed UUID.
Local canary success remains separate from cross-host containment acceptance.

| Source / deliverable | Exact commit | State |
| --- | --- | --- |
| Installed gateway | `a74890c47a57a13508b3d259929f1f1bec2a5cc3` | Installed; production inactive |
| PR59 helper corrections | `f1cef47bf65660eb48bde81ca7bfcb0d48bb4252` | VPS independently accepted; merged into main `df34bcdea5a178bb72193d1a431ea3a9f3e0c9f1` |
| [PR65 containment](https://github.com/lightning-goats/lightning-goats/pull/65) | `e350ee7a9db3e37b51fa4345e90cd8815f8ed454` | VPS source review accepted and merged; not applied |
| [PR71 weather](https://github.com/lightning-goats/lightning-goats/pull/71) | `cb7bde28ceb7bb2aadcaa566192422170742976c` | Accepted and merged; all exact-head CI passed; not installed |
| [PR72 owner correction](https://github.com/lightning-goats/lightning-goats/pull/72) | `bd59a055031f899787818592a633d6639a165871` | Accepted and merged; tested in unlinked fixture; physical owner unchanged |
| [PR76 v2 adapter](https://github.com/lightning-goats/lightning-goats/pull/76) | `bdd5a534a91015cdfadfc7f2f4fdc6ca69a74865` | Implemented; synthetic restart and actual read-only USER/JDBC recovery passed; VPS source review accepted; not installed |
| [PR77 held fixture](https://github.com/lightning-goats/lightning-goats/pull/77) | `3b16b7a85e0a2ab88b42562c78a8b7dde728513f` | Helper evidence-loss correction published; local Rust/helper tests and all 14 replacement CI jobs pass; VPS independently accepted replacement |
| [PR78 weather reader](https://github.com/lightning-goats/lightning-goats/pull/78) | `a1aa8dca34d2a25b9f8a87ccca8adea024355495` | Read-only permission regression fixed; merged via tested current-main integration; not installed |
| [PR79 held gateway](https://github.com/lightning-goats/lightning-goats/pull/79) | `daeb9ba0acfd4b506547aba8049a3f1a9aae3cc3` | All 13 CI jobs pass; installed; local held/restart/recovery passed; stopped/disabled |

```yaml
home_host: 10.8.0.6
canary_url: http://127.0.0.1:8790/
canary_protocol: uuid_canary
canary_active: true
canary_enabled_at_boot: false
canary_remote_enabled: false
canary_count: 6
production_url: http://10.8.0.6:8789/ # reserved future endpoint; not listening
production_active: false
production_enabled_at_boot: false
production_remote_item: absent # fails closed
configured_owner_protocol: feeder_request_v1
candidate_owner_protocol: feeder-request-v2 # not supported by installed adapter
credential_role: USER
credential_shared_with_vps: false
weather: unavailable
network_policy: proposed
```

Live owner digest rechecked unchanged:
`730053e0f3245cb83461e3fe6e3b05d49c8b508631e8cdb4a889c8be8d915978`.
Real `FeederOverride` remains OFF. Installed gateway binary SHA-256:
`1833948b9ffb5e24057b9107d6ee044f408565f36322644885297e0bd77b42a5`.
Ubuntu 24.04.4, OpenHAB/JS/JDBC 5.2.1, Java21.0.12, systemd255, Rust1.88.0.
Separate non-admin system users, root-owned units/config/binary and encrypted
systemd credentials are installed. Dedicated gateway/canary OpenHAB USER tokens
passed Item GET200/admin GET401; neither token is in this document or on VPS.

| Test actually run | Result / evidence |
| --- | --- |
| PR59 helper boundaries | 59 Python +3 JS tests; redirects, inspected target and isolated rehearsal regressions; exact-head Rust/Security/Deployment passed |
| Existing real loopback canary | Two revised-run confirmations, duplicate/restart no resend, refusal replay423, concurrency bounded; count4→6, remote returned OFF |
| Canary backup/restore | Quiesced nonempty store integrity and restored SQL dump matched; active store preserved |
| PR65 isolated containment | 11 packet cases, table-only rollback and2 drift tests passed; not cross-host acceptance |
| PR71 weather | Rust1.88 format/strict Clippy/all-feature tests, 69 Python tests including2 real gateway process tests passed; exact-head Security passed |
| PR72 owner | 18 JS +3 helper regressions passed; real unlinked OpenHAB/JDBC: one ON and durable complete, exact UUID duplicate gave zero additional ON |

The owner correction adopts PR57's existing slice, separates command ingress and
receipt ledger, waits for actual Item/JDBC readback, and fixes slow-persistence
cooldown. The 33rd distinct request fails closed; oldest UUID survives restart.
**No compaction or sustained-retention acceptance.** Runtime fixture bindings,
source hash, zero-based JDBC paging correction and replay evidence are documented
in [the exact candidate](https://github.com/lightning-goats/lightning-goats/blob/bd59a055031f899787818592a633d6639a165871/docs/deployment/home-owner-v2-candidate.md).
No actual OpenHAB restart/crash or physical-owner replacement was performed.

Weather inspection found32 normalized cached fields without observation time;
field count varies and is not freshness. PR71 pins the deployed receiver/radio
sources and prepares a coherent radio-decoder UTC recorder with explicit units,
persistent timestamp and separate loopback exporter. Real gateway tests reject
the current schema, missing/stale/future time and restored older producer state.
The shared producer and existing receiver remain unchanged. Real-frame timestamp
verification, source-interpretation ACK and installation approval remain open.

**Earlier handoff (superseded by the current checkpoint above):** PR78 review,
held-canary harness binding and the witness scope request were pending. VPS accepted PR76 at `bdd5a534` and explicitly acknowledged
the final generation2 Request/Ack binding in comment5655694424. The shared
`src/openhab.rs` scope is implemented; VPS-owned `tests/gateway_admission.rs`
remains untouched. V2 recovery requires a committed
same-UUID JDBC receipt and sends no command. Six focused tests pass, including
real gateway timeout/restart with one total command. Actual unlinked read-only
probe returned Complete using the dedicated canary USER; fixture counts stayed1.
Details and evidence are in PR76's `home-owner-v2-adapter.md` and
`docs/testing/evidence/home-owner-v2-reader-20260913.json`. Local Rust1.88 full
gates pass; PR76 exact-head CI passed.

The separate `LightningGoatsHeldCanary2*` fixture is now installed locally with
Hold ON, remote OFF, count2, and two released receipts. Real testing held the UUID
beyond6 seconds, released it and replayed release without a second delivery.
The prior failed held generation remains Fault ON with its evidence preserved;
its JDBC DecimalType mismatch was fixed and regression-tested in generation2.
No existing echo canary or physical owner was replaced. Full control/evidence is
in PR77. No new gateway listener or remote control endpoint was enabled.

VPS review reproduced the PR77 helper truncating its sole UUID evidence record
on a failed stage write. Replacement `3b16b7a` keeps the pre-dispatch intent
immutable and atomically publishes a separate `.progress` snapshot. Seven helper
tests pass, including six before/during-write failures at held/released/passed,
pre-dispatch directory-fsync failure and existing-evidence preservation; nine
Node fixture tests pass. The new full local Rust gate passed; 89 deployment tests completed with two
explicit weather-process skips covered by the passing real-gateway CI job. The local audit
subcommand was unavailable; exact replacement CI Security job103782810092
executed the audit successfully (310 locked dependencies; RSA unreachable). All
14 replacement CI jobs passed. These correction tests are isolated mocks and did not
submit another request to either deployed fixture.

PR79 prepares `uuid_held_canary` restricted to the final fixed generation2 pair,
a separate Unix identity/database/unit and loopback port 8791. It deliberately
uses the existing canary OpenHAB USER credential; it does not claim a distinct
OpenHAB authorization scope. All 13 exact-head CI jobs passed. Fresh inactive installation and separate verifier passed at `daeb9ba`. The new
locked nologin identity has no sudo authority and only its primary group; its
0700 state directory now contains one acknowledged request and one durable
refusal, integrity ok. The unit is inactive/disabled, MainPID0, with no
listener on8791. Original echo remains active; production remains inactive.
VPS independently accepted PR77 correction in comment5655799705. A bounded
GET-only startup passed installed-process identity/capability verification and
real credential/safety reads (both switches OFF); health200, temperature200/null,
weather502/unavailable. No feeder POST or switch mutation occurred. The service
was stopped after checks. The project lead accepted PR79 source863cfab in formal review5192093821.
Actual local held/restart/release then passed with exactly one extra delivery
(count1→2). The first test expected ambiguous instead of documented202/pending
after restart; it stopped safely, preserved evidence and continued the same UUID.
GET recovery and POST replay confirmed without another delivery. Remote OFF,
service stopped/disabled; no physical action. Evidence is in PR79 docs-only
`289274e`. Cross-host accounting/containment and paired restore remain open. PR79 is based on PR76 and must be
revalidated on current main when its parent is integrated.

Weather preparation found the WAL reader needed sidecar write access after the
writer closed. PR78 uses rollback journal/FULL, preserves the atomic timestamp
transaction and passes a real read-only identity/filesystem regression plus both
gateway process tests (11 weather tests total). The unit and source-pinned
absolute-module hook are prepared; both existing weather services are unchanged.

Sustained UUID retention/full-host rollback protection still requires coordinated
implementation and proof. A local witness cannot by itself certify freshness
after full-host restore; compaction stays disabled. Continue the reviewed
staging path/rollback and approved cross-host accounting acceptance separately.
Local success does not authorize network or physical activation.

Home policy application, shared weather-service changes, physical replacement
and production remain separately gated. No real feeder command, payment, DNS,
WireGuard or household firewall change occurred in this increment. Private peer
inventory and all secrets remain outside GitHub.

Earlier detailed evidence is preserved in
[initial HOME evidence](../testing/evidence/home-gateway-20260912.json) and
[helper recheck evidence](../testing/evidence/home-gateway-helper-review-20260913.json).
