# Codex VPS Bootstrap Guide

## Purpose

This guide describes how to prepare the new Lightning Goats VPS so Codex can implement Phase 1 and configure the host without becoming the permanent production runtime identity.

## Account model

Use two separate Unix accounts on the VPS.

### Deployment/Codex account

Suggested name:

```text
lg-deploy
```

Characteristics during staging:

- interactive login allowed;
- owns the working Git checkout;
- runs Codex and Rust tooling;
- temporary sudo allowed for provisioning;
- does **not** receive final production secrets until privilege is reduced.

### Runtime account

Suggested name:

```text
lightning-goats
```

Characteristics:

- no sudo;
- no interactive login shell if practical;
- runs `lightning-goatsd` only via a system-level systemd unit;
- owns/writes only required runtime state;
- receives runtime secrets through systemd credentials or root-managed restricted files;
- does **not** receive an OpenHAB API token.

Do not install/run Codex under this identity.

The in-house feeder gateway uses a separate non-admin service identity on the trusted host. Its dedicated OpenHAB token never belongs on the VPS.

## Initial root/bootstrap actions

As root or the provider-created admin account:

1. update the operating system;
2. install SSH, WireGuard, nginx, Git, build prerequisites, firewall tooling, and required system packages;
3. create `lg-deploy`;
4. add the operator-approved SSH key;
5. grant temporary sudo;
6. configure sensible SSH hardening;
7. do not install final Strike/NIP-46 production secrets yet;
8. do not install/copy any OpenHAB API token to the VPS.

Example account intent, not a copy/paste requirement across distributions:

```text
lg-deploy       interactive + temporary sudo
lightning-goats system runtime + no sudo/no login
```

## Codex startup instructions

Under `lg-deploy`:

1. install Codex using the currently supported OpenAI installation method;
2. authenticate through the operator-approved method;
3. clone:

```text
https://github.com/lightning-goats/lightning-goats
```

4. before changing anything, Codex must read:

```text
AGENTS.md
docs/README.md
docs/planning/phase1-execution-plan.md
docs/architecture/phase1-strike-architecture.md
docs/architecture/lightning-address-registry.md
docs/security/phase1-threat-model.md
docs/security/openhab-feeder-gateway.md
docs/security/phase1-hardening-checklist.md
docs/deployment/new-vps-staging.md
docs/testing/phase1-verification-matrix.md
```

5. inspect Phase 1 tracker #6 and its child issues, including #17–#20 security-boundary work;
6. work issue-by-issue, updating docs/tests as implementation evolves.

## Codex autonomy boundary

During staging Codex may, using temporary sudo:

- install build/runtime packages;
- configure the new VPS firewall;
- create/update nginx staging configuration;
- configure the new VPS WireGuard peer/application tunnel;
- create system users/directories;
- install staging/systemd unit files;
- build/install test binaries;
- run tests and canaries;
- create reviewed scripts/configuration for the trusted-side feeder gateway/UFW setup;
- modify this repository through normal reviewed Git workflow.

Codex must not autonomously:

- change production DNS;
- repoint existing production WireGuard clients from the old VPS;
- shut down/destroy the old VPS;
- reuse the old VPS WireGuard private key while the old host is online;
- grant Strike spend authority;
- copy an OpenHAB API token to the VPS;
- bypass the feeder gateway with direct OpenHAB access;
- release the real physical feeder safety gates without an operator-directed test/cutover step;
- change the operator-defined maximum online Strike balance/sweep policy;
- destroy historical LNbits/CLN recovery data.

## Privileged changes must be reproducible

Temporary sudo means Codex can alter the host deeply. Revoking sudo later does not erase those changes.

Therefore capture privileged deployment state in reviewed repo artifacts wherever practical:

- package/bootstrap scripts;
- nginx configuration;
- systemd units;
- firewall rules/runbooks;
- WireGuard configuration templates/runbooks (never private keys);
- filesystem ownership/installation scripts;
- feeder-gateway service/unit/config templates;
- OpenHAB Item/rule definitions or precise operator instructions.

For maximum assurance the operator may choose to reimage/rebuild the final VPS from those reviewed artifacts before installing final production secrets.

## Git workflow

Prefer focused branches/PRs corresponding to the Phase 1 GitHub issues.

Each PR should:

- reference the issue it implements;
- include tests for changed invariants;
- update architecture/deployment docs when contracts change;
- pass locked formatting/lint/test gates;
- avoid mixing unrelated server changes and application changes unless the issue explicitly requires both.

## Production service installation

Production application services should be **system-level systemd services**, not per-user services.

Reason:

- root manages the unit, credentials, binary, and policy;
- the process still runs unprivileged using `User=lightning-goats`;
- system-level services support the existing strong sandboxing model cleanly;
- the runtime account does not need login/session persistence or service-management authority.

Target pattern:

```ini
[Service]
User=lightning-goats
Group=lightning-goats
ExecStart=/usr/local/bin/lightning-goatsd --config /etc/lightning-goats/config.toml
```

with strong `Protect*`, `NoNewPrivileges`, capability, filesystem, and credential restrictions as compatible with the final implementation.

nginx and WireGuard also remain normal system services.

The trusted-side feeder gateway likewise uses a system-level unit under its own unprivileged identity.

## Filesystem ownership model

Recommended intent:

```text
/usr/local/bin/lightning-goatsd        root:root, not runtime-writable
/etc/lightning-goats/                  root-managed configuration
/var/lib/lightning-goats/              runtime state, lightning-goats writable
/run/lightning-goats/                  runtime transient state
working git checkout                    lg-deploy owned
systemd unit files                      root owned
production credentials                  root/systemd managed
```

Do not run the production binary from the mutable Codex working tree.

Record the exact deployed source commit/tag and binary SHA-256.

## Transition from staging authority to production authority

Before final production credentials/cutover:

1. merge/checkout the accepted release/commit;
2. build/verify the exact intended artifact;
3. install the production binary as root-owned;
4. install root-owned configuration/unit files;
5. create/finalize runtime state directories;
6. configure/review WireGuard, nginx, firewall and feeder-gateway boundary;
7. revoke/narrow broad `lg-deploy` sudo;
8. review `authorized_keys`, sudoers, groups, writable paths, and running processes;
9. confirm `lg-deploy` cannot read runtime credentials or alter installed binary/config;
10. optionally reimage/rebuild from reviewed deployment artifacts if the operator chooses maximum assurance;
11. install or rotate final **VPS** production credentials (receive-only Strike, webhook secret, NIP-46; no OpenHAB token);
12. install/rotate the dedicated OpenHAB integration token only on the trusted feeder-gateway host;
13. remove/revoke any temporary Strike webhook-management credential when no longer needed;
14. restart production/staging services;
15. execute `docs/testing/phase1-verification-matrix.md`;
16. stop for operator approval before production cutover.

## Temporary sudo policy

Broad sudo is acceptable only as a staging bootstrap convenience because the new host is not yet authoritative and should not yet hold valuable production credentials.

After provisioning, prefer removing broad sudo entirely. If long-term automated maintenance requires sudo later, create narrow command-specific policy only after the required operations are known.

Do not prematurely create a broad permanent automation sudo policy.

## SSH production posture

Before production acceptance:

- key-only authentication;
- password authentication disabled;
- direct root login disabled;
- stale keys/accounts removed;
- prefer restricting SSH to WireGuard/admin source addresses once reliable;
- keep Vultr console as emergency recovery.

## Post-cutover Codex use

Codex may remain installed under `lg-deploy` for maintenance/development with no broad sudo and no access to production secrets.

Future production changes should follow a reviewed build/install/restart procedure rather than allowing the runtime service to self-modify.
