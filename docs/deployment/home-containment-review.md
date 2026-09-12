# Home WireGuard containment candidate — 2026-09-12

Production HOLD. No home rules were changed. The candidate is
[`home-wg-canary.nft.example`](../../deploy/nftables/home-wg-canary.nft.example).
It is not included in automatic installation.

## Observed baseline

The operator exported nft, iptables-save and ip6tables-save at 15:30:51 UTC.
Read through pinned SSH as `sat`: `/home/sat/lg-home-firewall-review.txt`,
18,053 bytes, SHA256
`9ed13766f9a731afdd5f383d682d0aa495478b8164d511146847c066eb634b0d`.
Raw export remains private. IPv4 and IPv6 INPUT policies accept. FORWARD drops
by default but permits Docker traffic; DOCKER-USER is empty. Docker DNAT exposes
53, 80 and 443 on both families, and additional IPv4 services. Thus neither the
forward default nor a new narrow allow establishes inbound containment.
Read-only listener inventory found no gateway on 8789 or 8790.

## Candidate behavior

Drop all traffic entering home `wg0` except IPv4 TCP from reserved staging
10.8.0.12 to home 10.8.0.6 port 8790, reserved for a harmless canary. Drop IPv6
and all forwarding from wg0, including established connections and Docker DNAT.
The prerouting priority precedes existing raw and destination NAT chains. An
accept in this candidate does not bypass later host policy; a drop is terminal.
Other interfaces remain governed by existing rules.

This intentionally removes wg0 SSH, Hexmem, generic OpenHAB, MQTT, weather,
container ports and apparent administrative-source exceptions. A compromised
hub can impersonate an inner .10 or .12 source. Therefore source address is only
a routing filter, never authentication. Canary activation must require an
independently authenticated end-to-end channel (reviewed mTLS or pinned TLS with
a scoped credential); plaintext HTTP, with or without a bearer token, is insufficient
against a hostile hub. No certificate or credential is supplied by this policy.

## Before any separately approved application

Inventory old-hub legitimate flows and review the impact of blocking all of them
except the canary. Establish a tested administrative/recovery path independent
of this hub, such as the local console. Preserve exact current rules and Docker
state. Verify the actual kernel accepts the candidate in an isolated network
namespace, then test broad INPUT ACCEPT, Docker DNAT, established traffic,
spoofed .10, IPv6 and forwarding. Rehearse rollback by deleting only table
`inet lg_home_wg_canary`; never flush the ruleset. Decide persistence ownership
and ordering across Docker/firewall restarts. Do not silently retain inspection
exceptions in the runtime policy.

## Isolated verification

`sudo unshare --net -- python3 deploy/nftables/test-home-canary.py` passed nine
raw-packet cases on disposable veth links with broad INPUT/FORWARD ACCEPT and a
Docker-like DNAT rule: gateway admission; SSH, OpenHAB, Docker port, spoofed .10,
other peer, TCP ACK and IPv6 refusal, plus forwarding refusal after DNAT of
the admitted gateway port. Kernel syntax validation also passed.
Both tests reject the host network namespace. The separate
`test-home-established.py` creates a second disposable namespace and real TCP
sockets. It establishes a forbidden connection before policy, proves that data
is blocked despite an existing established/related accept, then proves a fresh
gateway handshake and echo succeed. Removing only the candidate table restores
the forbidden connection while preserving the original table. These are local
virtual-link tests, not a real WireGuard or home-host observation. Home
reboot/restore, administrative recovery and old-hub impact still need acceptance.

F09 remains OPEN: candidate not applied, home runtime/reboot matrix incomplete,
end-to-end gateway authentication not deployed, old-hub flow compatibility and
home restore/reboot acceptance outstanding. No physical tests are authorized.

## Parallel home integration

Current main adds `home-gateway-agent-handoff.md` at
7a051cb; it assigns home deployment and owner finality coordination to the home
agent. PR57 supplies the existing source-derived candidate for adoption/review.
This full-interface policy is not a legacy-compatible staging policy: old-hub
flow inventory and the separate staging/final plans remain home-side work. The
current gateway has no application peer authentication; end-to-end protection
must be coordinated with its VPS client before network exposure.
