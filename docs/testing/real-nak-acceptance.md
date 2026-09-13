# Real NIP-46 compatibility and bunker output correction

Production HOLD. This evidence uses public test scalars, an ephemeral local
relay and a fresh network namespace with only loopback. It does not validate
the project identity, production signer service, public relays or physical actions.

## Confirmed output disclosure

The shipped bunker wrapper reads the signer credential into `NOSTR_SECRET_KEY`
and invokes `nak bunker` without persistence. In upstream nak v0.20.6,
`main.go` maps that environment variable to `sec`. `bunker.go` builds a restart
command containing `sec`, then prints it on stderr together with a bunker URI
containing a generated pairing secret. That pairing secret can authorize another
client. The footer can repeat after startup; runtime request/response output
also carries sensitive data. The shipped systemd unit does not suppress these
streams, so the wrapper would send the output toward its journal.

Upstream source: [v0.20.6 bunker.go](https://github.com/fiatjaf/nak/blob/fb21939f45538eff414068fa1bb6f1bea705f447/bunker.go).
The source revision is `fb21939f45538eff414068fa1bb6f1bea705f447`.
The official linux-amd64 release asset SHA-256 is
`b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9`.

An isolated real-binary reproduction on 2026-09-12 confirmed that the unmodified
wrapper emitted the synthetic signer key and pairing URI on stderr. The process
was healthy and emitted 639 bytes. The corrected wrapper remained healthy and
emitted zero bytes on either stream. Only booleans and byte counts were retained
from that probe; no production key or journal was inspected.

The fix redirects both child streams at the final `exec`. It covers repeated
output, formatting variations and direct writes without trying to parse secrets
out of logs. Safe wrapper validation errors remain visible before execution.
The process retains its exit status and direct signal handling. Raw nak
diagnostics are intentionally unavailable; systemd process/exit/restart state
and an explicit signing acceptance check remain the operational evidence.
No persistent signer-key config is introduced.

## Regression tests

`deploy/tests/test_bunker_output.py` failed on the original wrapper and passes
on the correction. It models credential output on stdout and a different
pairing capability on stderr; it also checks exact arguments, environment-only
key delivery, no persistence, unchanged exit status and visible safe setup errors.

`tests/real_nak.rs` uses the real Rust `NakClient`, message processor and SQLite
outbox with the pinned real nak binary, the shipped bunker wrapper and nak's
in-memory relay. It checks:

- NIP-46 signing preserves the expected identity, content and tags;
- valid signatures pass and changed content fails verification;
- signing alone publishes no kind-1 note;
- the wrapper emits no child output during startup and signing;
- a relay outage leaves a durable failed outbox entry;
- closing/reopening the database preserves the exact signed JSON and event ID;
- retry succeeds after relay restart with the bunker stopped;
- duplicate publication retains the same note;
- informational and weather events remain overlay-only with the signer stopped.

The test refuses a namespace with any link other than loopback and verifies the
nak binary digest before executing it. It is ignored in ordinary Rust test runs
because its environment must be prepared explicitly. The dedicated
`Real NIP-46 acceptance` workflow builds it with locked dependencies, downloads
the checksum-pinned binary and runs the test as the unprivileged runner user in
a fresh network namespace. Its log is retained as an artifact. A successful
ordinary Rust workflow alone does not prove this test ran.

Focused wrapper checks:

```sh
sh -n deploy/scripts/run-nak-bunker
python3 -m unittest discover -s deploy/tests -p test_bunker_output.py -v
```

For a manual Rust reproduction, build `cargo test --locked --all-features
--test real_nak --no-run`, then run the resulting test executable with
`--ignored --nocapture` as an unprivileged user inside a fresh loopback-only
network namespace, setting `LG_TEST_NAK` to the checksum-pinned binary. The
workflow contains the exact namespace setup and executable-discovery commands.

## Shipped systemd sandbox rehearsal

`deploy/scripts/rehearse-bunker-systemd.py` adds an actual transient systemd
service derived from every Service directive in the shipped bunker unit. Only
the executable and fixture paths are substituted; a loopback-only network
namespace is added. No unit is installed or enabled. It uses the same pinned
nak binary, public synthetic scalars and a temporary local relay. A host-key
encrypted synthetic credential is created, its plaintext deleted, and the
service must sign and verify an event both before and after restart. The test
also checks the live process is non-root, has no effective capabilities, has
NoNewPrivileges enabled, uses the isolated namespace, emits no process journal
output and publishes no kind-1 note while signing. Corrupting the encrypted
credential must fail before execution with systemd status 243. The separate
persisted bunker configuration must be absent; ordinary LMDB caches and the
local control socket are expected runtime data.

The dedicated workflow runs this rehearsal after the Rust acceptance test and
retains `BUNKER-SYSTEMD.json`. This exercises the sandbox using synthetic
credentials; it does not establish production credential custody, public relay
compatibility, or isolation from another live application service. The script
requires an existing systemd host credential key and never initializes it on
an operator host; only the disposable CI runner setup initializes that key.

On the Fedora VPS, the synthetic rehearsal passed on 2026-09-13 after labeling
only disposable executable copies for their intended installed paths. The
first unlabeled-copy attempt could not connect to the relay (permission denied);
no SELinux policy was changed. An initial overbroad assertion that the runtime
directory contained no files also failed: upstream initializes LMDB caches and
a control socket without enabling signer configuration persistence. The final
assertion checks the source-defined `bunker` configuration path instead. The
successful run reported signing/restart, zero kind-1 publications, zero process
journal bytes, non-root/no effective capabilities/NoNewPrivileges, isolated
network and corrupted-credential rejection. CI results must be checked for the
exact candidate SHA; this local observation does not imply CI success.

The first CI attempt at `13be0888a2b5d1ad1ceb4f4827156773d48524a1`
([run 34727026940](https://github.com/lightning-goats/lightning-goats/actions/runs/34727026940),
job 103642842497) reported green despite a timeout on signing after restart.
Its new shell step lacked pipefail and `tee` masked the failing Python process,
leaving an empty JSON artifact. That run is not systemd acceptance evidence.
The correction uses strict Bash, direct file redirection and explicit JSON
validation. Signing readiness now has a 30-second bound with five-second
synthetic probes and recorded attempt counts; inactive/crashed/automatically
restarted services fail immediately. `Type=simple` startup alone is not signing
readiness. The precise cause of the original CI timeout remains unproven.
Evidence is emitted only after disposable service/relay cleanup succeeds.

## Remaining acceptance

The final production nak version and binary provenance must be reviewed and
installed separately. Repeat actual production credential isolation and sandbox
checks for the signer and validate the approved project identity and relay contract,
and obtain authorization before production-key operations or public publication.
Do not substitute this test's public keys or scalars into production settings.
Payment/feeder accounting, home containment and the parent acceptance gates
remain open.
