# Production Cutover and Rollback — After the Parallel Pilot

Effective 2026-09-14; tracker #16. Begin with
[parallel-live-pilot.md](parallel-live-pilot.md).

The operator will decide when live results at **herd@feeder.lightning-goats.com**
are good enough to point established DNS, Nostr profile metadata and other references
at the new stack. Completion of every historic issue/audit exercise is no longer a
blanket precondition. This does not erase a known payment/physical correctness defect
or waive repository protections. It does not execute a cutover.

## The useful readiness check

Before the operator switches public references, establish that the intended new path
accepts real payments exactly once, feeds/debits as intended with local safeguards,
retains state across restart, and has an accessible stop/rollback path. Review any
actual limitations together. Verify the installed source/config, existing credential
boundaries and outstanding payments/feeding state; reuse valid prior test evidence.

Weather polish, optional CI work and a complete infrastructure redesign do not block
the decision. Validate additional goat addresses as they become publicly routed; do
not require six paid transactions. Keep no-spend Strike authority, the home-only
OpenHAB token, protected runtime and gateway-only access. Keep the existing account
balance/sweep policy; no sweep or spend-capable daemon is introduced here.

## 1. Preserve the financial epoch and arbitrate dispatch

The pilot database already contains **real** invoices, payments, credit, feed UUIDs
and possibly signed Nostr events. Back it up consistently and keep it as the new
system's database. Do not start from zero again or import its receipts a second time.
If moving storage, quiesce the affected services and preserve outstanding identities;
a stale restored database is not proof that no later feed happened.

Pause old invoice creation/feeder dispatch as needed for the operator's switch.
Keep reconciliation for old issued invoices that may still settle. Record any earned
legacy credit and resolve it explicitly; do not silently delete it or blindly copy
it into the new ledger. Observe DNS caches and old outstanding invoices, not just
recent traffic volume. Keep one physical owner and no competing bypass dispatcher.
Do not disable household-wide safety or erase an ambiguous attempt to unblock cutover.

## 2. Prepare the final public origin and routes

Keep the pilot hostname working while outstanding invoices/callbacks may still be
used. Prepare the final TLS/nginx origin and `lnurl.public_base_url` for the public
address the operator chooses. The eventual registry remains:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All credit the herd pool and preserve the paid user. Unknown users remain rejected.
Verify metadata/callback origin for new invoices and retain original stored metadata
for already-issued pilot invoices. Keep the appropriate webhook subscriptions/recovery
paths during overlap. Changing a DNS record alone does not migrate invoice identity.

Do not couple the payment cutover to an unrelated website rewrite or stream migration.
Route only names/services that the operator intends to move. Inventory their current
DNS values and nginx/stream dependencies before repointing the apex or `www`.

## 3. Operator changes public references

The operator changes the chosen DNS records and Nostr profile Lightning Address or
other metadata, along with overlay/QR links when appropriate. Record old/new values
and the switch time. Existing production references are not changed by pilot startup
or by this planning revision. Update public IPv6 records only if the path works.

Confirm a wallet resolves the intended address on the new origin, generates the right
invoice, and that a manual payment produces one credit and the expected downstream
behavior. Observe pending invoices on both old/new paths during the overlap. Do not
let two independent dispatchers double-fulfil payments or exceed the local feed cap.

Nostr payment/feed publishing and Nostr **profile metadata** are separate operations.
Use `active` mode only when normal public event publishing is intended. Do not replay
old pilot history en masse or re-sign persisted outbox events just because DNS changed.

## 4. WireGuard hub migration is optional and separate

If the pilot's existing gateway route is working, leave the old hub running through
the payment cutover. The payment switch need not wait for household hub migration.
Keeping the old VPS as hub means it cannot yet be retired.

If the operator later chooses to move the hub, use [wireguard-topology.md](wireguard-topology.md):
back up the configuration, stop/release old `10.8.0.1` before the new host claims it,
use the new host's own keypair, update clients, adjust the narrow home-gateway allowance
and verify required connectivity and recovery access. Never run two `10.8.0.1` hubs.
Do not treat a payment-DNS instruction as permission to repoint all WireGuard clients.

## Rollback or pause

**Pilot-only failure:** close new pilot invoice issuance and stop its dispatcher;
keep the old public system available. Preserve new issued requests and reconcile
late settlements in a non-actuating mode. The owner must resolve any possible delivery
before another dispatcher is re-enabled. Keep local override/stop controls authoritative.

**After public cutover:** the operator can restore old public references, but DNS
rollback does not undo received money or physical feeding. Preserve the new database,
reconcile outstanding invoices/credits/UUIDs and prevent concurrent fulfilment. Do not
restore an old snapshot over current state, zero balances, or repay/re-credit everything.
Keep necessary webhook/read recovery paths for invoices issued by either stack.

**Gateway-only problem:** stop new dispatch, mark feeding unavailable and choose whether
to pause invoice issuance too. Never replace the gateway with generic OpenHAB access or
pretend that a receipt ACK proves completed feeding. Repair the affected feature without
rebuilding all unrelated services.

Retain the old VPS/configuration and recovery material until the operator is comfortable
retiring it and it no longer supplies required hub/stream/other services. No mandatory
observation duration is imposed. Preserve private backups; remove unneeded access and
subscriptions deliberately, not as an automatic side effect of a DNS change.

## Completion is observation, not paperwork

Record which public addresses and services moved, actual payment/feed/restart results,
remaining known limitations, and rollback state. #15 records observed behavior; #16
records the actual operator cutover. Do not close unverified work as passed, but an
open historical issue is not itself a veto on the operator's hobby-project launch.
The former blanket audit HOLD is superseded for this pilot/cutover sequence by the
operator's 2026-09-14 decision; specific unsafe outcomes still require stopping their
physical/financial path rather than inventing success.
