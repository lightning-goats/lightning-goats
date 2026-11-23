# Actual inactive CyberHerd installation — 2026-11-10

The reviewed inactive installer has now provisioned the three service roles on
the actual VPS. They remain persistently masked, inactive/dead and PID zero;
no application worker, public listener, reward execution or publication started.
This follows the [approved core maintenance](cyberherd-live-core-upgrade-20261110.md)
and the [combined disposable rehearsal](cyberherd-core-transaction-checkpoint-20261110.md).

The source-pinned thirteen-file root tool bundle is
`2e3b289f4de907855668583e6dd9e5af37081024`. Both installed program sets use build
`5a9e367c46529a65ce59ed693e168efefca8bff1`. The four-program CyberHerd archive
was independently authenticated before execution with SHA256
`86ef1868f90d69d3da42a61c9113f07ff95717688e860c3acc983af6f4cb3bd6`.
Private root-owned installation/configuration/native-backup receipts are retained;
they contain actual identity, epoch, cutoff, provider and file fingerprints and
are not public evidence bundles.

The explicit identity step created three distinct non-admin users and dedicated
IPC groups. Actual NSS home/shell/primary/supplementary groups, locked passwords
and sudo denial passed. No membership was added to the existing core identity.
Configuration uses the original accepted core epoch and a current durable event
sequence obtained through its native WebSocket snapshot, rather than reading or
migrating the live ledger. That epoch matched the retained native core evidence.
All six actual payees were read from the existing public TLS identity registry;
this discovery does not require member NIP-05 verification. A first loopback TLS
probe could not connect because nginx binds its existing public address. The
correct existing listener then verified the certificate and registry; no network
or nginx change was made.

The new payout journal pins the corrected encrypted credential fingerprint and
one retained administrative account label. The label is not represented as an
API-provided identity; actual receive-read/same-core-resource access has separate
evidence in the earlier checkpoint. The signer configuration pins its separately
generated authentication client's public key. All three processes remain disabled,
membership messaging/view are absent, payout execution and every monetary ceiling
are zero, and signing admission/net ceilings are zero. No core bridge is enabled.

The actual authenticated installer verified the archive, accepted native core
backup and cross-configuration pins. It established persistent masks before
publishing the units, four root-owned programs and protected configurations.
Three native initializers ran under their own actual runtime UIDs/GIDs with
cleared groups/capabilities and NoNewPrivs, without loading credentials or starting
workers. Every new journal is a private single-link 0600 file under its own
runtime-owned 0700 state directory. Root-owned configs are 0644 under protected
0755 directories; programs are 0755. The final private installation receipt exists
and its incomplete marker is absent.

Post-installation readback verified those identities, files/configuration hashes,
private journals and all three exact mask/inactivity/PID-zero states. SELinux's
non-applying label check reported no relabeling for the selected programs/configs/
journals. The receive service retained the same original process with zero
automatic restarts, unchanged configuration/unit bytes and health 200. No test
payment, grant, public post or physical feeder command was sent.

The actual root-private four-journal maintenance policy is now installed and its
native configuration-only check passes. No live snapshot was attempted: the
existing receive writer remains running. The first archive-member lookup used an
unnormalized name and failed before creating the policy; normalized verified
member selection then succeeded. No paid source was opened or copied by that
policy check.

This completes actual **inactive installation**, not activation or complete
runtime qualification. The [disposable OS policy probes](cyberherd-os-boundaries-checkpoint-20261110.md)
remain distinct from actual-host sandbox/socket/DNS/network/helper/browser checks.
Household authorization and actual kinds 1/1311/9734 round trips, payment-read/
quote/execute qualification, independently reconciled recovery adoption,
operator-selected nonzero budgets and explicit activation remain unfinished.
The full twelve-part goal and draft PR111 remain open.
