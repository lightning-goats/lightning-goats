# Harmless cross-host staging canary

Production remains on HOLD. This procedure is limited to the operator-authorized harmless staging canary. It does not authorize real Strike payments, public Nostr publication, production DNS/WireGuard identity cutover, physical feeder actuation, owner replacement/compaction, or unrelated household/OpenHAB changes.

## Pinned scope

Use a reviewed current-main release containing the generation2 held fixture and isolated held-gateway binding. HOME retains the OpenHAB credential and exact-UUID release control. The VPS must never receive that token and must never invoke the HOME release action.

The staging path may use only the reviewed temporary staging WireGuard peer/path and containment policy. Do not claim or reuse the production `10.8.0.1` identity and do not disable the old production VPS. Roll back the staging listener/path on any identity, containment, provenance, or evidence mismatch.

## Required evidence before starting

1. Record current `main`, reviewed release/archive digests, installed binary/config hashes, service identities, and inactive/rollback state.
2. HOME records a fresh read-only held-canary baseline using the generation2 control helper with Hold ON and RemoteEnabled OFF. Preserve the exact JSON, source digest, stderr/stdout and exit status. Do not reset the retained journal.
3. VPS prepares a fresh synthetic session with `prepare_cross_host`: exactly 2,340 sats, one synthetic `payment_received` event, no provider request, no public Nostr outbox work, and a unique run UUID in `PREPARED.json`.
4. Verify the reciprocal staging peer identities and both containment policies before exposing the held gateway. Negative probes must confirm unrelated trusted-side services remain unreachable.

## Execution

Keep the physical owner path untouched. Start only the isolated held-gateway staging service and the synthetic VPS daemon/session.

With HOME remote control still OFF, verify the daemon's refusal/replay path causes zero new HOME deliveries. Then perform the approved harmless enable for the held fixture only.

Exercise two distinct request UUIDs from the synthetic 2,340-sat credit pool. HOME retains Hold ON and controls exact-UUID release. The acceptance sequence must demonstrate:

- exactly two new delivered UUIDs total;
- duplicate/replayed UUIDs create no additional delivery;
- unresolved/restart behavior keeps the original UUID and does not auto-resend a fresh command;
- at least one held request survives the intended restart/observation-loss path and completes only after HOME releases that same UUID;
- failure injection does not clear or rewrite evidence;
- the two authoritative confirmations debit exactly 1,000 sats each, producing balances 1,340 then 340.

For paired backup/restore coverage, quiesce the staging services, preserve both daemon and HOME canary state, restore only according to the reviewed staging procedure, and prove unresolved identity and delivery counts survive. Never reset a count, journal, UUID or database to make the scenario pass.

## Final offline correlation

After the scenario is quiesced, export a consistent synthetic daemon database and record the final HOME control snapshot. Run:

```sh
python3 deploy/scripts/verify-cross-host-accounting.py \
  --database /private/session/export/daemon.db \
  --baseline /private/session/home-before.json \
  --completed /private/session/home-completed.json \
  --run-id ORIGINAL_PREPARED_RUN_UUID
```

A successful verifier result requires the original HOME journal to be unchanged, exactly two distinct new nonhistorical UUIDs, two matching confirmed attempts/debits/events, one 2,340-sat synthetic receipt/payment event, final remainder 340 sats, no provider receive request, and no public Nostr outbox work. Every extra/duplicate/unresolved delivery or accounting mismatch fails closed.

The verifier is deliberately read-only and cannot prove capture provenance, network identity, service sandboxing or scenario execution by itself. Preserve the host-side command logs, timestamps, peer/containment checks, service/source pins, backup/restore manifests and HOME before/after captures alongside its output.

## Cleanup

Return the held fixture to RemoteEnabled OFF, stop the held-gateway staging listener, stop the synthetic daemon/session, and remove or disable only the temporary staging path according to its reviewed rollback order. Preserve all evidence and databases. Do not change production DNS, the old production VPS, real payment state or the physical feeder.

A passing harmless canary advances #15/#17 staging acceptance; it does not itself authorize a real payment, physical feeder test, or production cutover.
