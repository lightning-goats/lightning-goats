# Six-program maintenance before financial activation

`maintain-cyberherd-programs.py` prepares and applies a program-only upgrade of
`lightning-goatsd`, `lightning-goatsctl`, `cyberherd-statectl`, `cyberherdd`,
`cyberherd-payoutd` and `cyberherd-signerd`. This is a separate transaction from
the completed contact/core/overlay upgrades. It keeps their original configuration,
credentials, runtime identities, paid history and zero financial limits. It does
not install a financial candidate, unmask outgoing roles or authorize spending.

This closes the program-installation portion of the later financial deployment:
the older installed payout program cannot enforce a newly added configuration
field merely because a separate checker accepts it. Financial configuration,
recipient/provider qualification and explicit activation remain separate work.
The full twelve-part port remains incomplete.

## Authenticated preparation

Use only a separately qualified root-owned tool generation. The explicit tool
closure now has **25 files**. Authenticate its launcher externally before isolated
Python interprets it; do not run a writable checkout as root. Independently select
the exact source and SHA256 of both the normal release and CyberHerd review archive.
Archive verification precedes program execution. No gateway, archive example,
website, systemd unit or configuration from either archive is installed.

The launcher selector and argument shapes are:

```text
--cyberherd-program-maintenance prepare
  RELEASE_ARCHIVE REVIEW_ARCHIVE SOURCE_SHA RELEASE_SHA256 REVIEW_SHA256 NEW_DIRECTORY

--cyberherd-program-maintenance apply NEW_DIRECTORY
  --review-sha256 PREPARED_SHA256 --exclusive-maintenance

--cyberherd-program-maintenance resume NEW_DIRECTORY
  --review-sha256 PREPARED_SHA256 --approve-resumption
```

These are argument shapes, not copyable production commands. Preparation freezes
the current protected installation, service/process identities and all six old and
selected program files. The existing core/member must be running and both outgoing
roles persistently masked, with all financial/signature limits zero. Existing
contact, signed self reads, operator allowlist, payees/aliases, epoch, template and
presentation settings remain exact. The three selected worker programs each load
their retained native configuration under their own original UID/GID, with no
supplementary groups/capabilities, NoNewPrivileges and an isolated network namespace.
Their exact positive `--check` output is required; no worker/runtime or credential
is opened. Preparation creates only private review data and a reversible six-file
transaction. An independent review pins the complete `PREPARED.json` digest before
any service operation.

## Exclusive maintenance and resumption

`apply` requires a separately approved maintenance window. It checks the frozen
baseline again before stopping core/member and masking all four writers. Empty
cgroups and absence of runtime-UID processes are required. It then:

1. Uses the retained original native verifier for a fresh four-journal snapshot.
2. Requires both original and selected verifiers to accept the same exact schemas,
   migration checksums, policy scope, image hashes and backup identity.
3. Runs original and selected core status on a separate runtime-owned offline copy,
   requiring identical status. It never runs status/migration on a live image.
4. Runs the selected native reward reviewer on this exact fresh backup. The private
   report must bind the backup ID/hash, owner and epoch and examine every barrier.
   Its activation flag remains false and external reconciliation remains required.
5. Rechecks all live images, accepted backup and protected files, replaces only the
   six executable bytes with original metadata/labels, and verifies them again.

Live database files, configuration and credentials remain byte-identical through
application. An unknown feeder UUID or held provider work is retained. The report
does not close old plans, create funding, authorize retries or clear recovery markers.
Any failure retains the stopped/masked state and evidence; `apply` never reruns a
started transaction automatically.

`resume` independently revalidates the accepted backup with both verifiers, live
image fingerprints, private review hash and selected native configuration checks.
A durable resumption fence precedes all unmask/start operations. Only the existing
core/member units are restored and started once, with effective identity/unit,
running executable and local health checks. Both outgoing roles remain masked and
all existing zero limits remain exact. This does not establish that the newly
installed outgoing programs have performed a payment or obtained a production zap
signature. Once resumption begins, program rollback is refused because databases
may already have new writes.

## Interrupted application before resumption

If a fresh backup was accepted and resumption has never begun, the separate
`rollback` command can restore the six original program files while all four
writers remain masked:

```text
--cyberherd-program-maintenance rollback NEW_DIRECTORY
  --review-sha256 PREPARED_SHA256 --approve-program-rollback

--cyberherd-program-maintenance resume-original NEW_DIRECTORY
  --review-sha256 PREPARED_SHA256 --approve-resumption
```

This checks actual original/selected file fingerprints, exact unchanged live
images and both native backup verifications before replacement. Unknown program
bytes, financial-image drift or any resumption fence refuse rollback. It never
restores a database. Original-program resumption requires its own explicit command;
selected-program resumption is refused after rollback was selected. Failures before
the backup was accepted need inspection rather than an invented backup/recovery
authorization.

## Verification scope

Protected-file tests cover different original/selected program bytes, all six
replacements, original metadata/configuration retention, partial-application
rollback, no financial-image restoration, parser/schema/report refusal and
post-resumption refusal. Native/systemd behavior is covered separately by the
disposable installed rehearsal using synthetic journals, real worker parsers,
native snapshot/review and actual core/member resumption. That phase uses the same
program build on both sides; it does not claim different-build live acceptance.
Provider operations, public networking and gateway POSTs remain zero in the fixture.
No actual-host maintenance has been performed for this new transaction.

The [financial candidate](cyberherd-financial-stage.md) and
[native backlog review](cyberherd-state-maintenance.md#review-retained-reward-backlog)
retain their own contracts. A later financial application/resumption executor and
operator-selected limits are still required. A successful zero-budget program
upgrade does not itself qualify recipients, complete external recovery adoption
or enable money movement.
