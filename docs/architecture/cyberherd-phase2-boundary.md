# CyberHerd Phase 2 Boundary

## Purpose

Phase 1 must leave a stable foundation for restoring CyberHerd functionality after the new Strike-backed Lightning Goats stack is live.

This document defines what Phase 1 should preserve without implementing CyberHerd now.

## Phase 1 owns

`lightning-goatsd` is authoritative for:

- Lightning Address/LNURL-pay receive ingress;
- Strike receive reconciliation;
- durable settled-payment records;
- goat-feed credit accounting;
- OpenHAB feeder actuation and ambiguity handling;
- durable event log;
- Phase 1 message rendering;
- Nostr publication of payment/feed messages;
- video overlay/status stream;
- overlay-only informational messages.

None of these should depend on CyberHerd membership state.

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

Phase 1 should store backend-neutral settlement information with room for optional future context.

Future CyberHerd logic must be able to associate an identity/zap/event with a settled payment without changing the core settlement semantics.

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

The exact schema may evolve, but the financial identity of a settlement must remain provider-source/payment-hash based rather than dependent on mutable Nostr profile data.

## Stable interface: durable events

Phase 1 event transport must support new event types without redesign.

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

The renderer/outbox design should allow additional message kinds and template pools.

Phase 1 ports only:

- `sats_received`;
- `feeder_trigger`;
- informational/interface/weather overlay messages.

Phase 2 may port the remaining canonical categories from `lightning-goats/cyberherd_messaging` as the business logic is implemented.

Audience policy remains server-defined per event/message kind.

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
- no public LNURL/webhook serving role.

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
- durable event types can be extended;
- template renderer can accept new message kinds;
- overlay/Nostr transports are reusable;
- a future separate service can consume/produce events without changing the public Lightning Address contract;
- spend authority remains absent from the Phase 1 daemon.
