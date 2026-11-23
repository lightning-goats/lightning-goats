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
The operator subsequently supplied the private input-file path and attempted its
import. The protected entry point successfully imported the separate key, verified
its encryption round trip and retained the existing receive credential. It made
zero API calls, sent no payments and changed no services. A synthetic non-key
argument also verified that accidental relative key arguments fail without echo.
The first key's receive-read requests returned 403; the working receive key read
the same resource with 200, and the imported bytes matched the operator's file.
The operator confirmed that `partner.receive-request.read` had been omitted and
explicitly regenerated the key. Before reimport, the three role services/configs/
journals were positively absent and the original core configuration still had no
financial bridge. The earlier ciphertext was retained privately; the regenerated
key was then encrypted through the same authenticated importer.

Read-only requalification passed: listing receive requests returned 200, and the
new payout key read an existing resource obtained with the core receive key,
matching its exact request/currency/invoice context. These three GETs sent no
payments and changed no services. Payment-read, quote and execute access remain
unverified; this narrow successful read is not full financial qualification.
Two separate encrypted NIP-46 authentication clients were also generated through
the pinned factory, verified distinct from the existing core/project/each other.
No household allowlist, service or publication changed. A household-only command
file is prepared, but has not been run there.

Separate role installation,
household authorization, OS/socket/network/helper/browser qualification,
independently reconciled recovery adoption and explicit financial activation
remain unfinished. PR111 remains draft and the full twelve-part goal stays active.
