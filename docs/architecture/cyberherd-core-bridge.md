# Private core-event bridge

The optional `core_bridge` endpoint supplies bounded, authenticated event/safety
reads to a separate CyberHerd identity. It has no public HTTP route and accepts
no payment, grant, feeder, SQL, gateway URL or generic event-import command. Its
absence/failure does not stop the core settlement, feeding or message workers.
The [intake worker](cyberherd-worker.md) now supplies ordered receiving and
membership orchestration in source. The [full port](../planning/cyberherd-implementation-plan.md)
still requires reward funding, payout and delivery capabilities.

## Authority and roles

Provision a dedicated non-admin CyberHerd UID and a separate bridge group. Do not
add that UID to the core's primary group, which may protect receive credentials.
The core and CyberHerd identities differ. Only the core is a member of the narrow
trusted-network gateway allowance. The bridge itself holds no gateway client or
spend key; it reads the core's existing safety cache.

The socket's parent must already be canonical, core-owned, assigned to the bridge
group, traversable by that group and unwritable by peers/world. The server does
not change its parent directory's permissions or group. It creates only its own
0660 socket, assigning the dedicated group. The server checks actual Unix peer
credentials before parsing a request; the client independently checks the core's
UID before sending anything. A declared UID/token in JSON grants no authority.
Unexpected regular files/symlinks are preserved; a live socket is never replaced.
Only an unchanged, stale, owned socket is reclaimed after a refused connection.

Each connection receives one length-prefixed JSON request. Request/response
schemas reject unknown fields. Frame size, clients, database reads, page count,
page bytes and time are bounded. At most one bridge read transaction competes
for the core connection pool. Admission allows two requests per second with a
four-request burst, before reading/parsing frames or verifying invoice data.
Busy readers are rejected rather than queued
without limit. Error responses contain no raw payment/configuration data.

## Financial and feeding proof

`payment_received` text/JSON alone is insufficient. The exporter follows the
native credit-grant event binding to the settlement and verifies native BTC
valuation, asset receipt, credit allocation and ledger provenance, then checks
event fields against those actual rows. Ordinary receives remain ordinary proof.
Operator grants are explicitly virtual and cannot become actual received funds.

Qualified native zap context additionally joins the immutable issued request and
settlement-bound protocol-receipt intent. Exact raw request bytes, amount/payee,
description hash and signed mainnet invoice remain checked. The exported source
time is authoritative settlement time, not the payer's request timestamp. Signer
or public receipt delivery need not be available to read a settled payment.
P2P/fiat amount mismatches remain real credit without qualified zap authority.

Migration 0015 binds new confirmed-feed events to their actual UUID in the same
transaction as confirmation and the debit. The binding is immutable and retained.
The exporter checks confirmed status, threshold, completion time, debit and event
against that binding. Command intent, uncertain outcomes and copied event JSON
cannot become food/reward authority. A failed binding write rolls back the debit
and confirmation; recovery continues to use the original feeder UUID.

Historical/unlinked events are retained and exported as non-authoritative `other`
items. They are not guessed into new reward authority from old payloads. This
additive migration does not rewrite previous events, credits, feeds or signed
messages. It does not activate reward distribution.

## Cursors and safety

Pages share the existing durable ledger/restore epoch. Epoch, contiguous event
sequences, requested position and response bounds must agree. An ahead cursor,
history gap or changed epoch requires explicit recovery rather than silent reset.
Byte limits stop at a complete event; the next page resumes at the returned
`through` cursor. A private client returns a non-deserializable
`AuthenticatedBatch` only after the peer and full batch pass validation.

Safety reads use the core's observed cache, without causing extra gateway calls.
Ready is valid only with a consistent observation no older than five seconds.
Missing, failed, stale or future observations are unknown. Shadow mode is paused.
The client checks freshness again before returning safety state. These advisory
reads cannot authorize physical delivery; the local owner/gateway remains the
physical authority. Future membership recovery must refresh targets after resume.

## Installation boundary

The receive example leaves the endpoint commented out. An installer must supply
real non-root UID/GID values, provision the isolated group/directory, grant only
the required Unix socket access, and preserve existing runtime/credential/network
permissions. Do not enable the new socket by borrowing the core's group or
running a daemon as root. Schema installation requires the normal quiesced backup.
The implementation does not install users, change a firewall or enable the
consumer service merely because this configuration type exists.

Rollback preserves the current database and source bindings. Disabling the
optional endpoint is distinct from downgrading a binary: an older SQLx build can
reject migrations absent from its embedded source. Use a compatible forward
revert/disabled configuration rather than assuming an old executable can open a
new schema or restoring older paid state.

Tests use temporary databases, synthetic payments and private Unix sockets. They
cover actual kernel peer-credential comparisons, source forgery, rollback, stale
socket handling and bounded framing. A same-user positive fixture is not evidence
of a provisioned cross-UID service; that role/containment qualification remains
part of installation verification.
