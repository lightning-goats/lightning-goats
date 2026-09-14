# New VPS Runbook — Parallel Live Pilot

Effective 2026-09-14. Start with [parallel-live-pilot.md](parallel-live-pilot.md).
This replaces the former full-audit staging sequence. It is a setup plan, not a
record that the host is installed or that live actions have been performed.

## 1. Use the existing deployment, not a new infrastructure project

Inventory the new VPS's current installation and choose its exact source/build.
Use available reviewed fixes for the enabled path; do not recreate existing PRs,
install Codex as a prerequisite, rebuild the server for an optional hardening pass,
or introduce another Lightning backend. Keep the old site/payment services intact.

Runtime stays non-admin with protected code/config and receive-only Strike secrets.
No OpenHAB token, old CLN/LNbits secret or spend key belongs in the new runtime.
Use the existing artifact/systemd mechanisms and keep private backups of changed
installation files. Never claim a merged binary is installed without checking it.

## 2. Add only the pilot hostname

Prepare `feeder.lightning-goats.com` A and, only when working, AAAA to the new VPS.
Obtain valid TLS and a dedicated nginx virtual host. Leave `lightning-goats.com`,
`www`, existing Lightning Addresses and Nostr profile metadata unchanged.

Adapt the existing nginx HTTP includes, upstream and server snippets; validate the
assembled configuration before reloading it. Expose only herd discovery/callback,
the constrained Strike webhook, and the needed pilot overlay/status paths. Reject
other users at the pilot edge while retaining the six configured application users.
Keep the daemon loopback-bound. Do not turn on an unrestricted generic reverse proxy.

Set `lnurl.public_base_url` to `https://feeder.lightning-goats.com`. Verify discovery
returns `/lnurlp/herd/callback` on that hostname and that the actual BOLT11 commits
to its metadata. Retain existing invoice amount/expiry and abuse controls.

## 3. Keep pilot financial state separate and durable

Start the pilot with its own empty durable SQLite database and distinct service/config
paths. Do not reuse synthetic fixture state or copy the old production ledger.
Once any real invoice is issued, retain that database across restarts and later cutover.
Payment-source IDs/request hashes must bind only locally issued pilot invoices.

Install only the necessary receive/read Strike key and webhook verification secret
through protected credential storage. A pilot webhook subscription is additive; do
not redirect/delete existing subscriptions. If genuine webhooks are not ready, use
the existing recovery scanner for a clearly labelled payment/recovery check. Live
webhook delivery remains a follow-up observation, not a claim from synthetic data.
No sandbox, automatic payment sender, withdrawal or sweep implementation is needed.

## 4. Connect the existing home boundary for feeding

Keep the old WireGuard hub at `10.8.0.1`; use the new VPS's own verified key and unused
staging address. Do not move existing clients or widen the home network policy.
The new application reaches only the existing narrow home gateway. Verify that the
specific path works and that generic OpenHAB/weather/admin access is not exposed;
reuse prior compatible containment tests instead of repeating every network laboratory.

Confirm the gateway is bound to the intended existing physical owner, not a canary
fixture or guessed protocol. Require correlated completion, duplicate-UUID protection
and working local override/enable/interval/cap controls. Keep the home token there.
If old dispatch bypasses this shared owner, pause just that dispatcher during pilot
feeding; preserve old intake, credits and data. Do not enable competing actuator owners.

## 5. Run manual payments and live observations

An optional initial `shadow` payment verifies the real provider/ledger without new
feeding or public Nostr. Move to `canary` for supervised real feeds once the actual
owner path works. Review accumulated feeds due and any unresolved attempt first.
`active` additionally enables public Nostr and needs the intended signer/relay setup;
canary is sufficient for initial live feeding and overlay testing.

The operator pays `herd@feeder.lightning-goats.com` from their wallet, choosing amounts
and duration. Check one credit per receive, correct threshold/remainder, one debit per
confirmed feed and no duplicate after a pilot restart. Use the pilot overlay separately
from the old OBS scene. Leave weather disabled if unavailable; it must not block money.
Do not reset paid state to get a visually clean test.

For an unexpected financial/physical result, pause the affected pilot feature, retain
evidence and repair the specific problem. No fresh UUID after an ambiguous delivery;
no automatic repeat payment after a timeout. A reached owner capacity limit is a stop,
not permission to prune replay identities. Keep exhaustive failure injection isolated.

## 6. Hand over the result, not another gate list

Return source/binary/config pin, actual payment/feed/remainder results, restart result,
remaining limitations and the way to pause the pilot. Use
[the verification matrix](../testing/phase1-verification-matrix.md) to distinguish
initial checks from pilot observations and optional follow-up. No complete-issue-list
or formal per-payment approval is required.

When the operator is satisfied, use [production-cutover.md](production-cutover.md)
for their DNS/Nostr-reference switch. Keep the real pilot database. WireGuard hub
migration and old-VPS retirement are separate choices, not dependencies of that
first live pilot. This document itself has applied no host or credential changes.
