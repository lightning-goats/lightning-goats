# Existing project operator and three-user capacity

The operator selected **3 user spots** and the existing Lightning Goats messaging
identity for authenticated operator access. This changes capacity, not the member
reward percentage. Signing stays with the household NIP-46 bunker; no new project
signing key is generated or copied to the VPS.

The authenticated root tool's `--cyberherd-project-operator prepare` operation
accepts the existing messaging public key, freezes the current installation,
retains the original membership configuration, adds that public identity to its
existing operator allowlist and sets the initial-capacity seed to 3. Its installed
native parser must accept the candidate under the original membership UID with
networking and capabilities removed. Preparation changes no running services.

`apply REVIEW --review-sha256 SHA --approve-project-operator` installs only this
configuration and restarts only membership. Core payment/feeding process identity,
all other protected files, outgoing masks and zero financial limits are checked.
It neither edits journals nor changes already stored membership settings. The
one-shot application fence requires inspection after an interrupted application.

The root-owned `set-cyberherd-capacity.py` client subsequently runs as the existing
non-admin membership identity in a private transient systemd unit with its
existing encrypted NIP-46 client credential. Its sole operation is a signed
NIP-98 settings request to the loopback operator API. It preserves the current
minimum and reward percentage, uses one command UUID across uncertain retries,
and verifies the audited receipt and unchanged members/cycle before reporting
success. Kind 27235 proofs are signed, not published to relays. The public roster
must independently report capacity 3 afterward.

Changing a configuration seed alone is insufficient for an existing roster.
Native operator validation refuses implicit eviction if active membership exceeds
the selected capacity; the client does not remove members or retry with a different
command. No financial activation, payout, payment or feeding is requested by this
operation. Existing sealed program-upgrade preparations become stale after the
configuration/member-process change and require fresh preparation.

Protected-file tests exercise configuration scope, native refusal, review digest
binding, one member-only restart and unchanged financial images with mocked
systemd/native calls. Client tests verify the exact settings payload, unchanged
allocation, original UUID reuse, idempotent no-op and refusal of unexpected state.
These tests do not substitute for the live signed request and public readback.
