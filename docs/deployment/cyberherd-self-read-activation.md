# CyberHerd signed self-read configuration

Source `548276f0f68674073ec583ee7fcffc0237a4c2dc` passes exact CI/native
qualification and [actual backend deployment](../testing/cyberherd-self-read-backend-live-20261111.md).
The current member is configured; do not reapply its completed transaction. Public
ingress and the browser flag remain separate and disabled.

The authenticated `--cyberherd-self-read` tool adds only `[view.auth]` to the
existing enabled member/view configuration. It binds one canonical HTTPS origin
and an **empty** operator allowlist. The API can verify a signer's own status;
no signer can read operator state/templates or perform an operator command.
It cannot configure an administrator, public proxy, site flag or payout policy.

The closure now contains 22 files. Export the exact qualified commit as data,
independently authenticate its files and install a new root-owned generation.
Externally verify the launcher SHA256 before interpretation, then verify the full
manifest using isolated `/usr/bin/python3 -I -S -B`. Existing generations and
completed transactions remain retained. Never execute checkout code as root.

The launcher argument shapes are:

```text
--cyberherd-self-read prepare NEW_DIRECTORY --public-origin https://lightning-goats.com
--cyberherd-self-read apply DIRECTORY --review-sha256 PREPARED_SHA --exclusive-maintenance
--cyberherd-self-read resume DIRECTORY --approve-resumption
```

These are shapes for the authenticated launcher, not standalone shell commands.
Choose a fresh direct child of the protected upgrade base. The earlier private
`cyberherd-self-auth-review-124809f` configuration-check record is not an activation
transaction and cannot be reused or imported as one.

Preparation freezes the complete original installation and actual process/unit
identities. It refuses any existing auth table and constructs the candidate with
only the new origin/empty allowlist; all other parsed fields must match exactly.
The installed member program validates it under the original membership UID/GID
with groups/capabilities removed and an isolated network namespace. Only `--check`
runs: no journal, credentials, service or network client opens. Exactly one fixed
configuration replacement is sealed, retaining its original metadata/attributes.

Apply requires the independently reviewed preparation digest and unchanged
installation. The shared exclusive lock fences all four writers. The retained
native state tool snapshots and verifies a **current** four-journal backup before
the single member configuration changes. Every stopped live main/WAL/SHM image
must remain byte-identical. The exact installed configuration is checked again
using the original member UID. Core configuration/private contact, every program,
unit, identity, financial ceiling and outgoing mask remain unchanged.

Resume separately rechecks the backup, paid images, configuration/program/unit
bindings and native validation. It restores the original core unit and removes
only its own member mask, then starts the existing core and member once with
health/executable checks. Outgoing roles stay masked. It does not migrate, restore,
replace financial state, clear an unknown feeder UUID or retry a partial phase.
Any failed/ambiguous transaction remains stopped for inspection.

Public self-read ingress and the browser auth flag are separate changes. A public
deployment should expose only the exact `/api/v1/cyberherd/me` GET path with its
method/body/header/rate/no-store/private-log controls. It must preserve existing
payment/contact flags and all OBS/artwork. Operator ingress remains closed until
the actual operator's public-key allowlist is selected and qualified separately.

Protected-file fixtures verify scope/identity refusal, exact replacement and
native-check-before-maintenance, backup refusal, wrong-target refusal and stopped
financial/backup drift before restart. Native/systemd qualification must also
execute the actual backup/apply/resume and genuinely signed self reads with zero
operator authority, unchanged core credit/unknown UUID and zero gateway POSTs.
Mocked file tests alone do not establish this installed result. Source and actual
deployment acceptance are recorded in the
[operator checkpoint](../testing/cyberherd-installed-operator-checkpoint-20261111.md).

## Public self-status website

The separate `--cyberherd-self-read-site` launcher mode prepares an immutable
clone of the current public release and switches only nginx after review. This
extends the authenticated tool closure to **23 files**. Export the exact qualified
commit using `export-upgrade-tools.py` and `export-self-read-site.py` as an
unprivileged account. Independently authenticate the exported source and manifest
before installing a fresh root-owned generation; do not run checkout code as root.

The launcher argument shapes are:

```text
--cyberherd-self-read-site prepare PAYLOAD NEW_PRIVATE_DIRECTORY --source-manifest-sha256 SOURCE_SHA
--cyberherd-self-read-site apply DIRECTORY --review-sha256 PREPARED_SHA --activate-self-read-site
```

Preparation requires the already configured loopback self-read backend and empty
operator allowlist. It checks the four retained browser/login/crypto files against
the qualified source, clones every current asset with its serving metadata and
SELinux label, replaces the auth client's read-error wording and changes only
`cyberherdAuthEnabled` to true. All other flag values, contact/payment files, OBS
and uploaded artwork remain exact. The default-off source site configuration is
never substituted for the live flags.

The single fixed nginx replacement changes the static root and adds the exact
`/api/v1/cyberherd/me` GET proxy. Existing no-store site-config and private-log/WAF
policies remain. Other methods, oversized bodies, operator paths and unrecognized
paths stay closed. Native signature, Origin, nonce and query checks still apply.

Apply verifies the reviewed digest, all retained/clone fingerprints and unchanged
application identity/configuration, then validates nginx before and after the
single replacement and reloads only nginx. Failed validation restores only the
old nginx site; a failed reload also attempts to reload that restored site.
Applications, journals, credentials and financial policy are never replaced or
restarted. Partial attempts remain recorded for inspection rather than automatic
reapplication.

Qualification must distinguish protected file transactions with mocked readiness,
actual TLS/nginx/native signatures in the disposable installed rehearsal, isolated
WAF/browser tests, and actual public read-only acceptance. Initial local source
results are recorded in the operator checkpoint; public activation remains a
separate operation until the exact CI source and installed artifact pass.
