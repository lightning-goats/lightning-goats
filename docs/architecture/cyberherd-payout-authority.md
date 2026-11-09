# Private payout funding authority

This advances the isolated outgoing boundary in the
[full CyberHerd port](../planning/cyberherd-implementation-plan.md). It adds
authenticated reservation/source readback and sender-credential qualification.
It does not implement or activate an outgoing payment worker.

## Separate roles and default hold

The optional core bridge can pin a separate non-root `payout_peer_uid`, distinct
from its intake UID. Both use the protected socket/group, but actual kernel UID
selects the role before frame parsing. Intake may read events/safety/allowances
and reserve its bounded plan; it cannot read private payout funding pages. Payout
may read only those funding pages, not intake history, allowances or reservations.
Both allowlists are explicit, so adding a future request grants neither role
access automatically. Existing frame, admission, concurrency and DB limits remain.

`payout_execution_enabled` defaults false. Setting it requires an explicit payout
UID and reward policy. Reads of existing reservations remain possible while held,
including shadow mode or removed reward policy, so reconciliation does not depend
on authorizing new spending. The response's execution status additionally checks
current owner/percentage/per-feed/outstanding limits and mode. It has a five-second
observation lifetime and is not a durable permission or provider payment. Actual
future execution needs a fresh read and independent payout policy.

No runtime identity, group, credential or service installation is changed by these
optional source fields. The core never receives an outgoing Strike key.

## Reservation and source pages

Only the dedicated private method returns `AuthenticatedFundingPage`; a parsed
JSON DTO cannot create this capability. The client checks kernel peer, epoch,
reservation UUID, owner and requested cursor. Core readback checks current epoch,
immutable reservation/frozen policy, confirmed UUID/debit and funding manifest.
Each returned lot's native provenance is rechecked against its saved fingerprint.

Pages contain at most eight consumed parts and fit the existing private byte
budget. Cursor values must be actual consumed ledger IDs. All parts, including
unavailable/virtual ones, contribute to the confirmed threshold; only correlated
Strike parts expose a receive anchor. Those anchors require actual request/receive
UUIDs, matching stored user/pool, a valid signed mainnet invoice and immutable
requested/settled amount/hash context. Legacy credit lacking request correlation
does not acquire provider authority from its source name or an account label.
Source failure does not rewrite core credit or recreate the feeding.

The anchor is private and excludes raw provider error/context bodies. It is not
an overlay/Nostr DTO. All monetary settlement/feeding state stays in core; payout
gets no DB handle, credential, generic gateway method or physical command.

## Qualifying the sender credential

`SourceVerifier` uses the actual payout credential to re-read every real consumed
receive through the existing strict authoritative Strike receive verifier. That
checks issued request/invoice identity, completed provider state and exact credited
BTC, including P2P amounts differing from the requested invoice. The necessary
read scope is `partner.receive-request.read` for both request and receive reads.
[Request reference](https://docs.strike.me/api/find-receive-request-by-id/),
[receive reference](https://docs.strike.me/api/get-receives-for-receive-request/)

API keys access their own Strike account, so successful private resource reads
establish access to the actual credited source records. A public profile/handle
or limits response does not provide this evidence.
[Strike authentication reference](https://docs.strike.me/api/)

Root configuration supplies an account UUID label and exact credential SHA256.
That UUID is an explicit operator binding, not a fabricated API self-identity
attestation. Credential/resource qualification supplies separate evidence. The
production constructor fixes the API to `https://api.strike.me`; it exposes no
caller-selected API origin, proxy, redirect or test-mode switch. Loopback mocks
exist only inside the Rust test module. Changed key/account/environment requires
explicit recovery of the eventual payout store's binding; it is not a retry.

A qualification session begins with authenticated page zero for one exact
core/epoch/owner/plan. Independent root percentage/per-feed/part-count limits bound
the grant. Pages must remain contiguous with unchanged reservation/funding/provider
scope. Duplicate receive IDs across lots are rejected. Page progress commits only
after every provider read and sum succeeds. Missing, failed, mismatched or partial
reads cannot skip sources or manufacture completion. Final totals must equal both
the confirmed threshold and its real funded amount.

Only then is non-deserializable `QualifiedFunding` constructed, retaining core
UID, reservation, funding, threshold, credential scope and actual receive records.
Provider errors are reduced to bounded generic diagnostics. Qualification itself
neither spends, reserves another budget nor announces a treat.

## Still required

The [separate payout journal](cyberherd-payout-journal.md) now pins this scope,
retains actual source slices across grants and holds independent gross capacity.
Already consumed portions cannot be consumed again under new ledger IDs or a
restored core record. Terminal rolling-window accounting and persisted
quote/execute/reconciliation intents are still required. Qualification proves
historical receipt access, not current available balance, unspent funds, final
outgoing delivery or an unchanged sweep policy.

The [outgoing contract](cyberherd-outgoing-contract.md) still requires the isolated
credential/service, signing broker, exact outgoing invoices and NIP-57 behavior,
provider submission/reconciliation, terminal budget outcomes and original-thread
announcements. Provider/account qualification on the real installation, provisioned
cross-UID isolation and explicit financial activation remain unperformed.
