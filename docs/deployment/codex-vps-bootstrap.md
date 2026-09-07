# Codex VPS Bootstrap Guide

## Purpose

This guide describes how to prepare the new Lightning Goats VPS so Codex can implement Phase 1 and configure the host without becoming the permanent production runtime identity.

## Account model

Use two separate Unix accounts.

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
- receives runtime secrets through systemd credentials or root-managed restricted files.

Do not install/run Codex under this identity.

## Initial root/bootstrap actions

As root or the provider-created admin account:

1. update the operating system;
2. install SSH, WireGuard, nginx, Git, build prerequisites, firewall tooling, and required system packages;
3. create `lg-deploy`;
4. add the operator-approved SSH key;
5. grant temporary sudo;
6. configure sensible SSH hardening;
7. do not install final Strike/OpenHAB/Nostr secrets yet.

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
docs/security/phase1-threat-model.md
docs/deployment/new-vps-staging.md
```

5. inspect Phase 1 tracker #6 and its child issues #7–#16;
6. work issue-by-issue, updating docs/tests as implementation evolves.

## Codex autonomy boundary

During staging Codex may, using temporary sudo:

- install build/runtime packages;
- configure the new VPS firewall;
- create/update nginx staging configuration;
- configure the new VPS WireGuard peer;
- create system users/directories;
- install staging/systemd unit files;
- build/install test binaries;
- run tests and canaries;
- modify this repository through normal reviewed Git workflow.

Codex must not autonomously:

- change production DNS;
- repoint existing production WireGuard clients from the old VPS;
- shut down/destroy the old VPS;
- reuse the old VPS WireGuard private key while the old host is online;
- grant Strike spend authority;
- release the real physical feeder override without an operator-directed test/cutover step;
- destroy historical LNbits/CLN recovery data.

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

## Transition from staging authority to production authority

Before final production credentials/cutover:

1. merge/checkout the accepted release/commit;
2. build/verify the exact intended artifact;
3. install the production binary as root-owned;
4. install root-owned configuration/unit files;
5. create/finalize runtime state directories;
6. revoke broad `lg-deploy` sudo;
7. review `authorized_keys`, groups, writable paths, and running processes;
8. confirm `lg-deploy` cannot read runtime credentials;
9. install or rotate final production credentials;
10. restart production services;
11. execute final staging verification;
12. stop for operator approval before production cutover.

## Temporary sudo policy

Broad sudo is acceptable only as a staging bootstrap convenience because the new host is not yet authoritative and should not yet hold valuable production credentials.

After provisioning, prefer removing broad sudo entirely. If long-term automated maintenance requires sudo later, create narrow command-specific policy only after the required operations are known.

Do not prematurely create a broad permanent automation sudo policy.

## Post-cutover Codex use

Codex may remain installed under `lg-deploy` for maintenance/development with no broad sudo and no access to production secrets.

Future production changes should follow a reviewed build/install/restart procedure rather than allowing the runtime service to self-modify.
