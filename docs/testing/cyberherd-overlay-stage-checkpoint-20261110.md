# Overlay stage preparation — 2026-11-10

**Passed:** the actual VPS now has a private, native-validated configuration
review bundle for incoming zaps, membership intake, overlay messages and a
loopback roster view. Preparation changed no installed configuration, service,
identity, credential or application binary. It implements no activation command.
The full twelve-part port and the maintenance/apply/resume executor remain
unfinished. No maintenance approval is requested at this checkpoint.

Qualified code is `a0fed3dbe256ff26203b2761af927b36f0e8ae41`; the tested merge is
`9824fffc9bc8d65e45c3f10152377edd28dbe0f4`. GitHub's comparison reports zero changed
files between them. The selected code tree is
`f0a9ece8dd25aa4fc7fa6e42290c92f2de07e9da`. All ten workflows passed, including:

- [CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42887202636):
  format, strict locked all-target/all-feature Clippy, all-feature tests and
  required positive root-ownership/state/bootstrap/legacy-upgrade fixtures.
- [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42887202609):
  all five jobs, including the explicit native stage example test, root disposable
  preparation/refusal fixtures and installed systemd rehearsal.
- [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42887202925)
  and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42887202770).

The preceding package job `130074758434` failed because the new test fixture
indexed optional TOML tables before inserting them. The correction changes only
fixture construction to explicit table insertion. The final package job
`130078553981` positively ran the corrected example test. Earlier passing Rust
tests alone were not treated as evidence for that separately invoked example.

Test-only artifact `13796292713` has ZIP SHA256
`68ee8a419e598207199f9d5d46bb85cc628166f0605b5015f70deac4990e3531`.
The outer digest, all five inner checksum entries and source-bound tool export
were verified before selecting only `CHECK-ONLY-overlay-stage`. Its 3,973,856
bytes have SHA256
`b27597900aced427645ffc10c82e6cfb15434c5544ed28c8f790202c1e300025`.
It was installed under a new protected prefix, separately from application code.

All sixteen tool files were independently matched to pinned GitHub blobs, then
copied as data to a new protected root-owned generation. No writable checkout
module was interpreted as root. The external manifest SHA256 is
`b576c4b9736e9181b16bb6e8d7a22ea17c3873ca3ccff9c882ec451b9680a7ef`;
the launcher SHA256 is
`78da2cfee20d8d64ad150efa5f3770a0eff22afcb80cb5f19cfdff960234cfd7`.
System checksum verification preceded its authenticated `--help` and `prepare`
calls. All older tool generations and the installed credential importer remain.

Actual preparation positively checked the accepted inactive installation, locked
non-admin identities, all fresh role-journal hashes, original running core and
installed programs, protected four-role maintenance policy, unused bridge paths
and available loopback view port. The native policy check opened no state.
The separately pinned native validator accepted the actual candidate TOMLs, and
`systemd-analyze verify` accepted the candidate core unit.

The bundle retains exact originals, candidates, hashes and a recoverable
two-configuration replacement plan. The candidate preserves original payment,
gateway, feeder, mode, amounts and all six individual receive payees. It preserves
the core epoch and original ingress cutoff; the fresh domain message cutoff is
separately explicit. Core reward ceilings remain zero, outgoing peers/execution
are absent, delegated feeder messages remain false, and membership has no Nostr
signer, lifecycle, rewards, payout bridge or operator HTTP routes. Its view is
`127.0.0.1:3340`. Neither the proposed tmpfiles file nor socket parent was created.

The proposed core unit adds only the dedicated bridge supplementary group and
writable socket parent while retaining the original private runtime mode. The
proposed parent is core-owned 0750 with the dedicated bridge group. This does
not grant membership access to the core credential directory.

Independent readback verified candidate hashes, unchanged original file bytes
and permissions, the three unchanged role-journal hashes, preserved routing and
epoch/cutoff, zero limits and absent membership signer. The private prepared
receipt has SHA256
`26ed99e63078eeb703acc43ca6ac3d65896b7239dd20cae0abf7ad0a57367852`.
Role-journal bytes were read only for hash comparison; no journal was parsed and
the live core journal was not opened. No credential was loaded. The receive PID
and zero-restart count were unchanged; local health/status returned 200. All three
CyberHerd services remain masked/inactive with zero PIDs.

Next work is the reviewed, rehearsed maintenance/apply/resume executor with a
current native four-journal backup and configuration-only rollback. Public
nginx/OBS integration, actual incoming settlement/receipt delivery, membership
presentation, provider/payment qualification, externally reconciled recovery and
later operator-directed activation remain open. Preparation does not satisfy
those gates. See the [runbook](../deployment/cyberherd-overlay-stage.md).
