# Gateway replay witness — first implementation slice

**Source preparation only; no deployed activation or owner compaction.** Scope
ACK: issue17 comment5655823559; explicit library entrypoint identified in
comment5655865945. This builds on source-accepted PR76. The shipped gateway CLI,
configuration schema, default constructor and existing owner remain unchanged.

`OwnerWitness::open(url, generation)` opens only an existing nonempty regular
SQLite file, with create-if-missing disabled and WAL/FULL durability. It validates
version/generation and canonical unique UUID entries. It never creates the
schema, adopts an observed generation, migrates or repairs missing coverage.
The explicit library constructor `TrustedGateway::from_config_with_owner_witness`
validates coverage before returning a witness-enabled gateway. A future reviewed
configuration/initializer and local generation-enable mechanism are still needed
before deployment; neither is supplied by this patch.

## Version-one storage contract

The initializer's eventual schema must provide these two relations:

- `owner_witness_metadata(version INTEGER, generation TEXT)`: exactly one row,
  version1 and the expected canonical installation-generation UUID.
- `owner_witness(request_id TEXT PRIMARY KEY, gateway_required INTEGER NOT NULL
  CHECK(gateway_required IN(0,1)))`: immutable identity coverage. Value1 means a
  matching gateway request/refusal must exist; value0 is an imported owner-only
  tombstone that blocks dispatch without pretending a gateway result exists.

This is a contract for a separately reviewed quiesced initializer, not a manual
production SQL recipe. Tests create synthetic schema fixtures only. Migration
must preserve the union of gateway requests/refusals and authoritative owner
identities, setting gateway-required for every gateway identity and retaining
owner-only tombstones. It must not fabricate confirmations, clear history or
reinterpret v1 owner receipts as v2 completion.

## Admission and recovery

Before GET recovery or POST admission, required witness identities must equal
the gateway request/refusal union. Missing or additional required coverage fails
closed. An owner-only witness hit with no gateway row returns an error before a
safety refusal or unknown-UUID response can suggest no earlier actuation.

The existing atomic capacity/refusal reservation stays intact. Only its original
`New` result can proceed to a unique committed witness insertion and then one
owner POST. A hit or failed commit leaves the gateway pending and sends nothing.
New refusal tombstones also receive witness coverage before returning refusal.
If that commit fails, the refusal remains in the gateway and ordinary requests
fail closed until explicit reconciliation; no automatic coverage repair occurs.
Metadata generation is rechecked inside the consuming witness write transaction.

A crash between gateway reservation and witness commit conservatively blocks
admission, including after reopening. A crash after witness commit but before
POST leaves pending state that is never automatically dispatched. Completion
still requires the configured owner adapter; the witness contains no success
field and can never confirm a request. Lost notification recovery remains GET-only.

Coverage reads across two databases are deliberately not represented as one
atomic transaction. A concurrent in-flight reservation can cause a conservative
coverage error; it cannot grant dispatch. This first slice scans identity sets
on checks and is not a proven throughput solution for indefinite history.
A later optimization must preserve the same refusal/restore properties.

## Verified cases and remaining acceptance

Four witness-store tests cover160 identities/reopen/oldest replay, concurrent
unique reservation, missing/empty/foreign/version-invalid state and owner-only
tombstones. Four real loopback gateway/owner tests cover witness-hit/store-miss
before remote-OFF refusal, failed witness insert after gateway reservation, held
restart/GET recovery/refusal replay with exactly one command, and both directions
of partial restore blocking GET/POST/startup. All stores and endpoints are
synthetic/disposable; no deployed owner, credential or database is involved.

This does not make the current owner retain more than32 receipts. The owner cap,
receipt representation and legacy ingress remain unchanged. An independent
owner-side transactional replay mechanism is required before compaction. Full-host
co-restore is not detectable by two local databases alone; current-process local
enablement plus independent reconciliation evidence remain unimplemented gates.
The160-entry witness test is not physical-owner sustained-retention acceptance.

Rollback of this unactivated source requires no host change. Once any future
witness mode is used, preserve both stores and all pending state; do not downgrade
or remove the witness to bypass a coverage failure. Shared CLI/config/migration,
owner backend and physical enable need separate claims, review and approval.
