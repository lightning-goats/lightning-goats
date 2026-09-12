# Direct-peer authentication regression

Status: isolated lab evidence only. Production HOLD. This experiment supports
review of the staging design in PR #59; it does not install that design or
certify a household network. The existing owner and home containment candidates
remain under PR #57 and the home agent's review.

The old hub's broad peer mapping authenticates the hub, including inner source
addresses it may forward or originate. A staging source-address firewall rule
therefore needs an independently authenticated peer binding. WireGuard associates
each peer key with permitted tunnel addresses and checks decrypted source
addresses before delivering packets to the interface. See the official
[cryptokey routing description](https://www.wireguard.com/#cryptokey-routing).

## Reproduce without a real network

From a Linux host with root, network namespaces, kernel WireGuard, `ip`, `wg`,
`nft`, `ping` and Python 3:

```sh
sudo python3 deploy/scripts/verify-wireguard-peer-isolation.py
```

The script creates three isolated namespaces and connects their underlay using
veth pairs created inside those namespaces. It uses only synthetic keys, the
documentation underlay `192.0.2.0/24` and synthetic tunnel `10.77.0.0/24`. It never
opens existing private-key files, attaches to host interfaces, adds host routes,
or contacts a real peer. Temporary key files are inside a private temporary
directory. The script deletes its own namespaces and temporary files on normal
completion or an exception. Abrupt termination such as SIGKILL can leave its
uniquely named `lgck<PID>-*` namespaces and temporary directory for manual cleanup.

The simulated home peer initially accepts the hub's broad `/24`. After a
negative control, it adds a separate staging peer with the more-specific `/32`.
An nftables counter at home INPUT counts requests claiming the staging source.
This distinguishes rejection before host delivery from a missing reply caused
only by changed return routing. The INPUT chain itself accepts traffic.

## Observed result

All six cases passed on Linux `7.2.4-200.fc44.x86_64` on 2026-09-12, using the
script in this change. No production credential or physical device was used.

| Case | Ping reply | Staging-source requests delivered to home INPUT |
| --- | --- | --- |
| Legitimate old hub before change | Yes | 0 |
| Hub spoofs staging source before specific peer | Yes | 1 |
| Hub spoofs staging source after specific peer | No | 0 |
| Direct authenticated staging peer | Yes | 1 |
| Legitimate old hub after change | Yes | 0 |
| Hub spoof after removing specific peer | Yes | 1 |

The final case is an intentional negative control: deleting the specific peer
restores the broad hub's ability to use the staging source. Stop the canary and
close its exposure before removing the authentication binding during rollback.

## Evidence still required before live staging

- Pin both real public keys, endpoint reachability, peer mappings and routes.
  Replace the destination's old-hub mapping at the VPS with the direct home
  mapping; demonstrate a fresh direct authenticated handshake on both ends.
- Review and test the exact home firewall candidate before DNAT and forwarding,
  including established flows, prohibited services, IPv6 and alternate paths.
  This experiment tests peer authentication, not the firewall candidate.
- Remove or separately contain temporary inspection/administrative exceptions
  from the payment runtime's path. Keep the coordination/recovery plan usable
  when those exceptions close; do not silently widen the gateway-only policy.
- Verify service namespace and systemd routing, then measure real gateway
  listener/firewall counters during positive and negative tests.
- Obtain approval for the concrete live peer, route, firewall and service change
  after reviewing drift checks, persistence and narrowly scoped rollback.

This preserves the distinction between staging that retains other hub traffic
and final containment that intentionally retires broad legacy access. Lab
success alone authorizes neither network changes nor payments or feeding.
