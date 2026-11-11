# Private contact schema — actual upgrade, 2026-11-11

The actual receive core now uses schema 24. Core and overlay-only membership are
running again; the 28-sat feed credit, paused feeder and zero queued/unresolved
feeding state are preserved. Contact configuration, nginx routes and browser form
remain disabled/uninstalled. Both outgoing roles remain masked, and financial
ceilings and membership signing remain unchanged.

## Qualified source and artifacts

Source `9a7c0931d2ebb22b0458679c4aeb5a0c92d9e8dc` passes all ten workflows:
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42945666584),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42945666561),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42945666657)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42945666632).
Tested merge `4de9ff4ca470a8918a7c8b4563982599d708bf64` has zero file differences.
Native format, strict locked Clippy, all-feature tests, separately required root
fixtures, contact-only configuration checks and installed systemd pass. The
installed rehearsal actually exercises native schema-23 backup, original-UID
clone migration, both four-journal comparisons, actual schema migration and
resumption of the new core/membership. Its old-core ELF is explicitly a harmless
stand-in; no production credentials, fee or physical action qualifies from CI.

All four artifact ZIP digests and internal manifests were verified as data:

| Artifact | ID | ZIP SHA256 | Internal checksum entries |
| --- | --- | --- | --- |
| Core package | 13823196276 | `6136ba5089ac98f6da62a57481bb5eeefff913f61efba7c3267278496c6d8751` | 6 |
| CyberHerd review | 13822927448 | `87031fb842edfc277303101e021777582cbba2b8ea500b7717d90fdd11725b71` | 1, then 104-file verifier |
| Test-only tools/checkers | 13823141658 | `8144ada795df9455860aad21a094b68055fcad529dc1fdb8b6b553689c7e8f6a` | 7 |
| Installed systemd report | 13822593179 | `2129e51564e60bbb4c8ba58b057ea3f676e9f66b87bb56b38cee6ea9a04b8557` | 6 |

The core archive SHA256 is
`c47e90e48607f828d1317816e5efead9d50d72ac47e3904138f7b9ea9444e6c8`;
the review archive SHA256 is
`9d1afc0eb06ab9944feb8f7712f1c3559656d5af26a7167cdce3a2189b78ad8a`.
The configuration-only checker SHA256 is
`45acfb90165e1ba9bc6388e933b3443072ecd57f0c54023875c1521eb7fb798f`.
No test-only state-transform or old-core fixture was installed on production.

## Actual preparation and execution

All nineteen tool files independently matched pinned GitHub blobs before a new
root-owned inactive generation was installed. Manifest SHA256:
`855aa4a2cbcc3949fc8fdb2f40dad1cafc8a206e125038560ee838f9dd503dca`;
launcher SHA256:
`fcea8989075936c6357677ae1e3afc566153798c250f51b2f48e531708ae2de4`.
External launcher authentication precedes isolated Python and closure validation;
no writable-checkout code executes as root. Older tool generations and the
earlier sealed preparation remain intact.

Fresh preparation verifies actual execution of the selected CLI's `--help`
under the original core UID, cleared groups/capabilities and a new network
namespace while services still run. All seventeen original file identities and
both PIDs remain unchanged. Preparation SHA256:
`6e1982bebf7bc831af0abf34c3fe859ccab2199a8bf8f28e2deaafd35c43d30e`.
The separately frozen execution record SHA256 is
`1cabe7ff82c94449d2f0e8e2ee05f2c066d073bdf3fc4b8a110b1b400090442a`;
its three-program transaction SHA256 is
`9da097a69c43d88db3f47ec9ed7bee66e8fe5f27dd47cdb8057d3d8d06d3a9b4`.
Independent readback verifies these bindings before maintenance.

The operator-requested deployment proceeds only after a fresh status check shows
active receive mode, override ON, no queued feeding and no unresolved request.
Apply masks/quiesces all four writers, takes and re-verifies a current native
four-journal schema-23 backup, migrates a separate core copy as the original UID
in a new network namespace, and accepts the strict comparison before replacing
three programs. It then migrates the actual core as that UID, snapshots all four
current journals and compares them again. Original rows, row IDs, signed bytes,
schema objects and migration-history prefix remain exact; only the compiled
contact DDL, migration row and three empty tables are added. No journal is
restored, reset or replaced with an older image.

Private receipt digests:

- Backup manifest: `8aa60d2deb06e3b2d9fdfa8e0b9ec31eabe9ea81c207c0e8797560cb1d6f58d2`.
- Migrated manifest: `55cc1f757ad868f4bb8c7d1cb45e4bfbd3eef0ab38359e46a347fc438487422e`.
- Comparison: `5c161b277c67c565aac61e48f88e8efefb1329f94fa5075def10a5e15a2eb0be`.
- Resumption: `d0d58b143cbacf3fa7810ad78faabbacbd69068be0bad0c2b8a163ececbb9037`.

Independent readback verifies matching policy/epoch, zero financial holds and
pending feeds, three empty private tables, unchanged other-role row counts and
exact installed program fingerprints. The native comparator, not count equality
alone, proves preservation of every retained value and signed byte.

## Resumed state and remaining work

Resumption re-verifies private images, comparisons, live image hashes and all
program/configuration/unit bindings, restores the exact original core unit and
membership mask state, then starts only core and membership. Both report running
with zero automatic restarts; outgoing services remain masked with zero PIDs.
Core boot enablement remains enabled; membership remains disabled at boot.

Installed program SHA256s:

| Program | SHA256 |
| --- | --- |
| `lightning-goatsd` | `ecccb1747aed69c61347b6c5f65ac2edadc1f8e71a5dc51dc84ebc20c1993fe5` |
| `lightning-goatsctl` | `1bcd037da4e31238aa1eeae6aa3bacc0ee6e76227bb2c531762395631ffa3c6f` |
| `cyberherd-statectl` | `70be3db8d71e2fe68c5b974413eb65c82670abbbe800f3a60be6a915b86b748f` |

Actual status confirms 28 sats, threshold 1,000, zero feeds due, no unresolved
attempt, override ON and reachable gateway. Homepage, leaderboard, roster API and
herd LNURL discovery return 200; discovery still advertises native zaps. The
website remains the verified `9abcd88` release and OBS/artwork are unchanged.
No invoice, payment, injected credit, DM, public note or physical feeding was
initiated as an acceptance test. Credentials, provider policy, DNS, WireGuard,
nginx and physical owner are unchanged.

Still required for contact: reviewed configuration activation with the native
contact-only validator, optional exact nginx ingress and privacy/WAF checks,
minimal static contact release preserving existing flags/artwork, and controlled
paid-contact/recipient observation. This checkpoint does not activate CyberHerd
publication, rewards, operator controls or payouts and does not complete the
[full twelve-part port](../planning/cyberherd-implementation-plan.md).
