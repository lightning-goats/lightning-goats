# Home staging and final containment plans

**Proposed only; no host rules, routes or WireGuard peers changed.** Actual
2026-09-12 home inventory confirms UFW inactive, iptables-nft/Docker rules,
IPv4/IPv6 INPUT ACCEPT and Docker DNAT. One wg0 peer accepts 10.8.0.0/24 and
its existing IPv6 /64. Raw rules/topology remain private. A new narrow UFW
allow cannot establish containment and activating UFW would introduce a new
policy manager. Home canary stays 127.0.0.1:8790, never exposed by this plan.

PR #57 at `5cd7976f78e747eaf7bc3cc130199353a891191b` owns the existing
`deploy/nftables/home-wg-canary.nft.example` and namespace tests. Its full-wg0
restriction is a **final-policy candidate**, not a legacy-compatible staging
change. Do not copy or apply it here during parallel operation.

## Plan 1: staging while the old hub remains authoritative

The current completed stage is loopback only, with no connectivity claim.
Preferred bounded next candidate: a directly authenticated end-to-end WireGuard
peer between the home host and new VPS, reserving only staging 10.8.0.12/32 to
that new peer's public key on existing wg0. The original subnet remains
10.8.0.0/24; no private key is shared or reused. This requires confirmation of
the VPS public key, endpoint, actual AllowedIPs/routes and operator approval.
Do not infer that traffic relayed by the old hub authenticates the inner .12
source. The new specific peer mapping and direct handshake must be demonstrated.

Before binding canary to 10.8.0.6:8790, prepare/review a narrowly owned nftables
prerouting chain for packets from that authenticated /32: allow only destination
10.8.0.6 TCP8790; drop every other destination/port before DNAT and forwarding.
Do not grant that peer an IPv6 prefix or LAN routes. Preserve all other legacy
hub flows and existing manager state. Keep systemd destination/source allowances
aligned with the approved /32. The direct cryptographic peer is authentication;
source filtering alone is not. This is a design candidate, not installed rules.
If direct peer separation cannot be proven, retain loopback and use isolated
fixtures; adding a second tunnel without removing broad new-VPS paths is not
containment. An SSH administrative exception is not a runtime acceptance result.

Preflight: capture `wg show wg0 allowed-ips`, peer public keys/endpoints (never
private/dump output), `ip route get 10.8.0.12`, nft rules with handles, IPv4/IPv6
filter/NAT, Docker listener inventory, current gateway configs and independent
console access. Pin hashes. Application script must recompare those snapshots,
refuse drift/existing project tables, and make no blanket forwarding changes.
Test the exact candidate in a network namespace before presenting its apply
commands. Preserve table-specific rollback and a timer/console recovery path.

Rollback the staging candidate, once reviewed, by first stopping the new canary,
removing only its newly owned nft table and new peer mapping, and restoring the
exact previous specific route if one changed. Never flush rules or replace
wg0.conf wholesale. A fresh state/peer inventory is required before authoring
that apply script; public key and current VPS path are still pending.

## Plan 2: final home-enforced containment after approved cutover

Adopt/review PR #57's full wg0 prerouting restriction, including established
traffic, IPv6 and Docker DNAT/forwarded alternatives. The old full-subnet hub can
impersonate inner addresses, so a laptop source-IP exception is not independently
authenticated administration. Establish a tested local console or independent
end-to-end admin path before deleting broad hub access. Inventory legacy
LNbits/OpenHAB, MQTT, Hexmem and remote-admin callers; retire/migrate each under
the operator's cutover plan. This is explicitly incompatible with preserving
all current wg0 household access during staging.

Production port8789 is a later separately approved allowance after owner-finality
and physical acceptance. Coordinate endpoint authentication with the VPS client;
no bearer scheme or protocol switch is introduced by this home PR. Define one
manager for table persistence and verify ordering across Docker/network restarts.
Never use `nft flush ruleset`, `ufw reset`, global IPv6 disablement or blanket
subnet allowances. PR #57's rollback deletes only `inet lg_home_wg_canary` and
preserves other tables; on the actual host pin the table's content before removal.

Acceptance needs actual authenticated staging reachability, denied OpenHAB
8080/8443, weather5000, SSH/Postgres/unrelated services and routed/DNAT alternatives,
correlated listener/firewall counters, established-flow refusal, and independent
admin recovery. Namespace results in PR #57 do not certify the home path. No
cross-host packet tests, broad scans, spoofing or household policy changes were
performed by this assignment.
