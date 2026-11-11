# Four-role CyberHerd financial candidate

`prepare-cyberherd-financial-stage.py` prepares a complete candidate for review;
it installs nothing. It adds one selector to the authenticated root tool closure,
bringing the closure to **24 files**. Use only an independently
qualified, root-owned generation and authenticate the launcher externally before
isolated Python interprets it. Never execute a writable checkout as root.

The command requires the current running receive/member installation, existing
private-contact/self-read settings where configured, zero original monetary
ceilings and persistently masked outgoing roles. It freezes the existing owner,
epoch, account/fingerprint, four identities, private sockets, payees and installed
files/PIDs. It does not read a credential, open a journal or contact a provider.

All six limits are explicit operator input: member percentage, per-feeding and
per-24-hour sats, per-payment fee ceiling, maximum net payout and new signatures
per hour. Nothing defaults to a nonzero spending value. All-zero review is
supported; otherwise the allocation/admission limits must form a complete bounded
positive policy. Zero fee ceilings remain valid. Native validation enforces the
same limits and exact cross-role references.

The fixed loopback roster must freshly report the selected stored member
allocation and original project owner. If it differs, preparation refuses before
creating the output directory. Select the stored allocation separately through
the authenticated operator interface; editing a configuration seed does not
change existing membership state. This preparer never issues a membership command.

The authenticated launcher argument shape is:

```text
--cyberherd-financial-stage NEW_DIRECTORY
  --validator ROOT_OWNED_CHECKER --validator-sha256 INDEPENDENT_SHA256
  --member-percent PERCENT --max-sats-per-feed SATS --max-sats-per-24h SATS
  --max-fee-sats SATS --max-net-sats SATS --max-new-per-hour COUNT
```

This is an argument shape, not a standalone shell command. The output must be a
new direct child of the protected upgrade base. The native checker is the
separately built `CHECK-ONLY-financial-stage` example, not a new application daemon.

Preparation copies all four originals and writes four candidates. Only the
explicit financial settings, required payout peer/account/plan-export links and
candidate process flags change. Existing receive credentials/configuration,
contact, native zap registry/aliases, core epoch/cutoff, operator allowlist,
membership messaging/lifecycle/view, signer client/bunker identity, relays and
physical controls remain untouched. The desired allocation is also recorded in
the candidate seed after matching the actual stored allocation.

Planning reserves the chosen maximum fee in each member's gross share, uses
whole sats with a one-sat minimum net amount and a 3,600-second profile freshness
bound. The selected net limit binds the planner's `maximum_net_sats`, the payout
worker's `max_net_sats` and the signer's `max_net_sats`. The planner leaves an
oversized share with operations instead of reserving it. Payout admission,
invoice preparation, quote preparation and execution independently check the
limit for both ordinary Lightning and NIP-57. Existing known-payment reads remain
available after reducing the limit; unknown executions stay held under their
original identity. An omitted limit defaults to zero for new work. Old frozen
plans retain their exact serialized hash when that field was absent.
All six native receive addresses and every configured
alias are excluded as operational destinations, together with existing extra
exclusions. The same exclusion list and receipt-relay list bind the planner,
payout worker and signing broker. Unspent amounts remain subject to the existing
confirmed-feeding accounting; no candidate can create synthetic funding.

The checker loads and validates the four native configuration types and their
cross-role bindings. It runs as the original core UID/GID with groups/capabilities
removed, NoNewPrivileges and a fresh isolated network namespace. It loads no
systemd credential and opens no runtime/store. A bounded protected carrier holds
only the copied checker and configuration files. Its exact positive JSON report
is required. The original installation and plan-group identity are checked again
before the final private preparation receipt is published.

`PREPARED.json` explicitly states `financial_candidate_only=true`,
`activation_authorized=false`, `backlog_review_required=true` and
`provider_qualification_required=true`. Candidate flags describe the proposed
future configuration; they do not change any live process. This command has no
apply, migration, restore, unmask, start, payment, signature or feeder operation.
Refusals preserve originals and retain any partial preparation for inspection.

## Qualification and remaining activation

The source includes pure scope/admission checks, protected-file refusal tests and
a separate native validator. The disposable installed rehearsal selects a
synthetic allocation through the actual signed operator API, then prepares a
nonzero candidate using the original runtime UID. All four installed original
configurations and core credit/unknown UUID remain unchanged, outgoing units
stay masked, and provider/gateway/publication operations remain zero. The
[qualified checkpoint](../testing/cyberherd-financial-candidate-checkpoint-20261111.md)
records all ten passing workflows, verified artifacts and the actual VPS's
stored-allocation refusal before candidate creation. The new tools/checker are
installed separately; no actual-host candidate has been validated or activated.

The actual-host candidate awaits the requested operator limits. Before any
separately approved activation, review retained confirmed-feeding barriers and
funding/reservations, qualify the real provider and intended recipients, take a
current verified native four-journal backup with all writers quiesced, and apply
only a qualified maintenance transaction. Enabling limits must not reinterpret
old zero allocations or erase unknown work. This preparation does not supply that
executor, prove external recovery adoption, or authorize spending/publication.
The full twelve-part port remains incomplete.

The per-payment fields require the newly qualified membership/payout programs.
An older installed payout program rejects the added field rather than enforcing
it; a current native configuration checker does not qualify an older program's
execution behavior. Any later maintenance executor must qualify the selected
programs and retained native state together before applying these candidates.

The [native backlog review](cyberherd-state-maintenance.md#review-retained-reward-backlog)
must use a current verified native snapshot, not the running
membership journal or a Python SQLite reader. `reward_worker::Worker::tick`
selects the oldest unfinished barrier. Its immutable snapshot supplies the
allocation; changing today's stored percentage cannot change an earlier
barrier's percentage. `rewards::build` also requires real funding and a fresh
core allowance for a positive allocation, while an existing `no_payout` plan
stays closed. Review unplanned positive-allocation barriers, awaiting
reservations and existing reservations explicitly. A zero current configuration
alone does not prove that every historical barrier has a zero allocation, and a
configuration-only candidate cannot establish the contents of those journals.
The separate report records frozen allocations, unplanned/closed/reserved work,
pending intake and cursor lag. It creates no plan/reservation and grants no
activation authority. Its [separate qualification](../testing/cyberherd-reward-backlog-checkpoint-20261111.md)
includes protected-root/installed CLI checks and actual historical-copy acceptance;
it does not replace the later maintenance transaction's fresh backup review.
