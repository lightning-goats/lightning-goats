# Recipient relay routing — live 2026-11-15

The operator approved broader public publishing and then explicitly excluded
measurement of downstream visibility. PR [#133](https://github.com/lightning-goats/lightning-goats/pull/133)
is merged and the verified programs are installed. Public payment/feeding and
CyberHerd notes now freeze recipient NIP-65 read destinations and author write
destinations alongside configured public relays. Send It supplements direct
publishing. NIP-17 inboxes remain separate and exclude the broadcaster.

## Exact source and qualification

| Identity | Value |
| --- | --- |
| Reviewed head | `d17ee218074fd70dba27c414026f658ab67fb56e` |
| Tested integration source in artifacts | `742245296f6b9c1f02fbbdb8797eb305aa11381b` |
| Merged main | `4c1a911cd4da91e42a34f58a43d4cf50da992dad` |
| Identical tree for all three | `1b09596fe1910a2a7f08ee92af064411bdd91227` |
| Core archive SHA256 | `0990c9407d7f9a22b930f0e3de0246b24c317594345f42b00972d922f268fb24` |
| Member archive SHA256 | `8b91c1da5334f616c5e5a77fd6dd68affe0807c98a9f98d15ab960ed700afb3c` |
| Installed lightning-goatsd SHA256 | `ec5ce51c48a3464ea18e2f32386d126b4e22d92fb40e5225512df6214e39ccbb` |
| Installed cyberherdd SHA256 | `6e84daa6660f3d19c0ca286050d02d237a20409d012ecc107614103b167e3b6d` |

Release ZIP digests matched the authenticated GitHub artifact API: core artifact
14168763983 (`348f3b1e9df46f50a2832d763480e83743ddaa517e96b509c1cd81d00082bd68`)
and member artifact 14168988186
(`d4580308681c56f76ca3e318cd44a69580876b10075e150bc38ae2c22507dd3d`).
Outer archive checksums, complete inner payload manifests and exact source were
verified before candidate execution. No mutable build output was selected.

All required checks passed: [Rust/Clippy/tests](https://github.com/lightning-goats/lightning-goats/actions/runs/43732104686)
(449 library tests passed, six ignored, plus integrations and root ownership checks),
[audit](https://github.com/lightning-goats/lightning-goats/actions/runs/43732104787),
[real isolated nak](https://github.com/lightning-goats/lightning-goats/actions/runs/43732104762),
and [deployment/systemd](https://github.com/lightning-goats/lightning-goats/actions/runs/43732104690)
(systemd job 132868790516). Website, nginx, held-owner, gateway, peer and network
checks also passed. Tests use synthetic credentials/events and isolated relays.
Regressions cover signed NIP-65 replacement/direction, hint discovery, author writes,
private/public separation, frozen member destinations, retry fairness, restart
replay and immutable metadata signatures. Core polling consumes only newly appended
sequence-indexed records; accepted routes leave the in-memory pending projection.
No new database/schema/cache authority is introduced.

Earlier qualification failures (unused import, invalid old fake-signature fixture,
loopback transport mismatch, and missing spawned-transport lifetime) were corrected
before this exact source passed. Prior successful revisions were not deployed.

## Applied transaction and evidence

The protected review is
`/var/lib/lightning-goats-upgrades/recipient-relays-20261115-final-review`.
Preparation SHA256:
`e2ae73f44b5d40785db090d9d89c9d1fe4feffdcf2ef6b12abb64a18fc1c6cb0`.
Final `RESULT.json` SHA256:
`eb5e624e9a22da03382b1e323c111d74ccad54ef0b42ba047457bd7ec64efb3a`.
The review retains original/staged file fingerprints, recoverable file transactions,
original unit bytes, native backup/verification reports and a post-start check.

Preparation ran the candidate member parser with the retained configuration as UID
975 under isolated networking, without starting its runtime or loading credentials.
The first preparation stopped at a tar member-name prefix mismatch before any
production changes; that failed review remains intact. A corrected fresh review
was fully prepared and used for the rollout.

All four writers were stopped and persistently masked for a fresh native snapshot.
The installed statectl and the candidate statectl independently accepted the same
four-journal bundle. Original databases/sidecars remained byte-identical through
file replacement; no migration, restore, financial correction or credential import
ran. Only `/usr/local/bin/lightning-goatsd`, `/usr/local/bin/cyberherdd` and the live
site’s `contact.js` were replaced. The candidate statectl was verification-only;
the installed financial/operator tools, payout and signer programs stayed exact.

Original units and enablement were restored. All four workers ran under their
existing non-admin identities with pinned executable hashes. Core and member
HTTP health checks passed, and a later check again found all four running.
Local retained relay-list plans/attempts/ACKs show the new protocol path executing;
this is not a claim about downstream propagation. No relay event-presence or
read-after-publication query, real payment or physical feeding test was submitted.
Existing lifecycle and stream-chat opt-outs, payout budgets, credentials,
FeederOverride, feeder controls and network policy remain unchanged.

The separately applied Send It configuration evidence is retained in
`/var/lib/lightning-goats-upgrades/sendit-broadcast-20261115-review/RESULT.json`,
SHA256 `51c9f76957bfbc9d964273da8af9f36d05624eaeecc4f327fca025579f30d8ed`.

## Operational boundary

A matching positive ACK ends retries only for that destination; broadcaster
acceptance is distinct from direct-relay acceptance and is not wider delivery proof.
Absent/unreachable signed lists retain configured public fallback destinations.
Public networking remains bounded to validated, DNS-pinned public WSS on port 443.
Authentication/payment-restricted or unavailable relays can refuse publication;
there is no propagation guarantee or downstream measurement.

For program rollback, stop/mask writers again and use the retained file transaction
under a fresh reviewed maintenance check. Preserve current journals and immutable
publication records; do not restore the old checkpoint or mint replacement events.
The existing recovery-adoption blocker and financial/physical acceptance history
remain separate from this publication-only change.
