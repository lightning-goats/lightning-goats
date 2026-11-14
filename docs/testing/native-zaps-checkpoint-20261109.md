# Native zap source checkpoint — 2026-11-09

Source: `0221d39` on `feat/cyberherd-20261109`. Merge `8ac38bc` incorporates main's
already-merged restoration PR #110 with an identical Git tree; no source change
was introduced by that ancestry merge. This is an opt-in source checkpoint. The
[full CyberHerd implementation](../planning/cyberherd-implementation-plan.md)
remains incomplete and no native zap feature was activated on the live daemon.

## Completed evidence

- Rust 1.88 `cargo fmt --all --check`: passed.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- `cargo test --locked --all-features`: 303 passed, zero failures, one existing
  real-NIP-46 test ignored for its dedicated isolated-network workflow.
- Nine native zap integration tests use the real receive adapter and durable store
  against local fake Strike endpoints: ordinary metadata compatibility, explicit
  opt-in, invalid-input zero provider contact, canonical/legacy-alias LNURL binding,
  all six paid-goat identities/shared pool, raw JSON invoice hash, duplicate and
  distinct-invoice settlement, signed-byte retry/reopen, signer outage, differing
  authoritative P2P amounts, immutable context and injected atomic storage failure.
- A small-zap test preserves 1-sat credit and the required empty protocol receipt
  without a below-10-sat kind-1/overlay announcement. Synthetic receipts are never
  treated as financial settlement evidence.
- Native signature/coordinate tests reject mutations, forged IDs/signatures,
  duplicate/unsafe tags, noncanonical identities and future/oversized inputs.
  Protocol signing freezes timestamp/content/tags, verifies the actual signature,
  and redacts malformed helper output. Kind 9734 is rejected before relay contact.
- Real installed `nak v0.20.6`, SHA256
  `b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9`,
  signed and verified an offline public synthetic request. It preserved the frozen
  timestamp, Unicode/control escaping and exact fields. The committed fixture is
  independently accepted by the native BIP-340 validator. Its JSON SHA256 is
  `4a32f3ac66d179cab9151ebf6b7e5a68f91e2c72eca67c6a8d13dc1312fdb9f9`.
- Real isolated nginx/ModSecurity/CRS acceptance: nine tests passed, including
  exact transport of a valid-size percent-expanded zap URI, rejection beyond the
  32 KiB request-line bound, existing attack enforcement, rate limits and logs.
  Command: `python3 -B deploy/waf/acceptance.py /usr/local/share/lightning-goats/coreruleset-4.30.0 /usr/lib64/nginx/modules/ngx_http_modsecurity_module.so`.
- `cargo tree -i rsa --locked --all-features`: no active reverse dependency.
- Fresh audit of 311 locked packages: failed only on existing
  `RUSTSEC-2026-0459` / `libsqlite3-sys 0.30.1`. No new ignore or workflow weakening.
  The [existing SQLite finding](../security/sqlite-wal-advisory-20261109.md)
  remains open. The single new TLS compatibility package is recorded in the
  [native contract](../architecture/native-nip57.md); other versions are unchanged.
- Staged source scan: 31 paths, zero production-secret pattern matches. New local
  documentation links and changed Python syntax checked successfully.

## Operational limits and next work

No real Lightning payment, credit grant, project/bunker signature, public Nostr
publication, feeding, DNS/WireGuard change, production schema migration or service
restart was performed. All financial/process tests use temporary databases and
local provider/owner fixtures. Offline nak uses publicly known fixture keys, not
the household signer. These results do not prove live NIP-57 receipt delivery.

The receive example leaves native zaps disabled. Runtime installation/backup,
the actual household kind-9735 round trip and relay acceptance remain separate
activation evidence. The overall goal still requires authenticated stream/note/
profile ingestion, local core bridge, membership worker, real-fund reward plans,
isolated payout worker, exact conversation outboxes, lifecycle/reset and operator
interfaces. Existing core financial and physical authority are preserved.

PR maintenance observed restoration #110 already merged at `18e0261`. Other older
PRs were not blindly merged: #109 has a failed cargo-audit check; older ready-marked
PRs have conflicts or outstanding review requirements. Source integration is not
host activation, and the preserved Monero work remains on hold.
