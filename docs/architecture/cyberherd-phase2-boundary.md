# CyberHerd Phase 2 Boundary

## Purpose

Phase 1 must leave a stable foundation for restoring CyberHerd functionality after the new Strike-backed Lightning Goats stack is live.

This document defines what Phase 1 preserves without implementing CyberHerd now.

## Phase 1 owns

`lightning-goatsd` is authoritative for:

- Lightning Address/LNURL-pay receive ingress;
- Strike receive reconciliation;
- durable settled-payment records;
- goat-feed credit accounting and durable feed-attempt state;
- deciding when a feed is due and preserving ambiguous/no-blind-retry semantics;
- durable event log;
- Phase 1 message rendering;
- Nostr publication of payment/feed messages;
- video overlay/status stream;
- overlay-only informational messages.

The trusted `lightning-goats-gateway` / existing correlated OpenHAB feeder owner are authoritative for the physical-control boundary. `lightning-goatsd` does not receive an OpenHAB token and CyberHerd must never introduce a bypass around that gateway/owner contract.

None of these authorities should depend on CyberHerd membership state.

## Phase 2 CyberHerd responsibilities

Expected CyberHerd domain includes:

- Nostr identity/event ingestion;
- member identity/profile context;
- daily roster/membership state;
- contribution accumulation by member;
- capacity/spots;
- headbutt rules;
- member contribution increases;
- daily resets;
- reward/distribution calculations;
- richer CyberHerd-specific Nostr/overlay events;
- possible outbound Lightning rewards.

These are out of Phase 1 scope.

## Stable interface: payments

Phase 1 stores backend-neutral settlement information with room for optional future context.

Future CyberHerd logic must be able to associate an identity/zap/event with a settled payment without changing core settlement semantics.

Do not require identity metadata for a payment to be financially valid in Phase 1.

Possible optional future fields/context include:

```text
nostr_pubkey
nostr_event_id
zap_request
payer_comment
display_name
source_context_json
```

The exact schema may evolve, but financial settlement identity remains provider-source/payment-hash based rather than dependent on mutable Nostr profile data.

## Stable interface: durable events

Phase 1 event transport supports new event types without redesign.

Phase 1 examples:

```text
payment_received
feeder_confirmed
interface_info
weather_status
```

Phase 2 may add:

```text
cyberherd_member_joined
cyberherd_member_increased
cyberherd_headbutt_success
cyberherd_headbutt_failure
cyberherd_daily_reset
cyberherd_reward_planned
cyberherd_reward_paid
```

Names are illustrative; choose a consistent final schema during Phase 2.

## Stable interface: messaging

The renderer/outbox design allows additional message kinds and template pools.

Phase 1 ports only:

- `sats_received`;
- `feeder_trigger`;
- informational/interface/weather overlay messages.

Phase 2 may port the remaining canonical categories from `lightning-goats/cyberherd_messaging` as the business logic is implemented.

Audience policy remains server-defined per event/message kind.

## Stable interface: physical feeding

CyberHerd must never write OpenHAB Items, invoke the feeder owner directly, or hold the OpenHAB gateway credential.

If future CyberHerd activity contributes feed credit or causes a feed to become due, it must enter through the same durable Lightning Goats accounting/feeder authority so that:

- the same feed-attempt UUID is the cross-boundary idempotency key;
- the trusted gateway persists and deduplicates the UUID;
- gateway-local rate caps remain active;
- `FeederOverride`, remote-enable, and the existing correlated OpenHAB owner remain local physical safety authorities;
- ambiguous outcomes continue to block automatic retry.

This physical-control contract is not optional Phase 2 plumbing; it is a security boundary.

## Separate service vs integrated module

Do not force this decision in Phase 1.

### Option A — separate `cyberherdd`

Potential shape:

```text
lightning-goatsd
    |
    +-- durable events / shared API
    |
cyberherdd
    +-- Nostr ingress
    +-- membership state
    +-- headbutts
    +-- reward planning
```

Advantages:

- smaller feeder/payment trust domain;
- independent failure/restart behavior;
- easier privilege separation;
- future spend-capable reward authority can be isolated.

Costs:

- IPC/API/event-consumption mechanism;
- another process/service to operate.

### Option B — CyberHerd module in Lightning Goats codebase/process

Advantages:

- simpler deployment/state transactions;
- fewer moving parts.

Costs:

- larger attack surface in the feeder/payment process;
- more difficult privilege separation if payouts are added.

## Current preference

Architect Phase 1 so Option A remains easy. A separate CyberHerd/payout trust domain is likely preferable if outbound reward spending is restored.

This is a design preference, not a Phase 1 implementation requirement.

## Outbound payments

Do not give Phase 1 `lightning-goatsd` a spend-capable Strike key in anticipation of Phase 2.

If CyberHerd rewards require Strike outbound payments, prefer a separately privileged payout component, for example:

```text
cyberherd-payoutd
```

with:

- narrowly scoped outbound credential;
- durable payout intents;
- idempotency/reconciliation;
- explicit policy input from CyberHerd state;
- no public LNURL/webhook serving role;
- no OpenHAB/gateway physical-control credential.

## Legacy repositories as behavioral references

During Phase 2 inspect and preserve desired behavior from:

- `lightning-goats/cyberherd_extension`;
- `lightning-goats/cyberherd_messaging`;
- `lightning-goats/lightning_goats_extension`.

Do not mechanically port LNbits framework dependencies. Port the domain behavior and presentation contracts into the new architecture.

## Phase 1 acceptance requirement

Phase 1 is CyberHerd-ready when:

- payment settlement does not require CyberHerd state;
- feeder accounting does not require CyberHerd state;
- the physical feeder boundary remains gateway-mediated and CyberHerd-independent;
- durable event types can be extended;
- template renderer can accept new message kinds;
- overlay/Nostr transports are reusable;
- a future separate service can consume/produce events without changing the public Lightning Address contract;
- spend authority remains absent from the Phase 1 daemon.
