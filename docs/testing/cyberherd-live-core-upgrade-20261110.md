# Live core upgrade — 2026-11-10

The operator explicitly approved the maintenance interruption, backup, migration,
comparison and restart. The existing receive core now runs the reviewed build
`5a9e367c46529a65ce59ed693e168efefca8bff1`, from qualified code
`2e3b289f4de907855668583e6dd9e5af37081024` and tree
`06fa6abf750b46a86a70d9ccad2b79e4ab1cc1a3`. Its exact CI, Security, Deployment
and isolated NIP-46 evidence is recorded in the
[combined transaction checkpoint](cyberherd-core-transaction-checkpoint-20261110.md).

The thirteen-file root tool closure was authenticated against the pinned git
objects and manifest before installation. The root-owned launcher was separately
authenticated before interpreting it. Core archive SHA256:
`1989cf4415c6f90c6d1bf8b30ea3ad6ee8a213cff7560b51eee458d5a2c3dabf`.
Preparation retained original protected binaries/config/unit/drop-ins without
opening paid state or interrupting service. Scratch extraction used a protected
directory on the main filesystem, not the space-constrained `/tmp` tmpfs.

Application stopped the existing core, established the persistent mask and
positively excluded runtime/admin writers. Native SQLite 3.54.1 verified the
pre-upgrade backup before binary replacement. Migration ran as the existing
non-admin core UID with cleared groups/capabilities and NoNewPrivs. The accepted
native comparison preserved original rows, IDs, epoch and signed bytes; both
installed binary hashes matched the pinned release. Database ownership and the
entire existing configuration were preserved.

Finish rechecked state/program/configuration drift and restored the exact original
unit without starting it. The separately approved start then resumed the existing
operating mode. The first immediate health probe preceded HTTP listener readiness
and returned a connection error; the subsequent health and status requests both
returned 200, with the service active/running and zero automatic restarts.
Private root-owned before/after, backup, transaction and comparison receipts were
retained. No paid snapshot was restored, epoch reset or signed event regenerated.

The three new CyberHerd services remain absent. No new bridge, reward spending,
CyberHerd publication or payout execution was activated. No test Lightning payment,
credit grant, public post or physical feeder command was sent by this maintenance.
The existing active core resumed its previously configured responsibilities.

The authenticated payout importer and protected convenience command are installed;
see [the credential procedure](../deployment/cyberherd-payout-credential.md).
Installation alone does not claim that a payout key has been imported or its
provider scopes/account binding have been verified. Separate role installation,
household authorization, OS/socket/network/helper/browser qualification,
independently reconciled recovery adoption and explicit financial activation
remain unfinished. PR111 remains draft and the full twelve-part goal stays active.
