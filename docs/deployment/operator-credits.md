# Local operator credit and payment-message minimum

The operator requested local CLI grants for feeder/overlay tests without sending
Lightning, followed by normal-looking public messaging and a minimum of **10 sats
credited per payment/grant** for payment announcements. This is a local operator
capability, not a public API or a change to Strike settlement authority.

## Commands on this VPS

Preview without adding credit:

```sh
sudo /usr/local/bin/lg-credit --sats 1000 --id obs-credit-001 --reason "OBS feeder test" --dry-run
```

Add the credit:

```sh
sudo /usr/local/bin/lg-credit --sats 1000 --id obs-credit-001 --reason "OBS feeder test"
```

Use a new ID (for example `obs-credit-002`) for another addition. Repeating the
identical ID/amount/address/reason is a durable no-op, even after feeding consumed
its credit or the service restarted. Changing data on an existing ID fails.
The output distinguishes `preview`, `granted` and `duplicate`; displayed credit
and feeds due refer to the grant transaction and may subsequently change as the
normal worker processes feeding.

`--address dexter` attributes a grant to that configured goat instead of the
default `herd`. All configured addresses use the same herd credit pool. The
required reason is retained privately for audit and is omitted from public event
payloads/messages. IDs use 1–64 ASCII letters/digits/dot/hyphen/underscore; grants
are positive whole sats and resulting credit must fit SQLite's integer range.

The protected wrapper runs the existing `lightning-goatsctl add-credit` command
as the `lightning-goats` runtime owner, preserving database/WAL ownership. It
requires sudo for mutation. CLI help needs no private credential. Direct CLI use
is also possible with the runtime identity and
`--config /etc/lightning-goats/config.receive.toml` before `add-credit`.

## Feeding and rollout behavior

A grant increases the existing feed-credit pool. Once it crosses the configured
threshold, the normal worker can perform a **real physical feeding** when local
controls permit. Hold `FeederOverride` ON to accumulate credit and test overlay
messaging without feeding; release it when ready to observe the physical path.
The grant itself submits no gateway command and creates no artificial feed
confirmation. All serialization, correlated UUID handling, local interval/caps,
remote-enable and ambiguity/recovery rules remain authoritative.

The installed threshold is currently 1000 sats. A 1000-sat grant adds to existing
credit; one confirmed feeding debits 1000, preserving the prior remainder. Actual
confirmation is required before a debit and feeder animation. A pure 2340-sat
fixture produces two confirmations and 340 remaining; this is an isolated
regression case, not a production grant performed by the deployment agent.

The same command works during the current feeding/overlay stage (`canary`) and
a later configured Nostr stage (`active`). It does not enable Nostr, install keys,
change the runtime mode, or bypass the isolated signer. Announcement text uses
the normal payment/goat templates without visible test labels, as requested.
Actual gateway-confirmed feed messages also use their normal presentation.

## Minimum announcement amount

Both real payments and operator grants of **1–9 sats** still commit credit and
update progress. They produce no payment ticker, payment popup, goat cards,
QR/lightning swap or Nostr payment announcement. Multiple small payments do not
combine into a delayed announcement; eligibility is per credited event.

At **10 sats or more**, normal announcements and effects are eligible. Nostr is
still subject to the configured runtime mode and signer. Actual feed
confirmations and periodic informational/weather messages keep their existing
independent behavior. LNURL minimum amounts and financial settlement are unchanged.

The renderer applies the threshold centrally to both output targets. The overlay
emits a cursor-preserving skip marker with current absolute credit for small
receipts; the browser independently enforces the same threshold. Nostr advances
its cursor without calling the signer or adding an outbox row for a small receipt.
Reconnect/replay handling remains unchanged.

## Durable audit boundary

`operator_credit_grants` preserves the ID, amount, address, reason, ledger entry
and durable event reference. In one `BEGIN IMMEDIATE` transaction the grant
creates an `OPERATOR_CREDIT` ledger entry and `operator_credit_added` event.
The existing `settled_payments`, provider requests and native-asset receipt graph
are untouched. Income reports should continue to use authoritative provider
settlements, rather than treating all feeder-credit additions as Lightning income.

Grant retries verify immutable metadata and linked ledger/event content; conflicting
or corrupt linkage fails closed. A failed late insert rolls back the grant,
ledger delta and event together. The same SQL transaction lock serializes native
settlement and concurrent grants. Local filesystem permissions protect this
capability; no HTTP grant endpoint or additional service credential is installed.

Migration 0012 adds the audit table. Upgrade the paired daemon/CLI from the same
verified package, retain all real state, and keep the root-private quiesced backup.
The release includes the migration source plus the embedded executable migration.
Never restore an older paid database as an application rollback.

## Verification

Relevant regressions cover concurrent retry, conflicting ID data, dry run,
invalid/overflow values, rollback on a late write failure, retained identity after
credit consumption/reopen, actual CLI processes without provider credentials,
worker override enforcement and exact harmless-gateway command/debit counts.
The 9/10-sat boundary is checked in renderer, overlay frames, browser effects and
the Nostr subprocess/outbox fixture. These remain isolated tests; live deployment
verification uses a dry run and does not mint production credit automatically.

Selected-source checks passed: formatting, strict locked all-target/all-feature
Clippy, 250 Rust tests with the dedicated real-nak namespace case explicitly
ignored, all 18 mocked browser tests and 126 deployment cases with five existing
skips. The security gate passed and RSA remains unreachable under the documented
exception. No production grant or real feeding was used to produce this evidence.

## Installed acceptance — 2026-11-08

Source `be2598306cb7c6aae7ea99759aafc05172c512e3` was packaged with SHA256
`f4adb1c7569543877b044765dbc85dd259c47426a86808d541df42b12cf8cede`.
Before installation, a consistent root-private copy of the live schema-11 ledger
was upgraded in a network-isolated namespace. Preview preserved all prior row
fingerprints; a private-copy grant/retry produced one addition and preserved
provider receipts/feed state. The copy was removed after the rehearsal. No
production credit was granted during that test.

The paired production binary/wrapper upgrade retained a private backup at
`/var/backups/lightning-goats/receive-upgrade-20261108T230435Z`; the matching static
update retained `/var/backups/lightning-goats/obs-20261108T230521Z`. The root-owned
receive/overlay install records retain the source, archive and binary hashes.
The daemon migrated to schema 12 and remained in canary mode with the existing
1000-sat threshold, 3 sats real credit, three provider settlements and no unresolved
feed/physical attempt. Both production wrapper dry runs (1000 sats and 9 sats)
returned preview, with announcement eligibility true and false respectively.
The operator-grant audit table remained empty and credit stayed 3 sats. The
wrapper and CLI are root-owned 0755; the database remained runtime-owned 0600.
No grant or feeder command was submitted by the deployment agent. The public
page serves the v4 client; existing OBS browser sources need one refresh to load
new JavaScript. Real operator-command feeding and later public Nostr observations
remain distinct from this deployment verification.
