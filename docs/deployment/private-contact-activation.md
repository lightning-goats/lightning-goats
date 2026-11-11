# Private contact configuration activation

This is a configuration-only step after the [actual schema-24 upgrade](../testing/private-contact-schema-live-20261111.md).
It neither migrates nor restores a database and does not change financial limits,
physical policy, programs, credentials, nginx or browser flags. CyberHerd public
messaging, lifecycle, rewards, operator routes and both outgoing roles stay off.

The candidate twenty-one-file authenticated tool generation adds
`--private-contact-activation` and `--private-contact-site`. Independently verify the full source closure and
the launcher before interpreting protected installed Python with `-I -S -B`.
Older tool generations and completed upgrade transactions remain untouched.

`prepare DIRECTORY --validator PATH --validator-sha256 SHA --inbox-relay WSS`
freezes the active core/overlay-member installation, preserves original files,
and prepares a transaction for exactly one destination: the core configuration.
The separately pinned native `CHECK-ONLY-private-contact-stage` accepts only an
added `[contact]` table with the existing project recipient and bounded public
inbox relays. Every other parsed configuration value must match. Its positive
report must explicitly assert no journal, credential, provider or service work.
Use the recipient's signature-verified public kind-10050 inbox list. Preparation
opens no journal, stops no service and does not enable the feature.

After independent preparation-digest readback,
`apply DIRECTORY --review-sha256 SHA --exclusive-maintenance` freezes the same
four writers, masks/quiesces them and verifies a current native schema-24
four-journal snapshot. It fingerprints original main/WAL/SHM images, replaces
only the reviewed configuration and verifies both native configuration semantics
and unchanged live images. Services remain stopped. No stale database rollback
or automatic reapply exists; retain evidence and inspect any partial failure.

`resume DIRECTORY --approve-resumption` re-verifies the accepted backup manifest,
native report, current images, original programs/units/configuration, runtime
identities and masks before restoring the exact original core unit and member
mask state. Only the two existing services start; payout and signer stay masked.
Each phase retains private immutable receipts and shares the existing host
maintenance lock. Administrative exclusivity is still required.

The required protected-file fixtures mock native/systemd operations and explicitly
cover backup refusal, validator/review drift, unknown replacement destinations,
changed paid state, altered backup manifests and ambiguous retry refusal. Separate
native tests enforce configuration semantics. The disposable-systemd rehearsal
uses the actual checker, current native backup/verifier, transaction and resumed
active core with synthetic credentials in an isolated loopback namespace. Its
signing executable intentionally does not exist; malformed contact requests must
fail before any provider operation. It proves no real signing or paid delivery.

The separately selected `--private-contact-site` prepares and activates only
the optional exact contact nginx ingress and minimal static form release. Its
`prepare PAYLOAD DIRECTORY --source-manifest-sha256 SHA` requires an empty private
review directory and a private four-file payload (`index.html`, `contact.js`,
`contact-fee.js`, `contact.conf`) plus `SOURCE.json`, whose full source commit must
match the externally authenticated tool. Verify every payload blob independently
against that GitHub commit before root-owned data installation. The manifest is
`{"version":1,"source_commit":"FULL_COMMIT","files":{"NAME":"SHA256"}}`.

Preparation requires the contact-configured core already running, clones the
entire existing public release, preserves artwork/vendor files and all existing
boolean site choices, and changes only the three browser files plus the new
contact flag. It retains the existing SELinux labels on public directories and
new/changed files, using the original index label for new contact scripts and the
original site configuration label for the new snippet. Absence of selinuxfs in a
caller namespace does not prove the serving worker is unconfined. No MAC policy
is changed or disabled. It installs an unused exact nginx snippet and prepares a one-file
site-configuration transaction. No service or public route changes yet.
`apply DIRECTORY --review-sha256 SHA --activate-contact-site` rechecks all original
and candidate files, native-core configuration identity and source bindings;
tests nginx before and after the configuration replacement, then reloads only
nginx. A failed new syntax test restores the original site before any reload;
an uncertain reload requires inspection, never blind reapply. Existing static
releases remain available. No application restart, credential access, journal
open, invoice or payment exists in this site operation.

Protected file fixtures cover preservation, source/symlink refusal, changed
artwork, and failed nginx validation. These tests mock nginx/systemd; separate
nginx/WAF/browser tests and actual public readback are required.
Actual fee settlement and recipient observation remain distinct acceptance work;
neither simulated settlement nor relay ACK establishes a recipient read receipt.
