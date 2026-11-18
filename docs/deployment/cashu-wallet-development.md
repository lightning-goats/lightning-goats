# Cashu wallet development entry point

This source entry point is not production activation. The scoped
[implementation plan](../planning/cashu-nutnovember-implementation-plan.md)
retains operator control of keys, mint selection, limits, live funds and receiving
announcements. Treasury scheduling is wired into the wallet runtime with
separate default-off execution and reconciliation controls. Its isolated
qualification does not establish installed startup, live publication acceptance,
coordinated backup recovery or full deployment readiness; those remain unfinished.

Build from the repository root, retaining the separate wallet dependency graph:

```sh
cargo build --locked --manifest-path tools/cashu-compat/Cargo.toml --bin lightning-goats-cashud
```

The executable supports these commands; announcement preparation additionally
requires an explicit operation UUID:

| Command | Effect |
| --- | --- |
| `--help` | Print the command contract; no wallet state access. |
| `check-config <path>` | Validate bounded root-owned protected JSON and policy; no keys, databases, listener or mint request. |
| `probe-mint <path> <mint-id>` | Verify public capabilities and pinned keysets over the wallet's native HTTPS transport using only GET requests. |
| `initialize <path>` | Under the registered non-admin runtime UID, load existing private keys, require absent database files/sidecars, and create registered wallet databases. Does not start intake or acquisition. |
| `status <path>` | Read a private, bounded JSON journal summary under the registered runtime UID, without loading keys or acquiring the writer lock. |
| `run <path>` | Under the registered runtime UID, require existing registered databases and keys, then run private intake, optional treasury admission/status, and configured acquisition, receipt, relay, signing and publication workers. Never initialize missing databases. |
| `prepare-request <path> <https-endpoint> <creqA\|creqB>` | Offline, print an amountless reusable request and stable descriptor ID from protected public configuration. Open no keys/journals/listeners; change no receiving flags or descriptor inventory. |
| `prepare-announcement <path> <operation-uuid>` | Offline, open existing registered state under the writer lock, retain the configured announcement and print a public-metadata preview. Start no worker. |

Configuration comes from a canonical root-owned file and protected parents, not
CLI secrets or environment variables. Provision the private state directory and
separate IPC directory before startup. See the
[wallet boundary](../architecture/cashu-wallet-boundary.md) for ownership,
registration, exact key-file formats, and default-off permission contracts.
Initialization is a distinct operator action; a failed partial initialization
requires inspection and preservation, never deletion of journals to retry.

Exit code 0 means the selected command completed, 2 means invalid command syntax,
and 1 means a configuration/startup/runtime failure. Errors are coarse and omit
private paths and provider details. SIGTERM/SIGINT stop the runtime; they do not
mark pending financial operations failed, release proofs, or reset attempt
budgets. No API for arbitrary invoices, withdrawals, wallet resets, or private
key generation is exposed by this entry point.

No systemd unit is installed by the build. Do not substitute a development
checkout for root-owned installed binaries/configuration. Service hardening,
network confinement, deployed cross-UID acceptance, historical-key recovery, current backups,
and live acceptance must be completed before real receiving is announced.

`status` reports per-state counts, configured execution/delivery toggles,
retry-exhausted counts, and at most 64 pending operation records (held records
first). Details include acquisition/mint IDs, coarse hold reasons, attempts and
persisted next-attempt timestamps. A truncation flag makes incomplete details
explicit. Output is private operator data; do not publish it in the overlay.
It does not expose proof bodies, secrets, memos, source authors or raw mint
requests/replies. It does not load private keys, query providers/the core or
certify worker liveness. Expected and observed wallet instance/epoch are explicit;
a mismatch reports `registration_matches:false` and `recovery_required:true`.
This diagnostic does not relax the daemon's existing startup registration fence.

Private `health` now reports global journal accounting plus at most 64 configured
or historical mint summaries. `mints_truncated` never truncates global totals.
`unverified_input_sats` is submitted face value, not owned money.
`journal_custody_sats` is acquired net value minus completed treasury consumption
and committed preparation fees for unfinished melts. It can still include value
whose external outcome is ambiguous; it is not a current mint balance or spend
permission. `ambiguous_submitted_input_sats` exposes that unresolved whole-input
bound separately. Pending reservations are original input allowances, so they can
include preparation costs already consumed and must not be added to custody.

Receive fees, completed conversion fees, split preparation fees and unfinished
preparation fees are separate. Total accounted fees count receive fees plus
completed conversion costs plus unfinished preparation costs; completed split
fees are already inside completed conversion costs. Reported treasury deposits
are wallet journal claims, not independent Strike verification. The rolling
24-hour execution-admission count/input are admitted budgets, not expenditure.

New acquisition timestamps commit with admission and never change on retries.
Queue health exposes oldest known pending age and overdue persisted schedule lag.
Older records have unknown age; status never backfills a guessed receipt time or
creates the optional timing table. Future timestamps are counted separately to
expose clock rollback without reporting negative ages. Schedule lag describes
retained deadlines even when work is paused or attempts are exhausted.
Historical core acknowledgment epochs are compared only with the configured
expected core epoch. Different epochs are diagnostic, not proof that an earlier
credit was invalid or that a currently running core matches the configuration.
Invalid acknowledgment JSON is counted without echoing its contents.


Optional `relay_intake` configuration enables the bounded NIP-61 reader only when
`receive_nutzap` is also enabled. Omit this object to leave relay intake off. For
source qualification, the object has this shape (the relay is a placeholder):

```json
{
  "enabled": false,
  "relays": ["wss://relay.example"],
  "historical_floor": 0,
  "window_seconds": 3600,
  "overlap_seconds": 120,
  "event_limit": 128
}
```

Select the historical floor deliberately: it defines the earliest timestamp
eligible for discovery, not a payment cutoff. The worker alternates recent and
historical windows across at most eight configured public WSS relays. Its filter
uses only qualified configured mint URLs and the project recipient. A policy,
receiving key or pinned-key inventory change creates a new discovery scope;
previous acquisitions and proof claims remain durable. The reader cannot acquire
or credit funds; those require their independently configured workers.

The native relay fixture runs the wallet daemon against a private TLS WebSocket
relay. It verifies certificate trust and hostname checks, validates the
recipient and mint subscription, and interrupts delivery before `EOSE`.
After restart, the wallet associates a new signed event carrying the same
proof with the original acquisition. The fixture takes that retained
admission through the real CDK wallet with a synthetic mint, then sends
its receipt to the native core receiver under a separate UID. It verifies
one 8-sat swap, a 1-sat receive fee, and a single 7-sat credit with the
original event and author. After dropping the first acknowledgment,
the restarted wallet recovers the original receipt without another swap.
Another signed relay replay after acknowledgment keeps the original credit.

Private status includes at most 128 retained relay lane records, with scope,
relay URL (query omitted), discovery cursor/horizon/width, outcome and retry time.
`last_admitted` and `last_rejected` count the last finished scan, not lifetime
financial totals; admitted includes exact duplicates. A `pending` outcome may be
an active request or an interrupted request awaiting its durable lease. A saturated
single-second historical window remains unresolved while recent samples continue.
These fields do not certify that a relay possesses or returned every matching event.

## Separate Cashu signing broker

`lightning-goats-cashu-signerd` is a separate executable in the same wallet crate.
Its commands are `check-config <config.json>`, `initialize <config.json>` and
`run <config.json>`. Build with `--bin lightning-goats-cashu-signerd`. No systemd
unit or credential is installed by that build. Production activation remains on
HOLD; this entry point does not authorize public receiving announcements.

The bounded JSON configuration has these fields (unknown fields are rejected):

| Field | Contract |
| --- | --- |
| `version` | Exactly `1`. |
| `runtime_uid` | Dedicated non-root signer UID, different from `policy.wallet_uid`. |
| `ipc_gid` | Non-root group shared only for wallet access to the signer socket. |
| `state_directory` | Canonical absolute private signer-owned directory for the broker journal. |
| `socket` | Canonical absolute Unix socket path in a separate preprovisioned runtime directory. |
| `policy` | Broker/wallet instance and epoch UUIDs, wallet UID, owner, receiving key, mint/relay allowlists and explicit signing limits defined by `publication_broker::Policy`. |
| `nak` | Root-owned helper path and SHA256, separate private helper state directory, the same project owner, distinct client public key, approved bunker relays and a 1–45 second subprocess deadline. |

`policy.announcements` and `policy.redemptions` default false. Attempt/hour/amount
limits default zero. Enabling a purpose requires explicit nonzero applicable
limits. The configured helper must be the qualified bounded-stdin artifact,
not stock nak; see `tools/cashu-nak-stdin/README.md`.

Offline `check-config` validates policy and protected root-owned configuration
without loading credentials, touching journals or contacting relays. Explicit
`initialize` requires the registered runtime UID/private state and absent journal
files, creates only the immutable broker registration, and does not require a
helper or credential. It never replaces existing state. `run` requires existing
registered state, the pinned protected helper and provisioned helper state.

Only `run` reads `CREDENTIALS_DIRECTORY/cashu-nostr-client`. Provision this via
systemd `LoadCredential=cashu-nostr-client:<protected-source>` when a separately
reviewed unit is ready. The file is exactly 64 hexadecimal characters with one
optional LF and contains a dedicated NIP-46 client key, **not the project nsec or
a Cashu receiving key**. The loader rejects symlinks, hardlinks, nonregular files,
excess bytes, group/other permissions and unprotected directory ancestry. The
credential directory/file must be root- or runtime-owned; parent directories
must be root-owned and not group/other writable. The derived client public key
must match `nak.client_pubkey` and differ from the project owner.

The service uses fixed self-encryption and typed 10019/7376 signing operations.
It does not publish to relays directly, expose arbitrary signing, generate keys,
initialize missing state during `run`, or hold wallet seed/proof custody. SIGTERM
and SIGINT cancel serving while retaining durable attempts/results for recovery.
Private socket peer-UID checks remain authoritative. Positive cross-UID service
acceptance, systemd hardening, bunker permissions and publication scheduling still
require qualification before deployment.

## Wallet signing retries

The optional wallet `publication_signing` object configures `socket`,
`signer_uid`, `announcements`, `redemptions` and `maximum_attempts`. Both purpose
flags default false and the attempt limit defaults zero. The signer UID must be
non-root and differ from the wallet and core UIDs; its socket must differ from
wallet intake and core receipt sockets and lie outside private wallet state.
Enabling either purpose requires an explicit attempt limit (at most 10,000).
The broker independently applies its own immutable registration and signing policy.

When `redemptions` is enabled, the wallet stages at most one eligible acquired
nutzap per worker tick, including previously acquired records without history.
This is an explicit opt-in to historical catch-up. Ordinary Cashu receipts and
unacquired inputs are excluded. Staging freezes the original event time/source
and stops at the existing 1,024-pending-publication bound without stopping
acquisition or core receipt delivery. Announcements must already be explicitly
prepared under a stable operation UUID; this worker does not create announcements
or enable public receiving automatically.

The worker charges an attempt and stores its next eligible time before broker
IPC. Backoff starts at 180 seconds (longer than the broker client's 165-second
deadline), doubles to a one-hour cap, and survives restarts/cancellation. Failed
work yields to later due records. Exhaustion retains the original operation and
intent; raising a configured attempt limit resumes it without generating a new
identity. Successful responses store the original ciphertext and signed event
atomically. Signing errors do not stop the financial workers. Shutdown retains
claims; retries use the same broker request and exact signed bytes.

Private status now reports `publication_signing_enabled`, `unsigned_publications`,
`signed_unpublished`, and `exhausted_publication_signing`. These are queue counters,
not relay-delivery or sender-visibility claims. The separate relay publication worker below handles sender read-relay routing
and per-relay acknowledgments; signing itself does not set a publication's
`published` flag.

## Relay publication worker

The optional `publication_delivery` configuration has `announcements` and
`redemptions` switches, `base_relays` (at most 16), `discovery_relays` (at most
eight), and `maximum_attempts` (at most 10,000). Switches default false, lists
empty and the budget zero. Enabling either purpose requires base destinations
and a positive attempt budget. History delivery also requires configured metadata
discovery relays. All URLs must be unique after normalization, public WSS on port
443 and free of query credentials; discovery excludes write-only broadcasters.
These are independent of receiving, signing and core credit-import controls.

Each tick can plan one due signed event and deliver one due frozen relay target.
Routing claims survive restart and back off from 30 seconds to one hour. Failed
metadata discovery yields to later work and does not prevent delivery of existing
plans. The transport is bounded to 20 seconds per operation, with a 25-second
worker deadline. Routing elapsed time is accounted for before charging delivery,
so a slow metadata query cannot shorten a relay retry delay. No journal lock is
held during network I/O.

The first successful plan freezes base destinations and verified sender read
routes. Configuration changes do not rewrite those historical plans. Disable the
relevant purpose to pause its pending deliveries; reenabling resumes retained
plans. Failed or lost ACKs retry the original signed bytes; positive acknowledgments
are retained per relay and completed targets are skipped. Only acknowledgment
from every frozen target marks the event published. This records relay acceptance,
not downstream visibility, and never creates another credit or contribution.

Private status adds `publication_delivery_enabled`, `unrouted_publications`,
`pending_relay_deliveries`, `exhausted_publication_routing` and
`exhausted_relay_deliveries`. Attempt exhaustion retains work for operator review;
an explicitly raised budget resumes the original operation. Optional publication
errors do not stop the financial workers. No live publication switch has been
enabled by implementation or tests.

## Preparing the receiving announcement

The offline command is:

```sh
lightning-goats-cashud prepare-announcement /protected/wallet.json <operation-uuid>
```

Use a non-nil operator-selected UUID and retain it for retries. This command opens
only existing registered wallet state under the runtime UID and exclusive writer
lock, so the wallet daemon must be stopped for this maintenance action. It checks
provisioned keys against the retained registration. It does not initialize state,
generate keys, start listeners/workers, contact a mint or bunker, or publish.
Production maintenance and activation still require the operator's explicit step.

The candidate configuration must specify enabled nutzap admission, qualified
mints with pinned keysets, and enabled receive-relay intake. The command derives
the public owner, dedicated P2PK receiving key, mint list and receive relays from
that configuration. It retains the typed kind-10019 intent and prints a bounded
JSON preview containing the operation ID and advertised metadata. Review that
preview before separately enabling signing and publication in a qualified stack.
Receiving flags in this offline candidate do not start receiving by themselves.

New kind-10019 events advertise the dedicated receiving key as 66 lowercase hex
characters starting with `02`. Amethyst 1.21.0's sender passes the advertised
key through to its P2PK lock, so this encoding supplies a valid Cashu key
when the sender copies it directly. The signer's configured receiving key
continues to use its existing 64-character format.
[Amethyst sender](https://github.com/vitorpamplona/amethyst/blob/f3e6a6de6fd52ab182971807304793f266c7b4df/commons/src/commonMain/kotlin/com/vitorpamplona/amethyst/commons/cashu/ops/CashuWalletOps.kt#L706-L751)
[Quartz lock serialization](https://github.com/vitorpamplona/quartz/blob/d99aae08957f1f91240da065995d40129a698b95/quartz/src/commonMain/kotlin/com/vitorpamplona/quartz/nip60Cashu/p2pk/P2PK.kt#L64-L89)

Repeating the same UUID retains the first timestamp and exact advertised policy.
A policy change with that UUID is rejected, preserving the prior intent, signature
and any delivery acknowledgments. To intentionally advertise a changed policy,
use a new UUID with a timestamp later than all retained announcements; same-second
new operations and clock rollback are rejected rather than producing ambiguous
replaceable-event ordering. Retry a lost command response using its original UUID.
An earlier x-only announcement retains its original signed bytes and can be
replaced by preparing a new UUID with a later timestamp.
An already-published operation is not reset or republished by preparation.

Announcement signing and publication switches require configured nutzap receiving
and relay intake. Both workers compare queued announcements with the current
advertised policy immediately before their external operation, including already
signed/routed announcements. Stale mints, receiving keys or receive-relay lists
are retained for review and cannot silently be signed or sent. This check does
not prevent history delivery for already-acquired nutzaps while new receiving is
paused. When pausing receiving, disable announcement work as well.

Workers use the configuration loaded at startup; apply policy changes through a
controlled restart. Preparing an announcement does not change those settings.

## Optional private treasury admission/status listener

Omit `treasury_intake` to leave this listener off. The optional object has the
following shape; its mint ID must already exist in the protected `mints` array,
and all destination/epoch values here are placeholders for operator provisioning:

```json
{
  "enabled": false,
  "accept_new": false,
  "socket": "/run/lightning-goats-cashu/treasury.sock",
  "core_epoch": "00000000-0000-0000-0000-000000000001",
  "destination": {
    "origin": "https://api.example.com/",
    "account_id": "00000000-0000-0000-0000-000000000002",
    "credential_fingerprint": "abababababababababababababababababababababababababababababababab",
    "network": "bitcoin"
  },
  "policies": {"qualified-mint": "treasury-v1"}
}
```

`enabled` controls the separate Unix listener; `accept_new` independently permits
retaining new core-issued instructions. Both default false. With the listener
on and new admission off, the authenticated core may inspect retained work and
retry the exact original instruction. These operations remain available when
new receiving or mint exit qualification is paused. Accepting new instructions
requires the endpoint to be enabled and each listed mint exit to be qualified.
The treasury policy version is distinct from the mint's receive-policy version.

The endpoint shares the configured `core_uid`, `ipc_gid`, connection rate and
whole-connection deadline. Its socket must be separate from intake, receipt and
signer sockets and outside wallet state. If receipt delivery is configured, both
sections must use the same core epoch. Protected destination/account/fingerprint
and wallet registration are checked against every retained instruction. Keep
historical bindings available during reconciliation; changing them does not
rewrite existing intent authority.

The listener runs under the same supervised runtime and retained writer lock as
intake. Shutdown preserves instructions and reservations. Private `status` now
includes `treasury_intake_enabled` and `treasury_admission_enabled`; these describe
configuration, not proof of listener liveness or completed settlement. This
listener only retains/reads instructions. It does not activate treasury spending:
job orchestration, splitting and installed acceptance remain unfinished.
Synthetic distinct-UID admission, lost-reply and paused-restart tests now pass. No production service has been enabled or restarted.

## Optional wallet treasury worker

The separate `treasury_execution` object defaults entirely off. This illustrative
configuration retains both switches off; amounts are examples, not approved
production limits:

```json
{
  "execute": false,
  "reconcile": false,
  "maximum_attempts": 8,
  "maximum_input_sats": 10000,
  "maximum_fee_sats": 100,
  "operations_per_24h": 10,
  "input_sats_per_24h": 50000,
  "fee_sats_per_24h": 250,
  "maximum_in_flight_sats": 10000,
  "split": {
    "enabled": false,
    "limits": {
      "maximum_input_sats": 20000,
      "maximum_fee_sats": 100,
      "maximum_outputs": 128
    },
    "operations_per_24h": 10,
    "input_sats_per_24h": 100000,
    "maximum_in_flight_sats": 20000
  }
}
```

Either switch requires protected `treasury_intake` bindings, but the listener
itself can remain disabled. Execution requires reconciliation enabled, positive
input/admission limits, and a qualified mint exit. Reconciliation alone permits
zero financial limits. Pausing execution prevents new reservation, quote,
preparation and submission work; original quote/preparation recovery and submitted
melt reconciliation remain eligible. Missing historical bindings or incomplete
CDK preparation evidence retain the operation for review.

The supervised worker selects actions from durable state. It records a bounded
attempt before work, with per-intent/per-stage exponential backoff (five seconds
to one hour, plus the operation deadline). A frozen melt request always selects
reconciliation, never a second submission. Exhausted attempts retain the original
operation; restarting does not reset them. Other due instructions can progress.

Wallet admission charges the full original input once per intent, conservatively,
against rolling 24-hour operation/input limits and unresolved in-flight input.
Failures and timeouts do not refund admission. Only reconciled instructions cease
counting as in-flight; expiration of the daily window does not release that hold.
Current per-intent input/fee limits are checked again before each execution step.
These limits are separate from final measured fees and independent core settlement.

The optional nested `split` policy defaults off. Its limits cover the entire
reshaped proof value, including remainder belonging to other sources. These are
separate from the original treasury-intent limits above. Enabled splitting needs
explicit positive input, output-count, daily-operation, daily-input and in-flight
bounds; unknown fields are rejected. Effective split execution requires both
`execute` and `split.enabled`. The examples above remain disabled and are not
approved production exposure limits.

When an exact whole-proof allocation is unavailable, an enabled worker can plan
and reserve a bounded split. A storage error, inconsistent custody or exhausted
selection work budget does not authorize fallback. The original quote precedes
split execution; a durable preparation claim precedes CDK operation allocation.
Restart or cancellation retains that identity. The worker independently verifies
and commits original split outputs before selecting proofs and preparing the
original quoted melt. Pausing execution leaves original split reconciliation
eligible but prevents split execution, verified-proof selection and subsequent
melt preparation/submission. Disabling only `split.enabled` prevents new splits;
use `execute: false` to pause all treasury spending stages.


Before the scheduled melt-preparation stage, the worker restores the exact
original retained quote into CDK if its local row is missing. A conflicting row
is preserved and rejected before the one-time preparation claim is consumed.
This closes the journal/CDK persistence boundary without requesting a second
quote. The [quote-adoption qualification](../testing/cashu-quote-adoption-20261116.json)
checks conflict refusal and restart recovery in both normal and lost-reply
workflows. Direct executor callers do not receive this scheduler preflight.

CDK reserves the quote, writes reserved proofs and creates its saga in separate
steps. Recovery can reconstruct a missing saga when the original quote owner
and complete original proof reservations are retained. If CDK committed only the
quote owner, the worker may complete the exact proof reservation under that same
operation UUID with treasury execution enabled. It verifies that each selected
proof is still unspent and unowned, that the operation owns no other quote or
proof, and that the original source allocation and fee budget still agree. The
reservation is atomic in the CDK store. Paused recovery can bind an already
complete reservation and has its own retry budget. A pause cannot exhaust the
retry budget for completing a reservation after execution resumes. Partial
ownership, a conflicting saga, or an unknown
operation UUID keeps the original work held. Preparation recovery leaves the
actual melt submission to the next authorized stage. Completed quote history
does not limit recovery. See the [partial-preparation qualification](../testing/cashu-partial-preparation-20261116.json)
for the original reservation, conflict and 1,025-quote history fixtures.

After financial reconciliation, a separate retryable cleanup stage retires only
that original CDK operation. It verifies the retained paid report, original proof
and quote ownership, and a fresh complete spent-state response before mutating
local CDK state. It removes the original spent inputs, records the original quote
as paid, and removes only the original saga. A journal checkpoint binds the
operation UUID to the financial-report digest. Partial cleanup retries keep the
financial report intact; unrelated operations are preserved. This stage remains
eligible with spending paused when reconciliation is enabled.

The [cleanup qualification](../testing/cashu-cdk-cleanup-20261116.json) covers lost
replies, failed spent-state reads, interruption before the cleanup checkpoint,
and subsequent same-mint acquisition. The wallet suite passes 154 tests with
five explicitly ignored fixture children, plus strict Clippy, formatting and
vendor verification. This is synthetic source qualification, not installed
acceptance or independent core settlement qualification.

Private status exposes `treasury_execution_enabled`,
`treasury_reconciliation_enabled`, and `treasury_split_execution_enabled` (the
combined top-level/nested execution setting). They describe configuration, not
progress or settlement. The worker does not create core invoices, import settlement into
the core, or authorize feeding. Core financial planning/job generation, complete
independent settlement qualification, partial-CDK-preparation recovery and
installed acceptance remain unfinished.

## Core financial allocation

The draft schema-26 addition now includes `feed_financial_plans` and
`feed_financial_parts`. When Cashu history is present, feed admission freezes a
financial source allocation before creating a physical attempt. A plan waiting
for settlement creates no physical attempt and debits no credit. Only one
financial plan may be pending; repeated admission after restart retains it.

Selection prefers eligible settled credit, ordered by original ledger ID within
that set. If it cannot cover the threshold, a bounded FIFO allocation of remaining
credit becomes a pending plan. A pending plan that becomes fully covered proceeds
with its original parts. If later settled sources can fund a different complete
feeding, the pending plan receives a retained `settled_supersession` disposition
and a new settled allocation is selected atomically. Original allocations and
all treasury transfer history remain retained. Threshold changes similarly record
an explicit release. This is an eligibility-aware departure from global FIFO.

Dispatch links the financial plan to the existing physical request and copies its
exact parts into the physical source reservation in the same transaction.
Confirmation continues to consume those frozen parts through the existing debit
and reward-evidence path. Offline retirement rejects credit reserved by a pending
financial plan; unresolved physical attempts keep their existing protection.

The default-off `controls.plan` setting now lets the treasury worker generate
bounded per-mint jobs when no retained job is due. Planning subtracts verified
source coverage and pending transfer reservations, and reuses the existing
policy, daily-limit and custody checks in the same transaction as the plan/job
link. A mint with exhausted exposure capacity does not block another mint.
Released plans retain their jobs; replacement plans are explicitly linked.

The local `release_financial_feed_plan` method requires the exact plan UUID and
threshold, is idempotent, and refuses dispatched plans or unresolved physical
attempts. It releases only the credit reservation, not treasury custody or transfer
history. Wiring this operation to protected operator tooling remains unfinished.

The [financial planning qualification](../testing/cashu-financial-jobs-20261116.json)
passes 629 core tests (9 explicitly ignored), strict Clippy, formatting,
SQLite vendor verification and the cached advisory audit. It retains the initial
retirement-test failure and the subsequent explicit-release correction. Fallback
sweeps, full end-to-end settlement qualification, operator tooling and installed
acceptance remain unfinished. No services or live journals were changed.

## Treasury sweeps under qualification

Protected `controls.sweep` settings default off. When enabled, the worker gives
pending feed planning priority, then evaluates available custody for age or
per-mint balance triggers. Age is measured from the core's original receipt
acceptance timestamp; it does not claim to measure unimported wallet custody.
Zero disables either automatic trigger. Selection includes capped/retired excess
holdings, but excludes already reserved or consumed custody. It creates an internal
treasury intent and a retained reason, with no donation grant, credit change,
celebration or physical attempt.

Feed and sweep jobs share the same source reservations, fee ceilings, daily limits
and unresolved-exposure bounds. Automatic mint selection prefers the least recent
durable treasury admission, so a continually funded mint does not monopolize
available operation slots. Sources within each mint remain in ledger order.

An operator can provide a bounded `requests` list in the protected sweep settings.
Each item contains a non-nil stable `id`, approved `mint_id` and positive
`maximum_input_sats`. The original ID and wallet/core scope are retained; changing
its contents is refused. At most one original treasury job is assigned to that
request across retries/restarts, even if more custody later arrives. Current
policy can further reduce its maximum. Automatic triggers, if configured, remain
independently active. Removed/paused mint settings do not authorize new work;
paused sweeping or treasury spending still permits existing inspection to start.

Sweep planning does not itself contact the wallet or provider. Issuance, wallet
delivery and wallet-side spending keep their separate controls. These source
changes are under verification; no live sweep or configuration change has run.

## Optional core treasury coordinator

The core's protected TOML configuration may include `cashu_treasury`. Omitting it
leaves the worker off. It requires the existing `cashu_receipts` wallet registration;
that registration may have receipt import paused. The destination below is a
placeholder and all execution controls remain off:

```toml
[cashu_treasury]
socket = "/run/lightning-goats-cashu/treasury.sock"
deadline_seconds = 30

[cashu_treasury.controls]
plan = false
issue = false
deliver = false
inspect = false
maximum_attempts = 8

[cashu_treasury.controls.sweep]
enabled = false
maximum_holding_age_seconds = 0
balance_trigger_sats = 0
requests = []

[cashu_treasury.policy]
enabled = false
version = "treasury-v1"
invoice_expiry_seconds = 300

[cashu_treasury.policy.destination]
origin = "https://api.example.com/"
account_id = "00000000-0000-0000-0000-000000000002"
credential_fingerprint = "abababababababababababababababababababababababababababababababab"
network = "bitcoin"
```

The destination, policy version and registered epochs must agree with the wallet's
protected treasury bindings. `policy.enabled` is the independent spending gate;
`issue` and `deliver` both require `inspect`. Enabling spending also requires
explicit positive global and per-mint limits. `policy.limits` accepts
`minimum_deposit_sats`, `maximum_deposit_sats`, `maximum_input_sats`,
`maximum_fee_sats`, `operations_per_24h`, `input_sats_per_24h`,
`unlimited_input_sats_per_24h`, `fee_sats_per_24h`, and `maximum_in_flight_sats`.
The daily limits apply across mints. Set `fee_sats_per_24h` explicitly on the
core and wallet before new treasury work. Zero permits only fee-free intents.
An omitted fee ceiling prevents fresh authorizations and preserves original
reconciliation. Set `unlimited_input_sats_per_24h` with a zero or omitted
`input_sats_per_24h` to remove the daily volume ceiling. The operation count,
per-transfer limits and outstanding exposure still apply.

An unresolved intent reserves its entire authorized fee across time windows.
The core replaces the reservation with the recorded fee after independently
verifying the original Strike receive and authenticating the wallet result.
The wallet counts the recorded fee for 24 hours from local reconciliation.
Treasury RPC version 2 carries that time beside the original report. The core
retains its first authenticated observation and counts the fee for 24 hours
from the latest of that observation, the wallet time and Strike verification.
Version 1 replies keep their original shape. Report bytes and digests remain
stable when read through either version.

Core schema 27 adds only the fee observation table and its retention triggers.
The migration gives prior authenticated reports one rolling window from the
migration time and preserves their amounts and report bytes. Legacy wallet
results without a retained time reserve their maximum for one window from
the first writer upgrade. For an existing schema-26 installation, qualify the
coordinated backup and upgrade with the wallet still paused. Initialize its
legacy fee timestamps before migrating the core so the core's backfill is at
least as recent. Fresh conversions require a source-matched pair.

Budget pressure leaves fresh work waiting without creating a smaller invoice or
charging wallet attempts. Fees include the authorized preparation split and
melt costs. Donation acquisition fees have their own per-acquisition limit.

The daemon supervises this worker alongside receipt import and the existing
Strike recovery worker. Startup construction does not contact the wallet or mint.
Its socket must differ from receipt and public-intake sockets; configured intake
must use the same wallet identity/epoch. Workers require distinct non-root core
and wallet identities. Restart reloads protected policy without resetting durable
attempt counters. Disabling the spending policy prevents new issuance claims and
makes wallet delivery fall back to status inspection. A missing wallet record
stays unresolved in inspection-only mode.

This coordinator processes retained intents and can generate jobs from pending
financial feed plans when `controls.plan` is enabled. Wallet proof splitting has
separate default-off execution controls. Sweep source qualification and full deployed
acceptance remain unfinished. No production configuration has been changed.

## Inactive wallet service artifact

[`lightning-goats-cashu.service.example`](../../deploy/systemd/lightning-goats-cashu.service.example)
is a review artifact, not an installation command. It has no `[Install]` section,
initialization hook, key generation or automatic database repair. Copying the
file into this repository neither starts a process nor changes host identities.
`run` continues to require existing registered journals and keys; systemd's
creation of an empty state directory cannot initialize a wallet.

The unit expects a dedicated non-admin `lightning-goats-cashu` user, the
`lg-cashu-ipc` primary group and the separate
`lg-cashu-signer-ipc` supplementary group. Register their actual
numeric IDs in protected configuration; do not reuse production daemon or payout
identities. Only the intended core IPC client belongs to the wallet IPC group;
only the wallet and dedicated Cashu broker belong to the signer IPC group.
Peer-UID authentication still controls each socket request. The core's receipt
socket must separately grant this wallet traversal/connect access through its
approved IPC group before receipt delivery can work.

The canonical configuration path is `/etc/lightning-goats-cashu/config.json`,
root-owned with protected parents. Install the pinned executable root-owned at
`/usr/local/bin/lightning-goats-cashud`. Configure private state at
`/var/lib/lightning-goats-cashu` (0700) and wallet listeners under
`/run/lightning-goats-cashu` (0750, wallet owner and wallet IPC group).
Keep the broker socket in its separate runtime directory. No Strike credential,
OpenHAB token or Nostr client credential is loaded by this unit. Existing core,
payout and signer configuration/state directories are inaccessible in its mount
namespace. Cashu seed/receiving keys remain inside private wallet state according
to the registered key-file contract.

The shipped unit denies **all IP egress** while permitting protected Unix IPC.
It cannot acquire from a mint, reconcile mint state or contact relays in this
state. This is an explicit inactive staging posture, not a qualified live network
policy. Before live startup, prepare and test a reviewed override and host egress
rules for selected public mint/relay endpoints and the exact resolver. Preserve
IPv4/IPv6 denial of household/WireGuard/private/link-local/metadata routes; test
DNS rebinding and redirects through the application's existing mint adapter.
Do not remove the deny rule merely to make a startup test pass. Confirm systemd's
IP filtering is supported/enforced on the target host; absence of a diagnostic
is not network-containment evidence.

Installed acceptance still requires real unit startup under the selected UID,
positive/negative Unix peer access, denied state/secret reads, egress containment,
SIGTERM/restart recovery, resource sizing and coordinated backups. No service,
account, firewall rule, production key or configuration is installed by this
artifact. Keep receiving, acquisition, receipt import, signing/publication and
treasury spending independently off until their respective acceptance and
operator activation steps are complete.

The matching inactive
[`lightning-goats-cashu-signer.service.example`](../../deploy/systemd/lightning-goats-cashu-signer.service.example)
uses the distinct `lightning-goats-cashu-signer` user and `lg-cashu-signer-ipc`
group. Configure its root-owned JSON at
`/etc/lightning-goats-cashu-signer/config.json`, broker state at
`/var/lib/lightning-goats-cashu-signer`, helper state at
`/var/lib/lightning-goats-cashu-nak`, and its Unix listener under
`/run/lightning-goats-cashu-signer`. Both state directories are private (0700)
and separate. The helper executable must be the root-owned, hash-pinned
bounded-stdin nak artifact already required by the broker configuration.

Only this unit loads `cashu-nostr-client`, through systemd's encrypted credential
store at `/etc/credstore.encrypted/lg-cashu-nostr-client`. The source is an
operator-provisioned dedicated NIP-46 client key, never the project signing key.
Do not create that credential or authorize bunker purposes as part of installing
an inactive example. Systemd decrypts the selected credential into its protected
runtime credential directory before the service's mount restrictions apply;
the source credential stores remain hidden from the running process. The
loader accepts systemd's root-owned read-only credential mount when its
access ACL grants only the registered signer UID the required read and
directory search permissions. Qualify this behavior on the actual target.
The signer unit hides wallet custody and core/payout state and denies IP egress
by default. It cannot reach a bunker until a separately reviewed egress override
is accepted. No `[Install]` section or dependency automatically starts it alongside
the wallet. A separate real-bunker acceptance step must verify narrowly approved
kinds and encryption permissions without granting generic signing authority.


## Offline reusable request preparation

The source CLI now exposes the existing CDK descriptor encoder through
`prepare-request <config.json> <https-endpoint> <creqA|creqB>`. The [source qualification](../testing/cashu-request-cli-20261116.json) passes
the wallet gates; installed and client/scan acceptance remain separate. Supply the exact short first-party HTTPS
submission URL intended for the approved deployment, and choose the encoding
from the qualified client/scan matrix. Credentials, query strings, fragments and
non-HTTPS endpoints are refused. The command never makes an HTTP request.

Preparation can run while global receiving/acquisition/spending remain disabled
and before wallet keys or state are initialized. The protected configuration must
still declare valid public receiving identity and the selected qualified mint
policies. The output is public receiving instructions, not a spendable token; it
contains `version`, stable `id`, exact encoded `request`, `encoding`, preparation
status and whether the ID is already in the configured descriptor inventory.
The same policy and endpoint produce the same ID for both encodings; changing
mint policy, endpoint or receiving key changes the descriptor identity.

The command does not register its generated ID, change protected configuration,
serve a public route, publish a receiving announcement or enable a payment rail.
Before activation, register the reviewed ID in the accepted descriptor inventory,
retain the approved old-descriptor window, bind the actual HTTPS route and
qualify the receiving pipeline. Preserve the exact case of `creqA` payloads.
Operator setup/public serving and real client/phone scans remain separate from
this offline preparation command.

## Selecting the public reusable descriptor

The optional protected wallet `public_request` object selects receiving
instructions by endpoint and encoding, rather than accepting an opaque QR string:

```json
{
  "public_request": {
    "endpoint": "https://lightning-goats.example/api/v1/cashu",
    "encoding": "creqB"
  }
}
```

This example is synthetic and does not enable receiving. First use
`prepare-request` with the intended public configuration; review the result and
place its ID in the wallet's `descriptors` inventory before selecting it here.
Configuration validation regenerates the request through CDK using the current
mint identities, advertised URLs, policy versions and receiving key. The generated
ID must already be admitted by that inventory, and the encoded request must fit
the public display bound. A changed endpoint, mint policy or key therefore needs
a newly reviewed ID. Both supported encodings identify the same descriptor.
Unknown fields, including a caller-supplied encoded `request`, are rejected.

When ordinary receiving is disabled, prepared runtime state has no public
descriptor. The core fetches the current selection for HTTP and overlay reads.
See the [overlay checkpoint](../testing/cashu-overlay-20261116.md).

### Retiring a published request

When changing the public request, remove its former ID from `descriptors` and
retain it under `retired_descriptors` with its original endpoint and original
published mint list. Copy the mint ID, exact advertised URL and policy version
from the reviewed prior configuration. Keep the original HTTPS path routed
to the wallet during the compatibility window if the new request moves paths.

```json
{
  "retired_descriptors": [
    {
      "id": "original-prepared-request-id",
      "endpoint": "https://lightning-goats.example/api/v1/cashu",
      "retired_at": 1794950000,
      "accept_before": 1795036400,
      "mints": [
        {
          "id": "example-mint",
          "advertised_url": "https://mint.example/Bitcoin",
          "policy_version": "receive-v1"
        }
      ]
    }
  ]
}
```

Replace the illustrative ID, times and mint data with the reviewed values.
The wallet regenerates the old ID from this historical policy and the current
receiving key. A retired ID cannot also be in the active inventory. The
compatibility window begins at `retired_at` and ends just before
`accept_before`, in Unix seconds. Within it, the old ID can admit payments
only from mints both advertised by that request and qualified and enabled by
the current configuration. Receipts record the policy used at admission.

An exact previously admitted HTTP body can retrieve its original status
capability after retirement while the bounded intake route is open. An
expired ID cannot admit a new body. The status route continues to accept
existing capabilities. Retain admitted state and the applicable keys across
rotation. When changing the receiving key, include the old compressed public
key as `receiving_key` on each retired descriptor that used it.

### Rotating a receiving key

Keep the existing `wallet-seed.bin` and `receiving-key.bin` in place. The
original receiving key and seed bind the existing wallet registration, epoch
and mint counters. Set `receiving_key_anchor` to the original compressed public
key and `receiving_key` to the new compressed public key. Supply the new raw
32-byte secret as `receiving-key-<compressed-public-key>.bin` in the private
wallet state directory with the same runtime ownership and mode `0600`.
The wallet verifies that each file matches the configured public key.

List additional retired keys in `retained_receiving_keys` using their compressed
`public_key`. Use the same filename convention for each retained non-anchor key.
The anchor is available for previously admitted work. To continue accepting
delayed nutzaps against it or another prior key, add that key to the list with a
`nutzap` object containing `starts_at`, `accept_before` and the original
`mint_ids` still approved for the window. Use Unix seconds and set an exclusive
end. Admission checks the wallet's observation time and current mint policy.
Omit the `nutzap` object to retain a key for already admitted work and any
explicitly bound retired QR.

The acquisition worker selects the signing key from each retained proof's lock.
Keep the corresponding key files while such work is queued. Wallet startup
requires the keys named by queued proofs. Previously submitted work continues
to use its serialized request. If NIP-61 publication is configured, update the
Cashu signer policy's `receiving_key` to the new x-only key while keeping its
registration and journal. The current-policy check keeps outdated announcements
out of active publication. Rehearse both configurations before an installed
change.

The encrypted checkpoint collects the original and additional configured key
files and verifies them against the recorded configuration. Retain the
checkpoint and its receipt through the separately approved rotation.

## Inactive Cashu edge include

`deploy/nginx/lightning-goats-cashu.conf.example` adds only the exact token POST
and public request GET paths, plus OPTIONS. It reuses the existing HTTP-context
rate-limit zones and preserves the enclosing API-prefix deny. It is not included
in any production site automatically. Its [four isolated nginx tests](../testing/cashu-nginx-20261116.json)
pass with a synthetic loopback backend.

The token route limits bodies to 1 MiB, matching the upper wallet envelope, with
a same-size memory buffer, no proxy request buffering, and no upstream retries.
Application limits may be lower. Method and query-string rejection happens at the
edge. The public metadata route has a 1 KiB body bound and does not forward a
request body. Both routes disable access logging and suppress route error capture;
application errors remain coarse. CORS and no-store response headers come from
the application to avoid duplicate headers. A failed upstream must not be served
from a cache.

Before installed acceptance, verify the selected CDN/WAF suppresses request and
response bodies, URL arguments, audit captures, analytics and debug capture for
these routes while retaining request bounds and attack blocking. The include
cannot configure an external provider's logging policy. Run real browser-wallet
OPTIONS/POST acceptance through that reviewed edge before public receiving.
The hosted `cashu-sdk-https-edge.json` fixture connects the pinned Cashu-TS
sender to the actual wallet and core through the packaged nginx routes. It
checks TLS trust and hostname validation, CORS preflight, stable admission
after a lost reply, credited status after settlement and local log suppression.
The mint, Strike and feeder responses are simulated. External WAF behavior and
live client interoperability require separate acceptance.

### Inactive sender-status edge addition

The optional include now also contains an exact POST/OPTIONS
`/api/v1/cashu/status` route with a 512-byte in-memory body bound, no upstream
retries/cache, query-string refusal and local log suppression. Its [expanded
nginx fixture](../testing/cashu-status-nginx-20261116.json) passes all four tests.
The corresponding wallet/core application source is now integrated and under
qualification; installed endpoint acceptance remains pending. Keep capability values in
JSON bodies only and qualify external WAF/CDN suppression separately before
activation. No production status endpoint is installed or qualified by this source change.

## Sender-status and financial-release source under qualification

The ordinary HTTP adapter now has a distinct private `ordinary_admit` operation.
A successful validated admission returns HTTP 202 with its durable state and an
opaque capability in the JSON body. The state can be pending, acquired, credited,
or held; it does not promise new credit merely because an HTTP request completed.
Pre-admission validation rejection is HTTP 400, while storage/unavailable or
unknown outcomes remain coarse 503 errors. Duplicate proof submissions reuse the
common acquisition and original capability.

Exact POST `/api/v1/cashu/status` accepts only a small JSON capability body and
returns coarse durable progress, or 404 for an unknown capability. It is usable
while new receiving is paused, provided the registered private wallet path remains
available. No capability belongs in URLs or logs. The private listener still
checks kernel UID, wallet instance/epoch and nonce; neither query invokes a mint
or mutates credit. These source paths are integrated but their full application
and distinct-UID status acceptance is still pending. Real sender behavior remains
unqualified.

`lightning-goatsctl release-financial-plan --approval <protected.json>
--writers-stopped [--apply]` is now implemented in source and under qualification.
It previews by default and branches before ordinary database initialization. It
requires protected root-owned app configuration and approval, the existing
non-admin database owner, current schema, exact ledger epoch/plan/threshold,
explicit review and a current approval window no longer than 24 hours. It uses a
fixed absolute systemctl binary with inherited bus-routing environment removed,
and requires the actual core unit to be inactive/dead with PID zero and no
control group. The command never stops the service itself.

Applying releases only the named pending financial plan. Original treasury
instructions, custody holds, receipts and ambiguous physical attempts remain
retained; unresolved physical attempts refuse the release. It does not refund,
resend, create a new payment, alter credit, reconcile a physical outcome or start
a feeder. A retained transfer may still complete after release. Source/unit
verification is not authorization to run this on production; production maintenance
remains an explicit operator step under the HOLD.


### Explicit restored-wallet fence

Wallet writer startup now refuses any `CASHU_RECOVERY_REQUIRED` entry in its
private state directory, including a dangling symlink. It does not create the
writer lock or open wallet databases when that entry is present. This matches
the fail-closed intent of the core maintenance boundary; it is not automatic
rollback detection or a completed coordinated backup/restore implementation.
Restore tooling must place this fence while writers are stopped and preserve
original operation IDs, proof material, keys and cross-store acknowledgements.
There is no automatic marker-clearance command. Mint/Strike reconciliation and
both ledger epochs must be resolved before a separately authorized resumption.
Regular-marker and dangling-marker regression cases are added; native wallet
qualification remains pending in the resumed build sequence.

The [coordinated recovery inventory](cashu-recovery-inventory.md) records the required databases, keys, historical policies and cross-store reconciliation boundaries. It explicitly tracks the unimplemented backup/restore and recovery-approval work.

### Mint-scoped CDK correction under qualification

The runtime now selects one private CDK store per configured mint, while sharing
its application journal, seed and writer authority. Acquisition and treasury
execution/recovery use that same selection. Startup checks the legacy
`wallet.sqlite` for proofs/sagas and refuses automatic layout adoption when the
old journal contains financial work. It does not silently move or ignore old
custody. New namespace bindings are retained before database initialization and
marked ready afterward; interrupted initialization fences subsequent startup.
Rebinding a retained mint ID to another URL refuses.

The equal-secret two-issuer fixture passes with the scoped adapter, where the
shared-store version left original recovery pending. Full runtime wallet gates
are still running; coordinated migration/restore and private status namespace
reporting remain unfinished. The updated recovery inventory specifies every
retained store and its binding, including disabled/removed acceptance policies.
No production schema, service or wallet has been changed by this source work.

## Wallet custody limits

Set `custody_limits.maximum_total_sats` and `custody_limits.maximum_per_mint_sats`
in the protected wallet JSON before receiving. The second field is a map from
configured mint IDs to positive sat limits at or below the total. Every mint
enabled for either receiving rail needs a limit. An older configuration without
this object can read and reconcile existing records and answer exact retries.
Fresh admissions and previously unsubmitted swaps need explicit limits.

The wallet reserves gross value for pending admissions and counts acquired net
value, less journal-confirmed consumption and preparation fees. Returned change
and unresolved swaps or melts continue to count. Both receiving rails use the
same transaction and reservations survive restart. A full wallet returns a
retryable response to ordinary Cashu and retains the relevant relay scan range
while continuing the batch. A payment larger than a configured ceiling is
rejected. Older queued admissions can acquire when their value fits, without
charging attempts for work waiting on capacity. Existing swap recovery keeps
its original request and permission gates.

The core separately limits imported custody and independently verifies Strike
settlement before any Cashu-funded feeding. A mint policy review may add a
limit for a newly configured mint while retaining the total and existing limits.

## Operator selections recorded 2026-11-16

The selected mint is exactly `https://mint.minibits.cash/Bitcoin` (including
path/case). The operator already uses Minibits on Android for ordinary Cashu.
The operator reported Minibits build `eefdfb9e0` and Amethyst v1.21.0 for
Android testing. Amethyst advertises NIP-61 in its
[official supported-feature list](https://github.com/vitorpamplona/amethyst/blob/v1.21.0/README.md).
End-to-end compatibility on the phone is still pending.

Operator direction requires Cashu settlement into the existing Strike Lightning
wallet as part of the feeding flow, independently verified before dispatch.
Retain exact transfer IDs across retry; the treasury receive never grants a new
donation or credit. This agrees with the implementation plan's financial gate.
No extra mint, age-based sweep policy, exposure/fee limits or live activation has
been approved by this selection.

[Public Minibits discovery](../testing/cashu-minibits-public-discovery-20261116.json)
records public capability and keyset discovery. Run
`lightning-goats-cashud probe-mint /protected/wallet.json minibits` to check the
mint from a deployment candidate. The command reports the advertised sat BOLT11
melt limits and checks the advertised proof-state, recovery, P2PK and DLEQ
capabilities. It verifies the ID, fee metadata and pinned public keys for each
configured keyset. It fails if
an active sat keyset is unpinned or the inventory changes during the check. It
uses only public HTTPS endpoints and opens no keys or wallet state. Funded swap,
melt and Android wallet tests are separate acceptance steps. See the checklist at
`/home/linuxuser/lightning-goats/CASHU-GO-LIVE-INPUTS.txt`.

The operator provided the public npub of the separate Nostr identity for NutZap
testing in Amethyst. Acceptance records need only the public npub and installed
app version.
This does not authorize sharing/reusing the Lightning Goats signer identity.
The phone/signer retains the private key; a remote-signer bunker connection is
needed only if that test identity is held in a remote signer.
