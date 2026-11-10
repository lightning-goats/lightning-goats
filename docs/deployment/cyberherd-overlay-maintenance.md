# Explicit overlay-stage maintenance

This executor is a source candidate. No maintenance or activation has been
performed or approved for it. Its actual installed rehearsal remains required
before asking the operator to approve a concrete maintenance transaction.

`maintain-cyberherd-overlay.py` consumes an already sealed
[configuration review](cyberherd-overlay-stage.md) and its independently reviewed
`PREPARED.json` SHA256. A fresh protected execution plan belongs to its own
authenticated seventeen-file tool generation. The earlier prepared bundle,
sixteen-file tools, native checker and credential importer are retained unchanged.
Never interpret writable checkout code as root or rewrite an old preparation's
tool identity to make a new launcher accept it.

New preparation inventories protected role-specific unit overrides even while
the role is masked; exact files and effective membership overrides are checked
before service start. An older sealed review without that inventory is accepted
unchanged only when no role override exists. This permits a separately reviewed
namespace override in the disposable rehearsal without changing vendor unit
bytes or weakening the production invocation.

Ubuntu's systemd 255 reports `NeedDaemonReload=yes` for the masked intake with
its namespace override, despite manager reload. In this masked state only,
the executor accepts that flag when the exact protected role-specific overrides
match their independently frozen fingerprints. Missing/unpinned/changed files,
unknown flag values or a non-masked unit refuse. Other masked-state checks,
zero PIDs/cgroups and the UID scan remain mandatory. Loaded/running service
acceptance still requires a reload, the exact effective override set and
`NeedDaemonReload=no`. Default inactive-install/qualification callers without a
frozen override inventory keep their original strict behavior.

`plan` revalidates the original running core, fresh inactive roles, frozen review
and actual typed candidate configuration. It creates a new root-private execution
directory with its own two-configuration replacement transaction; it changes no
installed file or service and opens no journal. An occupied socket parent or
tmpfiles destination refuses planning.

```text
LAUNCHER --cyberherd-overlay-maintenance plan REVIEW EXECUTION --review-sha256 REVIEW_RECEIPT_SHA256
```

`LAUNCHER` means isolated system Python `-I -S -B`, the externally authenticated
root-owned launcher and its reviewed tool-manifest SHA256. Paths must be canonical
direct children of the protected upgrade base. Do not substitute unreviewed
manifests or pick a different key, provider scope, epoch or journal cutoff.

The following commands are **operator maintenance actions**, not authorization to
run them now. Exclusive administration/no-other-writers must cover the entire
interruption, including helpers outside systemd. A host-wide nonblocking executor
lock and private transaction lock prevent concurrent invocations of this tool;
they do not lock out administrators or unrelated programs.

```text
LAUNCHER --cyberherd-overlay-maintenance apply EXECUTION --exclusive-maintenance
LAUNCHER --cyberherd-overlay-maintenance resume EXECUTION --approve-resumption
```

Apply checks unchanged identities/programs/policy/units, then stops the core and
atomically replaces its exact retained custom `/etc` unit with a persistent mask.
A `/run` mask cannot override that custom unit. All four writers must be masked,
inactive/dead with zero PIDs and unpopulated cgroups; a separate UID-process scan
rejects other writers. Neither outgoing role is unmasked or started.

The installed native `cyberherd-statectl` takes and verifies a new four-journal
backup before any configuration replacement. Exact policy scope, image hashes and
original main/WAL/SHM fingerprints are retained. Configuration changes require
those originals to remain unchanged. A partial backup is retained and refused;
the tool never deletes/replaces it or restores a database to make progress.

The two-file transaction recognizes exact old/new fingerprints after an
interrupted rename. It changes only core and membership TOML. The separately
reviewed tmpfiles entry and core-owned 0750 bridge parent are installed without
exposing the core credential runtime. Apply leaves every writer masked/stopped.
No migration, financial grant, invoice or publication is performed by apply.

Resume independently verifies the accepted native backup and exact new
configuration, restores the candidate core unit, removes only the exact membership
mask, reloads the manager, starts the existing core, checks local health/status,
then starts membership and checks its loopback roster response. The core retains
its existing payment/feed/Nostr behavior; native incoming receipts become enabled.
Membership has no Nostr credential/signer, lifecycle, rewards, outgoing bridge or
operator HTTP routes. All financial ceilings remain zero. The two outgoing
services stay masked. Membership is started for this stage, not enabled for boot;
boot persistence is a later qualified rollout step.

An interruption after core start is reconciled using the same unit/configuration
and transaction. Starting an already running selected unit is idempotent; the tool
never replaces journal history or resets its epoch. Completed resumption refuses
another start request. Failed checks retain evidence and require inspection;
health failures are not claimed as acceptance.

Configuration-only recovery also requires explicit maintenance:

```text
LAUNCHER --cyberherd-overlay-maintenance rollback EXECUTION --exclusive-maintenance
LAUNCHER --cyberherd-overlay-maintenance resume-core EXECUTION --approve-resumption
```

Rollback stops intake and core, retains all four masks, restores only original
configuration files and removes only the unchanged staged tmpfiles entry. It
retains the bridge directory, paid state, feeder UUIDs, signed jobs, replay markers
and every new journal row created after stage start. No older backup is adopted.
The original unit is restored and original core started only by the separate
`resume-core` command. Intake remains masked. This recovery can also restore the
original configuration after a failed backup, since apply has not altered any
journal. It does not resolve an unknown financial/physical outcome.

The Python root fixtures use real disposable protected files and actual atomic
transactions, but mock service/native commands and runtime inspection. They are
not installed systemd or live backup evidence. The existing mandatory native
four-journal test separately covers a paid core without a bridge, fresh role
journals, committed WAL, a pending feed UUID and retained signed bytes. A complete
installed executor rehearsal and explicit live maintenance approval remain open.
The disposable systemd harness now exercises this complete executor with actual
native services and synthetic paid/pending/signed journals. Both services join
its private loopback-only namespace, all credentials are synthetic and the core
is shadow/paused. It checks native backup, stage view, configuration-only rollback,
preserved financial/pending/signed counts and original-core resumption, with zero
mock gateway POSTs. Its actual result must be inspected before claiming success.
Public nginx/OBS integration and actual receipt/membership delivery remain later
acceptance work; this executor changes no public proxy or browser files.
