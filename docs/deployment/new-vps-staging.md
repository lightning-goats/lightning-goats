# New VPS Staging Runbook

Status: pre-production procedure for parallel deployment.

Do not use this runbook to perform final DNS/WireGuard cutover. That is covered by `production-cutover.md` and requires explicit operator approval.

## Goal

Build and validate the replacement Lightning Goats stack on a new VPS while the existing VPS remains production-authoritative.

## 1. Provision the VPS

Create a small clean Vultr VPS in the desired region.

Initial host should contain only what is needed for staging:

- SSH;
- package updates;
- WireGuard;
- nginx;
- Rust/build tooling as required by the deploy account;
- Codex under the deploy account;
- repository checkout.

Do not install LNbits, Core Lightning, CLNRest, `clnaddress`, or PostgreSQL for LNbits.

## 2. Create account boundaries

Create a deployment account, e.g.:

```text
lg-deploy
```

It may have temporary sudo during staging.

Later create a separate runtime account:

```text
lightning-goats
```

The runtime account should not have sudo and preferably should not allow interactive login.

Do not install Codex under the runtime account.

## 3. Install Codex and clone the repository

As the deploy account:

- install Codex using the current supported method;
- authenticate it using the operator-approved account flow;
- clone `lightning-goats/lightning-goats`;
- read `AGENTS.md` first;
- read the Phase 1 tracker (#6) and child issues;
- read `docs/planning/phase1-execution-plan.md` and the architecture/security docs before making changes.

Codex should work issue-by-issue and keep tests/docs updated with each behavioral change.

## 4. Add the new VPS to WireGuard as a new peer

Generate a new keypair for the new VPS.

Do not copy/reuse the old VPS private key while both systems are online.

Add the new VPS to the existing WireGuard network with a staging/new peer identity and suitable WireGuard IP.

Verify basic connectivity to explicitly required internal services only.

The production clients continue pointing at the old VPS during this stage.

## 5. Apply trusted-side firewall restrictions

At the home/trusted WireGuard boundary, permit the new VPS peer to reach only Phase 1-required destinations.

Expected minimum is normally:

```text
new VPS -> OpenHAB host:OpenHAB port
```

and any explicitly approved status/weather endpoint if separate.

Verify from the new VPS that unrelated internal hosts/ports are not reachable.

If the VPS will later function as a WireGuard hub for existing clients, prepare forwarding/routing rules separately from local-origin access policy.

## 6. Stage nginx and static site

Serve the Lightning Goats static site locally on the new VPS.

Prepare nginx routes for:

- static `lightning-goats.com` content;
- Lightning Address discovery;
- LNURL-pay callback;
- Strike webhook;
- `/healthz`;
- `/api/v1/status`;
- overlay WebSocket;
- any explicitly required canary paths.

Do not change production DNS yet.

Use a temporary hostname, hosts-file override, direct IP/SNI test method, or other controlled staging route.

## 7. Implement Phase 1 issues

Use the recommended ordering in `docs/planning/phase1-execution-plan.md`.

Codex must not reintroduce LNbits/CLN as shortcuts.

Run the locked Rust gates after each meaningful implementation slice.

## 8. Prepare system-level production units

Use root-managed systemd units under `/etc/systemd/system/` rather than user-level units for production.

The unit should run the process as the `lightning-goats` runtime user and retain strong sandboxing.

Production binary/config should be root-owned and not writable by the runtime user.

Runtime state should be writable only where required.

## 9. Staging secrets

Use test/staging credentials where possible.

Do not grant Strike spend authority.

Do not install final production secrets while the deploy account still has unnecessary broad sudo.

Recommended sequence:

1. finish host configuration;
2. test using non-production/mock credentials where practical;
3. create/finalize system services;
4. revoke broad deploy sudo;
5. audit permissions;
6. install/rotate final receive-only Strike, webhook, OpenHAB, and NIP-46 credentials;
7. run final staging verification.

## 10. Verification before canary acceptance

Required checks include:

- nginx configuration test;
- TLS/staging endpoint behavior;
- WireGuard connectivity and negative reachability tests;
- Lightning Address discovery/callback;
- real tiny Strike invoice creation/payment;
- webhook verification and exactly-once credit;
- Nostr publication;
- overlay display and reconnect/replay;
- overlay-only informational messages do not publish to Nostr;
- harmless OpenHAB canary rule;
- one operator-approved controlled physical feeder test after canary success.

Use issue #15 as the authoritative verification matrix.

## 11. Staging completion gate

Staging is accepted only when:

- all Phase 1 code gates are green;
- the new VPS passes the end-to-end matrix;
- production runtime account/system service is in place;
- broad temporary deploy sudo has been revoked or narrowed;
- production secret/access review is complete;
- production DNS is still unchanged;
- old VPS remains available as rollback/archive.

At that point stop and obtain explicit operator approval before executing `docs/deployment/production-cutover.md`.
