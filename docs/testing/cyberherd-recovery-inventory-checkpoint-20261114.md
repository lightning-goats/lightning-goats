# Known recovery identity inventory — qualified 2026-11-14

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

[PR #125](https://github.com/lightning-goats/lightning-goats/pull/125) is merged.
Source `8cc5ad6969ec38788b547c408b596426506e8e46` passed formatting, strict
locked Clippy, 437 library tests (six ignored), integrations and mandatory actual-root
fixtures in [Rust run 43638006202](https://github.com/lightning-goats/lightning-goats/actions/runs/43638006202).
Audit, signer, home/network, website, package and native systemd checks all passed;
[deployment run 43638006185](https://github.com/lightning-goats/lightning-goats/actions/runs/43638006185)
includes native job `132581137903`. Tested merge
`3fc5149773f163e63748b785ac78dee8df8af746` and actual main merge
`90faa517437f697458bfe24fc15166a4429c6173` have identical tree
`c58a05251fbac21f34f856172b2477f99faef0aa`. All 17 selected queries also prepared
against the actual migrated core/payout schemas; 25 local documentation links
passed validation.

Review artifact `14131570493` passed its archive and all internal checksums.
Retained tar SHA256: `d084d653a241bb99481f17fa5597b43bf888591f4265997c7b9353bfd741daff`.
Operator binary SHA256: `4cb7d64f074d97e222f7c7aa2c08bb40f388dc025808a317a91467789ded15ea`.
The native binary refused unprivileged operation before opening policy/state.
The first sandboxed attempt instead failed its local Unix-socket UID check;
that failure was not treated as permission acceptance evidence.

Actual host qualification ran the separate root-private candidate against the
existing accepted four-journal backup, without replacing the installed operator
or restarting services. It verified the backup before/after, output hash and
false external/adoption/resume flags, unchanged protected installed files and
unchanged four-service status. Protected result:
`/var/lib/lightning-goats-upgrades/recovery-inventory-8cc5ad6-review/RESULT.json`,
SHA256 `5701ec0aaa5eaf95db6b8701fdcbc642a4330037560fc3a712d73a5d9ff02fb4`.
No live journal, configuration, credential or provider was changed.

The first host attempt stopped at tool-bundle authentication: importing the
launcher generated one Python bytecode cache, which the exact-content check
correctly refused. Only that generated cache was removed; bytecode generation
was disabled before the successful retry. Reviewed tool bytes were unchanged.
This qualification does not install recovery state or authorize adoption.

Independent Strike/gateway histories and their coverage/retention still need to
be supplied/qualified. An operator information request is pending; no missing
export is inferred from silence. The inventory is not proof of external
completeness and does not satisfy the unfinished adoption scope on its own.
