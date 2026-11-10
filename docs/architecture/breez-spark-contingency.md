# Breez SDK + Spark contingency for Phase 1

Status: **design spike only; not an adopted Phase 1 provider change**

Reviewed: 2026-09-14

Pinned references:

- Lightning Goats baseline: `df6be90f1b0505d183643ec55a298b382cd43b6c`
- Breez Spark SDK repository `main`: `783ad07306c29dee448923763140c00819cbc010`
- Spark integration overview: <https://docs.spark.money/integrations/breez>
- Breez SDK Spark docs: <https://sdk-doc-spark.breez.technology/>

## Why evaluate this now

The current Phase 1 contract in issue #6 and `AGENTS.md` names Strike as the only
payment backend. Strike sandbox access is currently an external schedule risk.
Breez SDK + Spark is therefore worth evaluating as a **contingency and test
backend**, without silently changing the production contract.

The useful news is that the expensive accounting work is already provider-neutral.
`domain::payment::SettledPayment` carries `source`, `source_id`, optional
`payment_hash`, Lightning Address user, credit pool, amount, settlement time and
provider context. That boundary can accept a Spark-backed authoritative settlement
without changing feeder accounting or the durable `payment_received` contract.

The main coupling that remains is above that boundary:

- `AppConfig` contains a required `StrikeConfig`;
- `LnurlService` owns a concrete `StrikeRuntime`;
- LNURL callback issuance calls a Strike-specific create-and-persist method;
- Strike receive-request persistence/recovery and webhook parsing are provider-specific;
- deployment credentials and verification documents assume a receive-only Strike API
  credential.

That makes Spark technically feasible, but **not a drop-in replacement**.

## Findings from the current Breez/Spark implementation

### Development can proceed without Strike sandbox

Breez documents a maintained Spark regtest network and recommends it for testing.
Its regtest getting-started path explicitly says the default regtest configuration
can be initialized without a Breez API key. This is materially different from
mainnet, where the general Breez SDK documentation says a Breez API key must be
configured.

Reference: <https://sdk-doc-spark.breez.technology/guide/testing.html>

This means Spark regtest is a credible way to exercise an alternate real payment
provider boundary now, even if it is not selected for production.

### Rust integration is first-class

The upstream workspace contains the Rust `breez-sdk-spark` crate and currently
uses Rust 1.88, matching this repository's locked CI toolchain. The crate supports
native storage backends including SQLite, PostgreSQL and MySQL, plus external
signers and Turnkey integration.

Reference: <https://github.com/breez/spark-sdk/tree/783ad07306c29dee448923763140c00819cbc010>

### BOLT11 receiving is available

The SDK can create Lightning BOLT11 invoices with an amount and expiry. It can
also create an invoice for another Spark wallet using only that wallet's identity
public key.

Reference: <https://sdk-doc-spark.breez.technology/guide/receive_payment.html>

The currently documented request takes an invoice **description**, amount, expiry,
optional payment hash and optional receiver identity key. Lightning Goats native
LNURL currently computes SHA256 over the exact LNURL metadata and requires the
provider-created invoice to bind that `description_hash`.

**Therefore exact LNURL metadata-hash compatibility is an adoption blocker until
proven by a generated Spark invoice.** We must not replace the current issuer and
assume wallets will accept the result.

### Server mode exists

Breez explicitly provides `default_server_config` for multi-user/server workloads.
It disables per-instance background services and expects the host to orchestrate
sync/claims/event delivery explicitly, normally with webhooks. This is preferable
to embedding an ordinary mobile-style wallet lifecycle in `lightning-goatsd`.

Reference: <https://sdk-doc-spark.breez.technology/guide/config.html>

### Authenticated receive webhooks exist

Spark supports a `LightningReceiveFinished` webhook. The service-provider payload
is accompanied by an `X-Spark-Signature` HMAC-SHA256 signature computed over the
raw request body using the registration secret.

Reference: <https://sdk-doc-spark.breez.technology/guide/webhooks.html>

For Lightning Goats, a valid webhook should remain a **notification**, not the
accounting authority. After authenticating it, the provider adapter should reconcile
against SDK/provider payment state and the persisted issuance row before constructing
`SettledPayment`, just as the current Strike architecture refuses to credit directly
from a notification.

### The SDK is a wallet, not a receive-only API client

This is the most important difference from Strike. The Breez SDK can prepare and
send payments. A normal mnemonic-backed server instance therefore possesses
spend authority over received funds.

The current Phase 1 security model deliberately requires that the public daemon
not hold spend-capable payment credentials. Directly placing a Spark mnemonic or
an unrestricted Spark signer in `lightning-goatsd` would weaken that model and is
**not an acceptable transparent substitution**.

Breez does support external signers and Turnkey-backed signing. Turnkey policies
can keep wallet keys out of the server and can require a separate user credential
for the Spark transfer-approval operation.

References:

- <https://sdk-doc-spark.breez.technology/guide/external_signer.html>
- <https://sdk-doc-spark.breez.technology/guide/turnkey.html>

A production Spark design must independently prove that the credential available
to the Lightning Goats runtime cannot authorize outgoing transfer approval. A
statement that keys are remote is not enough: remote spend authority is still
spend authority.

### Do not enable Spark-over-Lightning fallback

Breez documents `prefer_spark_over_lightning` as not recommended. On receiving,
a payer may use the Spark fallback address embedded in the invoice, and that
receive cannot then be linked back to the invoice.

Reference: <https://sdk-doc-spark.breez.technology/guide/config.html#prefer-spark-over-lightning>

Lightning Goats depends on exact invoice/address-user/payment correlation. This
option should remain **disabled** unless that accounting contract is intentionally
redesigned and independently reviewed.

### Do not outsource the existing Lightning Addresses by default

Breez can host or self-host LNURL/Lightning-address functionality. The current
Phase 1 design, however, deliberately owns the six `lightning-goats.com` addresses
and performs unknown-user/amount rejection before provider contact.

Reference: <https://sdk-doc-spark.breez.technology/guide/receive_lnurl_pay.html>

A Spark contingency should keep the existing native Lightning Address/LNURL edge
and replace only the invoice/settlement provider underneath it. Migrating the
public Lightning Addresses to Breez would be a separate DNS/application contract
change and is unnecessary for the contingency.

## Recommended architecture if Spark advances beyond the spike

Do **not** put a mnemonic-backed `breez-sdk-spark` instance directly inside the
public daemon as the first implementation.

Prefer a small local payment-provider sidecar:

```text
public nginx
    |
    v
lightning-goatsd
  native LNURL + Lightning Addresses
  durable neutral ledger
    |
    | narrow localhost / Unix-socket provider API
    v
lightning-goats-spark-provider
  Breez SDK Spark server mode
  provider issuance/reconciliation state
  external signer or constrained Turnkey policy
    |
    +-- Spark/Breez services
```

Properties required of the sidecar boundary:

1. Separate non-admin system identity from `lightning-goatsd`.
2. Root-owned configuration and credential material.
3. No TCP listener reachable outside the local host; prefer a root-created Unix
   socket with explicit group/ACL ownership if practical.
4. Expose only receive-path operations required by Phase 1: create invoice,
   inspect/reconcile issued receive, health/readiness, and webhook ingestion if
   webhook termination is delegated to it.
5. **No generic prepare/send/withdraw/transfer endpoint.**
6. The credential/signer policy used by the service must be tested to reject an
   outgoing Spark transfer, not merely documented as intended to do so.
7. Stable-balance/token/cross-chain functionality remains disabled.
8. `prefer_spark_over_lightning` remains false.
9. Recovery data/signing configuration must have explicit backup/restore evidence
   before any production funds are accepted.

A direct in-process adapter remains useful for regtest experiments, but should not
be assumed to be the final production security boundary.

## Minimal provider interface for Lightning Goats

The repository should eventually stop passing `StrikeRuntime` directly into
`LnurlService`. A provider-neutral receive interface can stay deliberately narrow:

```text
create_receive(
    address_user,
    credit_pool,
    amount_msat,
    lnurl_metadata_hash,
    expiry_seconds,
) -> IssuedReceive {
    provider,
    provider_request_id,
    bolt11,
    payment_hash,
    amount_msat,
}

reconcile_receive(provider_request_id) -> Pending | Settled(SettledPayment)
```

The provider implementation—not LNURL and not the ledger—owns provider SDK/API
semantics. The existing ledger remains the sole authority for idempotent credit,
feed credit and `payment_received` event creation.

This interface intentionally excludes send/payment APIs.

## Proposed Spark settlement mapping

Only after authoritative reconciliation:

```text
SettledPayment {
    source:        "breez-spark",
    source_id:     <stable authoritative Spark payment/receive id>,
    payment_hash:  <verified Lightning payment hash>,
    address_user:  <from durable issuance row>,
    credit_pool:   <from durable issuance row>,
    amount_msat:   <verified received sats * 1000>,
    settled_at:    <authoritative provider completion time when available>,
    context_json:  <bounded non-secret provider evidence>,
}
```

`address_user` and `credit_pool` must come from the durable issuance record that
was created before returning the BOLT11 invoice. They must not be inferred from
an unauthenticated callback parameter or presentation metadata after payment.

The provider must reject a reconciled payment if its amount, payment hash, invoice
identity, network or receiver identity contradicts the persisted issuance row.

## Persistence/migration requirements

Do not delete or reinterpret current `strike_receive_requests` state in place.
A provider-neutral implementation needs one of these migration strategies:

- add a new generic issued-receive table and import/retain the existing Strike
  rows with provider=`strike`; or
- retain the Strike table and add a separate Spark issuance table behind a common
  repository interface until Strike state ages out.

The first option is cleaner long term but is a real schema migration and therefore
requires its own backwards/restart tests.

Do not issue invoices from both providers for the same callback request through
implicit fallback. Provider choice must be deterministic and durable before the
invoice is returned, otherwise restart/retry behavior can create uncorrelated
receives.

## Required test gates before runtime adoption

### Gate A — regtest feasibility

Use Breez/Spark regtest only, with no real-value funds or production keys.

- initialize a fresh isolated Spark test wallet;
- create a fixed-amount BOLT11 invoice;
- pay and reconcile it through authoritative SDK state;
- prove duplicate notification/reconciliation credits once;
- lose a notification, restart, and recover the same receive by persisted identity;
- prove conflicting amount/payment-hash/provider IDs fail closed;
- prove no presentation/Nostr failure affects financial commit.

### Gate B — native LNURL compatibility

For all six configured Lightning Address users:

- current discovery metadata remains byte-for-byte generated by Lightning Goats;
- callback rejects unknown users/invalid amounts before provider invocation;
- provider-generated BOLT11 is for the exact requested amount;
- BOLT11 expiry is within the configured contract;
- BOLT11 description hash equals SHA256 of the exact LNURL metadata;
- returned payment hash is stable and matches the settled receive;
- a callback retry does not create an ambiguous second authoritative issuance.

Failure of the description-hash requirement means Spark cannot replace the current
native LNURL issuer without a separately reviewed LNURL contract change.

### Gate C — notification/recovery semantics

- authenticate the raw Spark webhook body with the configured HMAC secret;
- malformed, wrong-network, wrong-type and bad-signature requests fail before
  provider/accounting mutation;
- notification alone never credits;
- authoritative lookup/reconciliation must match the stored issuance;
- duplicate and reordered webhook delivery remains idempotent;
- provider outage after a valid notification remains retryable without reissuing
  an invoice;
- restart between reconciliation and ledger commit remains exactly once.

### Gate D — receive-only security

The most important production gate:

- `lightning-goatsd` has no mnemonic, Spark signing key or unrestricted remote
  signing credential;
- provider runtime is a distinct identity and exposes no send endpoint;
- an automated negative test attempts the SDK's outgoing transfer path with the
  installed provider credential/policy and must be denied before value movement;
- local socket permissions prevent nginx and unrelated runtime users from invoking
  provider operations;
- secret values never enter logs, SQLite context JSON, artifacts or GitHub;
- backup/restore preserves the exact wallet/provider identity needed to recover
  incoming funds.

If a policy-constrained external signer cannot make outgoing transfer cryptographically
or administratively unavailable to the runtime credential, the Spark design does
not meet the present Phase 1 least-privilege contract.

### Gate E — existing Phase 1 invariants

Run the unchanged end-to-end acceptance around the alternate provider:

- one authoritative 2,340-sat settlement credits exactly 2,340 sats;
- exactly two 1,000-sat confirmed feed debits leave 340 sats;
- feeder UUID ambiguity/restart behavior is unchanged;
- payment/feeder Nostr + overlay events retain exact durable semantics;
- informational/weather remains overlay-only;
- no provider choice can bypass gateway/OpenHAB safety or WireGuard containment.

## Staged implementation sequence

1. **Now:** use this document to decide whether Spark is only a test contingency or
   a candidate production provider. No production contract changes yet.
2. Implement a disposable **regtest-only spike** behind a test/provider seam, without
   changing default runtime configuration or migrations.
3. Prove Gate B description-hash compatibility before doing schema/runtime work.
4. If production Spark remains desirable, explicitly amend issue #6/AGENTS and the
   accepted payment-provider task boundary. Obtain ownership handoff for the current
   VPS payment files.
5. Introduce provider-neutral receive issuance/reconciliation interfaces with the
   existing Strike implementation first, keeping behavior byte/semantically stable.
6. Add the Spark sidecar/adapter and generic issuance persistence behind an explicit
   provider selection. No automatic runtime fallback.
7. Run Gates C-D plus the full locked CI/security/deployment matrix.
8. Only after independent review decide whether Strike remains production, Spark
   becomes production, or both are supported as operator-selected providers.
9. Production/cutover remains separately operator-approved and retains rollback.

## Recommendation

**Use Breez/Spark regtest now to remove Strike sandbox access from the critical path
for provider-abstraction testing. Do not yet replace Strike as the production
backend.**

Spark is attractive because the Rust SDK is mature enough for server use, regtest
can be used without a Breez API key, authenticated receive webhooks exist, and our
ledger is already backend-neutral. The main risks are not accounting—they are
exact LNURL metadata-hash compatibility and the fact that Spark is a wallet with
potential spend authority rather than a naturally receive-only API credential.

If the regtest spike passes the LNURL compatibility gate and we can enforce a truly
receive-only signer/service boundary, Spark becomes a credible production option.
Until then, treating it as a test backend gives immediate value without weakening
Phase 1's security contract or throwing away the completed Strike work.

## Reference set

- Spark/Breez integration: <https://docs.spark.money/integrations/breez>
- Breez SDK Spark overview/API-key note: <https://sdk-doc-spark.breez.technology/>
- Testing/regtest: <https://sdk-doc-spark.breez.technology/guide/testing.html>
- Receive BOLT11: <https://sdk-doc-spark.breez.technology/guide/receive_payment.html>
- Server/custom configuration: <https://sdk-doc-spark.breez.technology/guide/config.html>
- Webhooks: <https://sdk-doc-spark.breez.technology/guide/webhooks.html>
- Lightning Address/LNURL: <https://sdk-doc-spark.breez.technology/guide/receive_lnurl_pay.html>
- External signer: <https://sdk-doc-spark.breez.technology/guide/external_signer.html>
- Turnkey: <https://sdk-doc-spark.breez.technology/guide/turnkey.html>
- Upstream SDK source pinned for this review: <https://github.com/breez/spark-sdk/tree/783ad07306c29dee448923763140c00819cbc010>
