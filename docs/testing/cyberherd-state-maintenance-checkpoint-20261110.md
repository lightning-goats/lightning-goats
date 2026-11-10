# CyberHerd four-journal maintenance checkpoint — 2026-11-10

This checkpoint advances the operations portion of the full twelve-part port.
It does not install CyberHerd, adopt a recovered backup, activate payouts or
qualify actual household/provider/browser integration.

Reviewed code is `c059d0655e58cc7e9c61484255d9e1d4cd6220ce`, tested as proposed merge
`4e250bc68742c5ee6772bcf3a7e3adf32520a6d9`. The GitHub head and merge Git-data trees
match local feature tree `ce5647605d239bc783d95e529e8714c4164c865c`.

[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42800143878)
passed format, strict locked all-target/all-feature Clippy and the full locked
all-feature Rust suite: **491 passed, zero failed, one ignored**. The two separately
repeated root fixtures and two inactive example tests are excluded from 491.
The native engine test again queries the actual SQLx connection for SQLite 3.54.1.

The five added regressions cover bounded policy-only checks and unprivileged
rejection; all four store startup guards, including a dangling marker; exact
masked/inactive/zero-PID unit parsing; a native four-journal outgoing workflow;
and actual root-owned four-UID private-file snapshot/staging authority. The
ordinary root fixture is a negative check under an unprivileged test process.
Its separate CI execution is accepted only after the positive
`ROOT_FOUR_JOURNAL_FIXTURE_VERIFIED` marker. The existing financial-review fixture
again requires `ROOT_OWNERSHIP_FIXTURE_VERIFIED`. Both run in disposable network
namespaces, without actual services or credentials.

The root fixture retains nonempty committed WAL, a paid receipt, original pending
feeder UUID and exact signed core outbox bytes. It verifies original main/WAL/SHM
bytes remain unchanged, rejects hard-linked/private-permission changes, active
writer observations, reused destinations and corrupt images, and stages all four
journals under startup markers. The unit-status parser is real production code;
the root fixture's writer-state observer is test-only, not actual systemd
quiescence qualification.

The native payout fixture uses the real four schemas, synthetic authenticated
HTTP and private Unix framing. It checks Pending submission, original execution
claim/quote/provider identity, a Failed result retaining its gross hold, exact
final core ACK, explicit synthetic failed-cost review and payout cost ACK.
Original signer result bytes survive. Inspection sends no provider request or
new payment, and never changes held funds. Old payout quotes, wrong epoch/account
and altered membership plan JSON fail, including a copy whose stored hash columns
and SQLite integrity still agree. The tool compares exact frozen plan bytes and
recomputes the core-committed hash before accepting correlation.

[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42800143988)
passed the offline driver provenance and actual RSA reverse-tree checks and
scanned 315 locked dependencies against 1,430 refreshed security advisories. There
is no new dependency, schema, migration or advisory waiver. The qualified native
engine parses only copied journals; Python/system SQLite is not substituted.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42800143901)
passed all five jobs. Release packaging passed **13 non-skipped CLI tests** and
verified a disabled **four-binary, 100-file** CyberHerd review archive with
`activation=none`. It includes `cyberherd-statectl`, the inactive maintenance
policy and `STATE-MAINTENANCE.md`, as well as the retained driver provenance.
The core archive's existing help/install/systemd rehearsals still pass; those
fixtures do not install or qualify the new CyberHerd roles on the actual host.
The browser job passed **28 mocked-network tests**. The nginx/provenance/root
upgrade fixtures also pass; environment-skipped general discovery is not counted
as entirely non-skipped acceptance.

[Isolated real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42800143806)
passed the existing native nak/bunker/sandbox regression. This is not actual
household client authorization or public relay delivery qualification.

The [maintenance contract](../deployment/cyberherd-state-maintenance.md) retains
all monetary/replay/signed history. Snapshots require stopped/masked writers and
copy original main/WAL bytes into a new private bundle. Verification checks exact
hashes, migration history and cross-journal scope. Restore staging preserves the
original epoch/UIDs/IDs and blocks all normal store startup; no tool command can
remove the block, install the journals, unmask units or resume transfers.

A consistent older core and payout can both have lost evidence of a real external
transfer. Independent provider/home-owner reconciliation through the outage and
backup interval, separately retained review evidence, and a concrete explicit
operator adoption action remain required. Internal consistency is not automatic
financial recovery authority. The existing live service/configuration/keys,
financial reviews, payment/credit/food history, Nostr publications and gateway
were not changed by this checkpoint.

Keep PR111 draft and the full goal active. Reviewed inactive installation, actual
UID/socket/network/helper/provider/engine/household-client/browser qualification,
external recovery adoption and explicit financial activation remain unfinished.
