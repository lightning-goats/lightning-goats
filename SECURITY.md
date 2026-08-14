# Security Policy

## Supported versions

Lightning Goats is currently a single-operator Phase 1 deployment. Security fixes are applied to:

- the current `main` branch; and
- the most recent tagged production release.

Older development builds are not supported.

## Reporting a vulnerability

Please do **not** open a public issue for a suspected vulnerability.

Use GitHub Private Vulnerability Reporting for this repository when available. Include:

- affected commit/tag;
- affected component (`lightning-goatsd`, `lightning-goatsctl`, deployment assets, overlay protocol, CLN/OpenHAB/Nostr integration);
- reproduction steps or a proof of concept;
- security impact;
- any suggested mitigation.

## Security boundaries

Phase 1 is intentionally least-privilege:

- `lightning-goatsd` observes Core Lightning through a restricted CLNRest rune and has no Lightning spend authority;
- the daemon must not receive the unrestricted `lightning-rpc` socket;
- project Nostr signing is delegated to a NIP-46 bunker;
- canary mode receives no NIP-46 client credential and must point only at a harmless OpenHAB test rule;
- the public HTTP/WebSocket API is read-only;
- manual feeder activation remains outside the application in OpenHAB;
- LNbits is not a runtime or accounting dependency after cutover;
- all public Internet traffic terminates at nginx; application listeners are loopback-only.

A report that demonstrates a path across one of these boundaries is considered high priority.
