# Parallel Live Pilot — herd@feeder.lightning-goats.com

Decision date: **2026-09-14**. Owner: operator. Tracker: #6; observations: #15.

## What the operator selected

Bring the new stack online on its own hostname, use **real Strike mainnet payments**
from the operator's wallet, observe actual feeding and presentation, and fix what
fails. Keep the old production site/payment system available. When satisfied, the
operator will change established DNS, Nostr profile metadata and other references.

This plan supersedes blanket production HOLD, sandbox-first, all-issues-closed and
full-matrix-before-first-payment requirements in older handoffs/audit addenda. It
reclassifies the work; it does not claim earlier failures passed. Source tests,
credential protections, correct accounting and local physical limits remain useful.
No fixed two-payment, 220-sat or 30-minute restriction was approved. The operator
chooses manual amounts/duration within configured limits; no automated real payments.

The current request changes planning documents. No DNS, service, credential, payment,
public Nostr or feeder operation is performed by this edit. Later pilot execution
is one coordinated setup/test session, not a series of per-payment approval gates.
The later switch of established public addresses remains the operator's decision.

## Intended layout

```text
herd@lightning-goats.com         -> old system (unchanged during pilot)
herd@feeder.lightning-goats.com  -> new VPS nginx/TLS -> lightning-goatsd -> Strike
                                                        |
                                                 existing home gateway
                                                        |
                                              ONE existing feeder owner
```

Only the pilot subdomain points at the new VPS initially. Keep the old hub at
`10.8.0.1`, and use the new VPS's distinct inventoried WireGuard identity/address.
No new hub migration, wildcard DNS change or household-wide firewall redesign is
needed. Preserve the current site/stream and Nostr profile Lightning Address until
the operator chooses to switch them.

## Small preflight, not a second project

Before offering the pilot invoice, check four things:

1. **Correct destination:** valid TLS for `feeder.lightning-goats.com`; herd discovery
   and callback point to the new daemon, not old LNbits. Unknown users fail and
   normal method/body/rate limits remain. Publish AAAA only if IPv6 actually works.
2. **Credentials and state:** non-admin runtime, protected receive/read-only Strike
   credential, no spend key/OpenHAB token on VPS; fresh durable pilot database,
   never a synthetic fixture or copied old ledger. Record the selected source and
   installed binary hash. Keep subsequent real payments in that database.
3. **The enabled path works:** use an already-tested selected build and inspect
   concrete unresolved findings relevant to it. Verify authoritative settlement,
   not just a webhook or wallet display. An unavailable feeder path may remain off
   for the first payment, with that limitation made explicit.
4. **Stop/recovery:** know how to close pilot invoice ingress and stop the new
   dispatcher without damaging old services; preserve records and local feeder
   stop controls. Have a private backup before changing existing installation files.

Do not wait for every old issue to close or repeat every CI/namespace/systemd
rehearsal to do these checks. Do not weaken existing sandbox/secret controls merely
to make a broken install start. Fix a concrete failure in the path being used.

## Configuration and public routes

Keep the existing six-user registry (`herd`, `dexter`, `rowan`, `cosmo`, `newton`,
`nova`, all `credit_pool=herd`): the current validator requires it. Initially allow
only **herd discovery and callback** at the pilot nginx edge and reject other users
there. This avoids a new registry feature or six live-payment tests to start.

Use the existing configuration shape with the pilot-specific origin and database.
These are settings to adapt, **not a complete config or an already-installed file**:

```toml
[service]
# Retain the selected pilot loopback port.
mode = "canary"

[lnurl]
public_base_url = "https://feeder.lightning-goats.com"
# Retain the configured invoice expiry and registry limits.
```

The public discovery path is `/.well-known/lnurlp/herd`; the callback is
`/lnurlp/herd/callback`; the Strike webhook path is `/api/v1/strike/webhook`;
overlay/status remain `/ws/overlay` and `/api/v1/status` on the pilot virtual host.
Use the existing nginx include/upstream layout, with the herd-only edge restriction.
Do not expose generic admin/OpenHAB routes. Check metadata and BOLT11 description
hash against the **pilot domain**, not `lightning-goats.com`.

| Runtime mode | New feeding | Public Nostr | Use here |
| --- | --- | --- | --- |
| `shadow` | Off | Off | Optional first payment/ledger check |
| `canary` | Enabled subject to gateway/local safety | Off | Initial supervised real-feed pilot |
| `active` | Enabled subject to gateway/local safety | On | When the operator wants normal payment/feed posts too |

`canary` is not inherently harmless: check the actual gateway/owner target. Before
switching from shadow, inspect accumulated credit, feeds due and unresolved requests;
otherwise several earned feeds could run at once in sequence. Do not reset credit.
A fresh pilot database avoids importing old unresolved attempts; shadow still performs
same-UUID reconciliation of existing attempts, so it is not a network isolation tool.

Use the existing production Strike API. Configure a separate pilot webhook subscription
when the live session covers webhook delivery; do not replace another application's
subscription. Runtime receives its verification secret, not webhook-management/spend
authority. If registration is delayed, the existing issued-request recovery scanner
can support the first payment check. Mark live webhook delivery **not yet tested**;
never substitute a synthetic callback for genuine provider acceptance.

## Parallel sites must not become competing physical owners

Low reported traffic makes manual observation practical, but is not a lock. Check
that both old and new feed requests pass through the **same existing owner** with
shared local interval/cap enforcement and persistent UUID duplicate protection.

If the legacy path bypasses that owner, pause **only the legacy feeder dispatcher**
while the new pilot feeds. Keep the old site/payment service and any earned legacy
credit intact. If both paths cannot be coordinated safely, leave the new physical
path off while payments/overlay are tested; do not create a second actuator owner.

Before the first live feed, confirm the selected existing owner protocol understands
the request and returns correlated **completion**, not merely receipt. Keep override,
remote enable, minimum interval and absolute feed cap authoritative locally. The
VPS uses only the narrow gateway, without generic home access or its OpenHAB token.
This is a quick check of the installed path, not a requirement to complete a new
owner/retention architecture before any payment can be tried.

Capacity/exhaustion stays fail-closed. A bounded pilot may use the deployed retention
limit; stop before that capacity is exhausted. Do not prune old UUIDs or reset databases
to keep going. Full-host/co-restore recovery experiments and destructive failure
injection belong in isolated tests, not a live feeder trial.

## Run it and observe

Have the operator resolve **herd@feeder.lightning-goats.com** in their wallet and pay
the invoice generated by the new path. Confirm provider completion and exactly one
matching credit/event. Then send amounts chosen by the operator across the configured
feed threshold and watch the intended feed, same-UUID completion, one debit and the
remaining balance. Keep a simple record of actual outcomes and failures.

At a 1000-sat threshold, 2340 sats with zero starting credit should yield two confirmed
feeds and 340 remaining. Account for earlier pilot credit; do not promise two feeds
from the same payment when the starting balance differs. Stop on duplicate credit,
unexpected/extra feeding, amount mismatch or unresolved delivery. Do not pay again
blindly because a UI timed out, and do not send a fresh UUID to resolve ambiguity.

Restart the **pilot process only** and verify settled payments do not gain another
credit and confirmed feeds do not dispatch again. Check replay using the same known
UUID/authoritative status, not a new feed request. Retain existing synthetic failure
coverage instead of deliberately creating dangerous real-world faults.

Preview the pilot overlay separately from the live OBS scene. Weather can stay off
or unavailable without blocking payments/feeding; when enabled it needs real source
observation time and stays overlay-only. Public Nostr is off initially via canary
mode; enabling active mode is a deliberate user-visible choice. Do not rewrite the
profile metadata or replay all historical pilot messages merely to test Nostr.

## Record enough, then stop adding gates

Record source/binary/config identity, starting/ending credit, payment and feed counts,
restart behavior, any unresolved UUID, and whether webhook/Nostr/weather were actually
exercised. Keep keys, full invoices, payment preimages and account details out of public
issues. A short sanitized checkpoint suffices; no new evidence framework is required.

The operator decides when observation is sufficient for cutover. There is no mandatory
observation duration, number of payments or requirement to close all historical audit
items first. Known limitations remain listed rather than falsely marked passed.

## Pause and later cutover

To pause, close **new pilot invoice issuance** and stop its dispatcher. Retain the
pilot daemon database, issued invoices, pending requests and signed outbox; already
issued invoices may still settle, so preserve a safe reconciliation path. Keep the
old system available. Resolve possible physical delivery before switching dispatchers.

Pilot money and feeds are real: do not discard the ledger, re-credit it into old
LNbits, restore stale snapshots as current, or erase pending UUIDs. At cutover, keep
the pilot ledger as the new system's ledger and reconcile outstanding old invoices
and legacy credit explicitly. The operator then changes established DNS/profile
references. See [production-cutover.md](production-cutover.md). Moving the WireGuard
hub is optional and separate; never run two hosts as `10.8.0.1`.
