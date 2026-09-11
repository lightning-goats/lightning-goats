# Security Hardening Review: OpenHAB owner finality

## Evidence Basis

I inspected the exported rule's admission, persistence and timer failure paths
against the repository's JDBC evidence. We have a source-derived finality defect,
not a physical failure reproduction. The [inventory](context.md) pins the evidence;
the [proposal](proposals/owner-finality.md) explains its limits.

## Constraints

We keep the existing owner as physical authority, preserve permanent gateway
UUID reservations and allow no live owner changes in this work. Production stays
HOLD. We have no measured latency or memory budget beyond existing bounded
request/ledger contracts; use a balanced reliability and implementation profile.

## Opportunity Portfolio

| Opportunity | Evidence | Options | Recommendation | Proposal |
| --- | --- | --- | --- | --- |
| Separate actuation finality from delivery | Complete-to-failed persistence and pre-lock ledger reads | 1. Retain unresolved hold; 2. Irreversible versioned receipts in existing owner | Develop Option 2, retain Option 1 until verified and approved | [Owner finality](proposals/owner-finality.md) |

## Recommendation Summary

I recommend Option 2 because we can preserve the physical authority while giving
recovery a receipt whose meaning does not depend on when a reader arrives.
The difficult work is proving the owner-side serialization and persistence
contract, not broadening the gateway's success parser. We should keep automatic
recovery disabled until those proofs exist. Option 1 remains preferable if the
actual OpenHAB persistence interface cannot support the required guarantees.

## Next Decisions

Review the proposed receipt semantics and admission scope before implementation.
This proposal neither changes the owner nor selects a new physical authority.
Applying a verified candidate to the home host will need separate explicit
approval. Public static staging is independent of this decision.
