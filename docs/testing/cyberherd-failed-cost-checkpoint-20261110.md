# Failed-payment cost recovery checkpoint — 2026-11-10

Feature code `22d0d2ffb959d8ee9e4dc67992f103b51a9204c2` was tested as PR merge
`d1649dcdc0acb6558b82908040833b646778a9fc` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. The local feature tree and independent
GitHub Git-data merge read both equal `5eee0f66655c03d763e30262060918dc7533ef35`.
The review archive records the actual tested merge checkout.

[CI 42785366992](https://github.com/lightning-goats/lightning-goats/actions/runs/42785366992),
job `129725113480`, passed format, strict locked all-target/all-feature Clippy
and the full locked all-feature suite: **485 passed, zero failed, one ignored**.
The separately repeated root-ownership test and two inactive example tests are
excluded from 485. All nine added native regressions appear in completed logs:

- An explicitly reviewed original failed hold can be reclassified into its
  actual cost. Original outcome/final acknowledgment bytes stay unchanged.
  Reviewing one 100-sat hold at a cost of three leaves an unrelated 100-sat
  reservation held: budget usage is 103, then 100 after the cost window ends.
  No feed credit or public/member message is created.
- Unfinalized, wrong-epoch/account/payment/hash, expired and Pending-provider
  cases cannot release the failed hold. A failed final insert rolls back budget
  changes; exact retry remains usable and changed decisions/deletion fail.
- A full synthetic provider/private-IPC workflow preserves one original
  quote/execute/payment identity. A lost payout cost ACK retains its full hold;
  restart and the same pinned core receipt synchronize without another payment.
  Core review retries avoid another provider read, and the intake role cannot
  read the payout-only cost receipt.
- Approval validation rejects implicit waivers, unsupported fields, invalid
  bounds and empty review explanations. Actual unprivileged file ownership
  fails; it cannot stand in for a protected operator decision.
- Core and payout recovery refuse missing, empty, outdated-history and incorrectly
  bound journals without initializing/upgrading them. Public/symlink paths fail.
  SQLite's existing-only open also refuses to recreate a removed database after
  path validation. The separate private payout directory fixture preserves the
  group-traversable IPC fixture rather than weakening production permissions.

The CI job then selects exactly its compiled native library-test artifact and
runs the protected-file test as root in a disposable network namespace. Both
`ROOT_OWNERSHIP_FIXTURE_VERIFIED` and an actual one-test passing result are required.
This positively checks root-owned private approval/evidence files, changed evidence,
public/group-writable files, symlinks and unknown fields. It is synthetic file
authority evidence, not a production cost approval, new-UID installation or refund
certificate. Execution status alone never establishes a refund.

The legacy two-store restore helper now refuses CyberHerd schema before copying
or rotating core's epoch, including an uninitialized schema. Its four offline
fixtures pass. The retained native legacy restore regression constructs the
historical pre-CyberHerd schema explicitly, copies its actual legacy columns and
sequence state, checks foreign keys, then migrates/restarts real loopback daemon
and gateway processes. Paid/pending identities and signed bytes survive, with no
duplicate harmless owner command. This does not qualify four-journal recovery.

Earlier candidates exposed Clippy placement/lock-scope errors, private-directory
and reactor fixture omissions, and the legacy integration fixture's use of a
current schema and additional presentation column. Those were corrected in the
final source above. No production file/schema check was weakened, and the legacy
end-to-end regression was preserved instead of being removed or skipped.

[Deployment 42785366523](https://github.com/lightning-goats/lightning-goats/actions/runs/42785366523)
passed all five jobs. Browser job `129725108397` passed **28 mocked-network tests**.
Package job `129725108019` built all release binaries, passed **eleven non-skipped
CLI checks**, and verified an inactive three-binary/**38-file** review archive
with `activation=none`. The added CLI check rejects incomplete/conflicting
recovery arguments and unprotected configuration without creating state or
loading sender credentials. Existing core systemd/install fixtures are not
installation evidence for the new CyberHerd roles or review guard.

[Real NIP-46 42785366575](https://github.com/lightning-goats/lightning-goats/actions/runs/42785366575),
job `129725108647`, passed the existing isolated real-nak regression. It does not
qualify a new household client or an actual public relay destination.

[Security 42785366628](https://github.com/lightning-goats/lightning-goats/actions/runs/42785366628),
job `129725108288`, fails solely on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1`; 315 locked dependencies were scanned. RSA remains
unreachable. No dependency version/checksum or advisory waiver was changed.
The [advisory](https://rustsec.org/advisories/RUSTSEC-2026-0459.html) identifies
patched `libsqlite3-sys >=0.39.0`. Compatible remediation is still required.

See [the recovery contract](../architecture/cyberherd-failed-cost-recovery.md).
No service, credential, financial review, payment, public publication or physical
feeding was installed/activated by this source work. Keep PR111 draft. Consistent
whole-stack backup/restore, retention operations, dependency remediation, reviewed
inactive installation, actual identity/network/helper/provider/household-signer/
browser qualification and explicit financial activation remain required by the
unchanged twelve-part implementation contract.
