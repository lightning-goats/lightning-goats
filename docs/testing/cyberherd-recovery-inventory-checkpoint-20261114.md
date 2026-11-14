# Known recovery identity inventory — implementation candidate

The full recovery path still lacks a concrete externally reconciled adoption
procedure. Its next prerequisite is a complete private list of original known
identities from an accepted backup; a bounded list of report examples cannot
serve as that input. `inventory-recovery` adds this read-only preparation to the
existing native operator tool, without adding a marker-clearance, service,
payment, provider-request or general SQL operation.

The inventory verifies all four native images and exact policy/manifest binding,
streams selected core/payout identities in stable order, compares every selected
row count to the accepted report, and hashes/readbacks the complete private
JSONL output. It omits raw invoice/callback/preimage/credential/message content.
Incomplete/existing/nested outputs fail without becoming accepted inventories.
Original images, signed state, reservations and replay IDs remain untouched.

Coverage extends the mandatory actual-root copied-WAL fixture to inventory a
paid settlement and pending feeder UUID while current-writer readiness is false,
check 0700/0600 permissions, unchanged source manifest, false external/adoption/
resume flags and refusal of output reuse/nesting. Additional fixtures exercise
129 identities (beyond the 32-example review limit), stable byte hashes, count
mismatch, malformed/oversized/wrong-kind records and JSONL framing. A synthetic
mapping fixture keeps original idempotency keys, provider quote/payment IDs and
latest per-payment state distinct, and verifies that private invoice context is
not exported. Discovery-only fee-probe identities are included without turning
their observations/promotions into execution authority. The CLI's
protected-policy refusal matrix includes the new command.

Local formatting, diff and documentation link checks are required before push.
Exact-source Rust/security/native CI and installed offline qualification remain
pending. No production program, configuration, journal, service, credential or
external provider has been changed for this source candidate.

Independent Strike/gateway histories and their coverage/retention still need to
be supplied/qualified. An operator information request is pending; no missing
export is inferred from silence. The inventory is not proof of external
completeness and does not satisfy the unfinished adoption scope on its own.
