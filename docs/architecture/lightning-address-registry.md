# Lightning Address Registry — Phase 1

Status: required Phase 1 architecture.

Tracker: issue #18.

## Purpose

Phase 1 must support the herd address plus one Lightning Address for each goat while keeping invoice creation explicitly allowlisted.

Required production addresses:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six addresses credit the same feeder pool in Phase 1. The service must nevertheless retain which address was paid as durable metadata for presentation and future CyberHerd logic.

## Routing model

Nginx should use generic/catchall route forwarding for LNURL paths rather than one location block per address, for example:

```text
/.well-known/lnurlp/<user> -> lightning-goatsd
/lnurlp/<user>             -> lightning-goatsd
```

This is only a routing catchall. The application must **not** implement a true wildcard Lightning Address.

`lightning-goatsd` must reject any user not present in its configured registry before contacting Strike.

## Suggested configuration

Example only:

```toml
[[lightning_address]]
user = "herd"
display_name = "Lightning Goats"
credit_pool = "herd"

[[lightning_address]]
user = "dexter"
display_name = "Dexter"
credit_pool = "herd"

[[lightning_address]]
user = "rowan"
display_name = "Rowan"
credit_pool = "herd"

[[lightning_address]]
user = "cosmo"
display_name = "Cosmo"
credit_pool = "herd"

[[lightning_address]]
user = "newton"
display_name = "Newton"
credit_pool = "herd"

[[lightning_address]]
user = "nova"
display_name = "Nova"
credit_pool = "herd"
```

Per-address configuration may later include safe presentation metadata (for example goat image/profile identifiers), but Phase 1 accounting authority comes only from the configured `user` and `credit_pool` values.

## Username rules

Use one canonical normalized username format.

Recommended constraints:

- lowercase ASCII;
- 1–64 characters;
- letters, digits, `.`, `_`, `-` only;
- no path separators, percent-decoded surprises, Unicode lookalikes, empty segments, query data, or case-fold ambiguity.

Reject invalid usernames before any provider call.

## LNURL discovery

For a configured user, return LNURL-pay metadata containing:

- callback URL tied to the same canonical user;
- configured min/max sendable amounts;
- human-readable metadata appropriate to that address;
- Phase 1 Nostr/zap policy only if explicitly implemented elsewhere (not required for the initial migration).

For an unknown user, return a safe 404/LNURL error without contacting Strike.

## Invoice callback

The callback flow must:

1. resolve the canonical user from the configured registry;
2. validate amount against that address policy;
3. build the exact metadata string used by discovery;
4. compute the required `descriptionHash`;
5. create a Strike receive request;
6. persist enough request context to reconcile the future receive;
7. return the BOLT11 invoice.

Unknown users and invalid amounts must fail before step 5.

## Durable settlement metadata

Use a backend-neutral settlement record containing at least:

```text
source
source_id / receive_request_id
payment_hash
address_user
credit_pool
amount_msat
settled_at
```

For Phase 1:

```text
source      = strike
credit_pool = herd
```

for all six configured addresses.

`address_user` remains the actual paid address (`herd`, `dexter`, etc.).

## Accounting behavior

All configured addresses feed the same durable credit balance:

```text
payment to herd   -> HERD_RECEIPT -> herd credit pool
payment to dexter -> HERD_RECEIPT -> herd credit pool
payment to rowan  -> HERD_RECEIPT -> herd credit pool
...
```

The feed threshold/remainder logic is unchanged.

Do not create per-goat spendable balances in Phase 1.

## Messaging / overlay behavior

Phase 1 may initially render all payments using the same `sats_received` template pool.

Preserve `address_user` in durable payment/event context so future presentation can say, for example, that Dexter's Lightning Address received a contribution without changing settlement/accounting storage.

Do not let an HTTP requester supply arbitrary template names, Nostr tags, credit pools, or event types.

## Abuse controls

The public route surface should be generic, but provider resource use is allowlisted.

Required controls:

- unknown users fail before Strike calls;
- invalid amounts fail before Strike calls;
- nginx rate limit on invoice-creating callbacks;
- application-level rate limit/backpressure below Strike limits;
- reasonable invoice expiration/provider behavior;
- no arbitrary user-provided metadata passed into Nostr/templates/accounting.

## Tests

Required automated tests:

- discovery for all six configured users;
- callback/invoice creation for all six users;
- exact metadata hash behavior;
- all six map to `credit_pool=herd`;
- durable `address_user` preserved;
- uppercase/invalid/path-injection usernames rejected;
- unknown username produces zero provider calls;
- min/max amount boundaries;
- duplicate/replayed settlement remains idempotent.

Staging verification must resolve and exercise all six production addresses before final DNS cutover.

## Future extension

Adding a legitimate future goat/address should require a configuration addition plus tests, not new routing/accounting code.

Phase 2 CyberHerd may consume `address_user` as context, but must not redefine Phase 1 payment finality or feeder-credit semantics.