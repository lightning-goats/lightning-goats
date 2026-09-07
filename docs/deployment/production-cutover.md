# Production Cutover and Rollback Runbook

Status: operator-gated production procedure.

Tracker: issue #16.

## Preconditions

Do not begin unless:

- issue #15 verification matrix has passed on the new VPS;
- the old VPS remains intact and recoverable;
- the new VPS has a distinct tested WireGuard identity;
- final production systemd units run under the non-admin `lightning-goats` runtime account;
- broad temporary Codex/deploy sudo has been revoked or narrowed;
- final production credentials are installed and audited;
- Strike credential is receive/read-only and cannot spend;
- feeder operator is available to control `FeederOverride`;
- a current archive exists of old LNbits/config/nginx/WireGuard/CLN recovery material.

## Cutover principle

The old VPS stays production-authoritative until the final switch.

The new VPS becomes authoritative only after:

1. legacy side effects are stopped;
2. WireGuard clients are repointed as required;
3. DNS points to the new VPS;
4. the new payment path is verified with a tiny real payment.

## 1. Freeze old feeder/payment side effects

Set the feeder safety override so automatic physical feeding is blocked.

Stop the old Lightning-Goats/LNbits feeder-side processes that could mutate goat-feeder accounting.

Archive/log the exact cutover timestamp.

Do not delete old databases or configuration.

## 2. Confirm new VPS readiness

On the new host confirm:

- nginx config valid;
- `lightning-goatsd` healthy;
- Strike API access valid;
- webhook route reachable;
- Nostr signer/publisher healthy;
- overlay status/WebSocket healthy;
- WireGuard healthy;
- OpenHAB is reachable only through the expected restricted path;
- SQLite state is the intended fresh Phase 1 accounting epoch.

## 3. Switch WireGuard hub/client peers

If existing clients currently use the old VPS as their WireGuard server/hub, update their server peer entry from the old VPS to the new VPS.

Normally this means changing:

```text
PublicKey = <old-vps-public-key>
Endpoint  = <old-vps-public-ip>:<wireguard-port>
```

to:

```text
PublicKey = <new-vps-public-key>
Endpoint  = <new-vps-public-ip>:<wireguard-port>
```

Preserve client private keys and client WireGuard addresses unless the approved topology says otherwise.

Confirm each required client handshakes with the new VPS and can reach only intended peers/services.

If the operator instead decides to reuse the old VPS WireGuard identity, first stop WireGuard on the old VPS and verify it cannot answer before activating that identity on the new host. Never run one peer private key on both hosts simultaneously.

## 4. Activate production nginx configuration

Install/reload the final production virtual hosts/routes on the new VPS.

Verify locally/directly before DNS change:

- static site;
- Lightning Address discovery;
- LNURL callback;
- webhook endpoint;
- overlay WebSocket;
- health/status.

## 5. Change DNS

Point required production names to the new VPS, including at least `lightning-goats.com` / `www` and any other still-required public hostname.

Record old and new DNS values and change time.

Monitor resolution from multiple resolvers if practical.

## 6. Verify public path

Once DNS resolves to the new VPS:

1. load `https://lightning-goats.com`;
2. resolve `herd@lightning-goats.com` through a real Lightning wallet/client;
3. request a tiny invoice;
4. pay a very small amount;
5. confirm Strike marks it completed;
6. confirm `lightning-goatsd` credits it exactly once;
7. confirm one durable `payment_received` event;
8. confirm templated Nostr publication;
9. confirm overlay message/animation;
10. verify no feeder actuation occurs while override is active.

If any financial-state invariant fails, stop and investigate before enabling the feeder.

## 7. Controlled feeder activation

After payment ingress/presentation is accepted:

1. arrange a controlled threshold condition;
2. verify `FeederOverride` is still active while checking state;
3. release the override only with operator approval;
4. confirm exactly one physical feeding for one due threshold;
5. confirm exactly one feed debit;
6. confirm exactly one durable `feeder_confirmed` event;
7. confirm templated feeder message on Nostr + overlay;
8. restore the desired normal override state.

For multiple due feeds, verify serialization and expected inter-feed delay.

## Rollback boundary

### Easy rollback

Before the new stack has accepted meaningful new-epoch payments, rollback may consist of:

- restore DNS to old VPS;
- repoint WireGuard clients to old VPS peer;
- keep feeder override active;
- restore only explicitly required legacy public services.

Do not automatically reactivate CLN routing capital as part of rollback.

### After new-epoch payments

Once the new Strike-backed system has accepted payments, do not perform a blind rollback that discards its SQLite feed-credit/event state.

Before routing payment traffic back elsewhere:

- preserve the new SQLite database;
- account for all accepted payments and resulting feed credit;
- reconcile any unresolved feed attempt;
- document how those credits remain authoritative.

The new Phase 1 ledger is authoritative for payments accepted after its production cutover.

## Old VPS retirement

After successful cutover:

- power down/disable unnecessary old services;
- retain the old VPS intact for an observation period;
- retain LNbits data read-only for historical audit as desired;
- retain CLN recovery material offline;
- retain old nginx/WireGuard configs in an encrypted/archive location;
- do not destroy the old Vultr instance until backups and new production behavior have been verified.

After the observation period, destroy/downgrade the old VPS to stop billing.

## Final acceptance

Phase 1 is complete when:

- production Lightning Address uses native LNURL-pay + Strike;
- no LNbits/CLN/clnaddress runtime path serves production payments;
- Strike settlements credit exactly once;
- feeder threshold/remainder/ambiguity behavior is verified;
- payment and feeder messages appear on Nostr + overlay;
- informational messages are overlay-only;
- WireGuard clients use the new approved topology;
- old VPS is archival/rollback only;
- tracker #6 and its Phase 1 child issues are complete.
