# Installed Rust worker qualification on synthetic state

`qualify-cyberherd-workers.py` extends the
[installed OS policy checks](cyberherd-installed-qualification.md). It runs the
installed native core, membership, payout and signer binaries under four distinct
locked users, with separate synthetic journals. The original receive service
keeps running and the three production CyberHerd units stay masked. This source
has passed both disposable-systemd and actual-VPS qualification, as recorded in
the [source-pinned checkpoint](../testing/cyberherd-worker-qualification-checkpoint-20261110.md).

Use a fresh independently authenticated fifteen-file tool generation through the
[protected launcher](inactive-upgrade-transaction.md). Earlier generations remain
intact. Select the accepted inactive installation and original accepted core
upgrade transaction, plus a new private destination directly under the protected
upgrade base:

```sh
sudo unshare --net -- /usr/bin/python3 -I -S -B \
  /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py TOOL_MANIFEST_SHA256 \
  --cyberherd-workers \
  --installation /var/lib/lightning-goats-upgrades/ORIGINAL_INACTIVE_INSTALL \
  --core-transaction /var/lib/lightning-goats-upgrades/ORIGINAL_CORE_UPGRADE \
  --destination /var/lib/lightning-goats-upgrades/NEW_WORKER_QUALIFICATION \
  --create-isolated-fixture
```

The command refuses the host network namespace, any interface except isolated
loopback, active/changed production roles, changed installed programs or original
configuration/journal fingerprints. The core template is authenticated against
the retained release. It creates or verifies one dedicated locked, non-admin
fixture-core user; the three other workers use their actual installed identities.
No runtime group membership or production configuration is changed.

Unique temporary units reconstruct only selected reviewed sandbox properties;
they inherit no original dependency, Exec hook, state/runtime directory,
environment file or credential load. They have private fixture write paths,
128 MiB memory limits and no swap. Production state/configuration, runtime
sockets and credentials are inaccessible. Known invalid receive/payout keys and
a fixed synthetic NIP-46 client are encrypted only inside the private fixture
transaction. A local helper fails if unexpectedly invoked; the real nak helper
and household bunker are not used. The isolated network has no external
interface or route. The shadow core has no incoming zap support, Nostr publishing
or informational producer, and talks only to a local no-actuation mock gateway.
Execution, reservation and signing ceilings stay zero.

The real Rust Unix-socket servers check actual mixed-UID clients. Positive cases
cover intake safety/event reads and payout plan export. Negative cases cover
payout access to intake reads, intake traversal into the payout signer, the core
socket owner's unauthorized peer identity, arbitrary signing and a native-validated
well-formed payout-signing request rejected by zero limits. Each named proof must
be positively reported; an empty/partial report cannot pass. Clients run under
temporary sandboxed units and check the server's kernel peer UID.

The same four fixture journals reopen after one complete worker restart. Every
worker's actual kernel UID/GID/groups, capabilities, NoNewPrivs and network
namespace must match the fixture policy. Both
cycles check an empty native read-only roster and the same core epoch. The tool
then stops and masks only its own temporary writers and uses native
`cyberherd-statectl` to snapshot and verify all four synthetic journals. The
snapshot must have zero settled payments, feeder attempts, publication outbox,
provider execution and signer intents, no unresolved monetary work, no mock
feeder POST and no helper invocation. Original role bytes/masks and the receive
process/unit/configuration must still match afterward.

Success retains `RESULT.json` and the native fixture snapshot. Failure retains
`INCOMPLETE.json`, private diagnostics and synthetic files for inspection. Cleanup
touches only the exact temporary unit files it created; changed files require
inspection. Never delete original journals or reuse a failed transaction to
force a pass.

This proves only the stated synthetic native worker/socket/restart boundary.
It does not qualify production recipient DNS, provider payments, household
signing kinds, browser presentation or externally reconciled recovery adoption.
It does not authorize nonzero budgets, production unmasking, publishing, payments
or physical feeding. The disposable installer invokes this same authenticated
entry point and retains its result separately from Python OS-probe evidence.
