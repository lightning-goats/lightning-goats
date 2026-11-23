# Verified payout outcomes and reservation accounting

This is source for requirements 7–8 of the complete CyberHerd port. It is not an
installed payout worker or authorization to activate spending. The receive and
physical feeding ledgers, existing messages and live configuration are unchanged.

The protected payout identity owns the outgoing journal and attests its original
invoice/quote/payment correlation. Core independently reads each original payment
ID using its own read-only Strike credential. The operator-pinned account UUID is
a configuration label; it is not a provider account self-attestation. Successful
authenticated reads establish accessibility with that credential. The provider
does not always return an invoice hash/preimage for internal payments, so this
does not claim cryptographic correlation where the API supplies none. A returned
preimage must match the frozen invoice. The payout UID and its protected journal
remain trusted for original correlation and no-execute attestations.

## Closing and freezing the payout journal

An unsubmitted row can be permanently closed only before **any** execute intent
exists, even if the process has not called HTTP yet. Unavailable recipients,
expired invoices and operator cancellation have explicit retained reasons. SQL
guards and API checks prohibit new invoice/quote/execute identities for closed
rows. A provider timeout or 404 never proves non-submission.

The payout store freezes one sorted manifest only when every payable row is
either durably closed or has a bound, observed terminal payment without a recorded
conflict. It includes exact original correlation, state, fee and total cost.
The header commits the complete core reservation, provider scope and row count;
an ordered SHA256 chain commits every row. Gross row amounts must sum exactly to
the reservation. Unknown/pending rows prevent finalization. Rows cannot be replaced
to turn a possible payment into a cancellation.

## Core verification and retry

Only the separate kernel-authenticated payout UID may submit outcomes or read
their progress. Intake cannot call either method. One bounded row is sent per
request. Core checks epoch, full reservation, immutable header and ordered cursor
before any provider call. It then reads the original payment ID, validates the
frozen amount/method/fee/preimage constraints, and requires exact agreement with
the journal's terminal state and costs. A separate single-request permit bounds
provider verification without holding the ordinary event/safety IPC permit.
HTTP happens outside database transactions. Payout requests have a 22-second
server/25-second client deadline; ordinary IPC deadlines are unchanged.

Core rechecks scope and cursor in an immediate transaction before retaining the
verified row. Provider payment IDs and invoice hashes are globally unique across
reports. Exact retries return the same progress without another provider read.
Partial reports keep the **whole** reservation held. The final row must verify
the committed hash, count and total gross amount before a final acknowledgement
can be committed. A failed final write rolls back the final row and release.
Committed progress remains readable when execution, reward policy or the optional
provider reader is disabled. A changed/restored core epoch requires explicit
recovery; it cannot silently approve the old outcome.

## Held, spent and released amounts

Completed payments count their verified total cost, including fees, as spent;
unused fee allocation is released. A durably unsubmitted row releases its entire
gross reservation. A Failed provider state does not prove fee/refund settlement:
it claims no member delivery and retains the entire row gross by default. Only a
separate [protected operator cost review](cyberherd-failed-cost-recovery.md) can
reclassify a finalized failed hold. Unknown costs never silently age into
available budget.

Core usage is all non-final reservations, plus final unresolved holds, plus
completed spending verified within the last 24 hours. Creation time never ages an
outstanding reservation out. A retained monotone clock rejects backward time for
new financial changes. Reservations remain immutable and cannot be reserved a
second time after release.

The independent payout store continues holding the whole grant until it receives
a final acknowledgement through the authenticated core client. It verifies the
core UID, epoch, reservation and exact manifest hash and durably saves that ACK.
Its own rolling limit conservatively dates completed spending from local ACK
acceptance, which may be later than core verification. Lost replies, a restart or
a disabled worker cannot release budget merely from a local Completed status.

## Configuration and remaining work

`core_bridge.payout_account_id` enables the optional read-only verifier and must
match the payout account label. It requires a separate configured payout UID.
The existing receive credential additionally needs `partner.payment.read` to
verify outgoing results. It does **not** gain quote, execute, withdrawal or spend
authority. Default absence means no provider outcome submissions are accepted;
status reads still work. No production credential is modified by this source.

Tests use temporary databases, actual Unix framing and distinct synthetic core
read and sender keys against loopback HTTP. They cover provider disagreement,
unused fee release, failed-cost holds, lost ACK/restart, role rejection,
never-executed closure, uncertain-intent retention, partial reports and failed
final commits. They do not establish live Strike behavior or paid deployment.

Subsequent source adds the [separate broker/worker](cyberherd-payout-runtime.md),
[member delivery](cyberherd-member-delivery.md), lifecycle and operator interfaces,
plus [explicit failed-cost review](cyberherd-failed-cost-recovery.md). These remain
uninstalled. Consistent restore/retention, patched-engine installed
qualification and explicit financial activation still remain for the full port.
