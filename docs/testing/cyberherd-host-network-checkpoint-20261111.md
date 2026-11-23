# Actual host network checks — 2026-11-11

Credential-free probes now verify public IPv4/IPv6 TLS and private-network
restrictions on the actual VPS network. The three CyberHerd identities cannot
reach the known-reachable household gateway. The receive identity retains its
narrow gateway exception and refuses five other private TCP paths. All original
application files/PIDs, unit masks and the nftables ruleset remain unchanged.
This supplements the earlier [isolated OS probes](cyberherd-installed-qualification-checkpoint-20261110.md)
and [native recipient/DNS checks](cyberherd-recipient-checkpoint-20261110.md).

The upstream explanation was fetched again from its main branch. Git blob
`b770126657d348779ce1b68068319b79eef8e30e` and raw SHA256
`e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`
still match the full implementation plan's reference. The full goal is unchanged.

## Existing policy and passive kernel inspection

The host uses active/enabled firewalld plus dedicated nftables tables; UFW is not
installed. The iptables OUTPUT default alone does not describe this policy.
`firewall-cmd --check-config` succeeds. Both dedicated receive/display firewall
units are active/enabled. Existing persistent peer-forwarding/DNS and VPN-egress
policies were inspected without reload, rule changes or functional peer tests.
No WireGuard interface, key, peer, route or DNS record changed.

The receive UID's loaded nftables policy permits the household gateway's TCP
port 8789, required DNS and established loopback service replies, then rejects
private destinations. This is a UID-scoped OUTPUT policy, separate from peer
forwarding. The membership unit uses systemd IP filters with exactly:

```text
10.0.0.0/8
172.16.0.0/12
192.168.0.0/16
169.254.0.0/16
fc00::/7
fe80::/10
```

The running member has both `sd_fw_ingress` and `sd_fw_egress` attached to its
actual cgroup. Both reference the same two LPM maps. Independent decoding of the
actual map keys finds exactly the six ranges above, with no added prefix or
allow exception. Its zero capabilities and NoNewPrivileges configuration remain.
The masked outgoing units' empty effective systemctl properties were not mistaken
for an active permissive policy: their retained protected unit files supplied the
configured probe ranges, and their actual units were never unmasked or started.

Only passive bpftool operations were used: `cgroup show ... effective`,
`prog show`, `map show` and `map dump`. Their read-only meaning is documented by
[upstream bpftool](https://github.com/libbpf/bpftool/blob/main/docs/bpftool-cgroup.rst).
No attach/detach, map update or firewall tracing rule was used. The reviewed signed
Fedora transaction installed only `bpftool-7.6.0-3.fc44`,
`llvm21-filesystem-21.1.8-5.fc44` and `llvm21-libs-21.1.8-5.fc44`:

```sh
sudo dnf -y --setopt=install_weak_deps=False install bpftool
```

It installed three packages, with no upgrades/removals or application restart.
This is an inspection dependency, not a new application/runtime dependency.

## Actual host probes

Ordinary host positive controls first establish verified TLS to the configured
Strike hostname using current all-public IPv4 and IPv6 answers. Only the TLS
handshake is sent: no HTTP request, API authorization or payment operation.
A TCP-only positive control establishes that the household gateway is reachable.

Three uniquely named transient systemd units run Python probes under the existing
`cyberherd`, `cyberherd-payout` and `cyberherd-signer` UIDs/GIDs/groups. They use
the original six deny ranges on the **host** network namespace. They have zero
capabilities, NoNewPrivileges, read-only system/home protections, namespace/device
restrictions, no loaded systemd credential, and inaccessible production
configuration, journals, IPC and credential directories. Memory/task limits and
short runtime/stop deadlines bound each probe; no application binary runs.

Every probe positively verifies its actual identity, zero capabilities, host
namespace and hidden production paths. Certificate-verified TLS succeeds to the
same selected public IPv4 and IPv6 addresses. Connecting to the private gateway
fails by permission refusal or timeout; generic DNS, TLS or connection errors
cannot substitute for the required public success. Ordinary host gateway positive
controls still succeed after each role, and original application identities are
rechecked after each probe. Thus the denied target is known reachable from this
host outside the tested policy.

A fourth transient process uses the receive UID and its existing global nftables
policy, without adding a systemd private-address filter. Both public TLS families
and gateway TCP access succeed. Generic OpenHAB, legacy weather, household SSH,
another private gateway address and loopback SQL attempts each receive connection
refusal. This is evidence of the observed attempts and the inspected explicit
UID policy; it does not assert that every refused remote service was listening.
No HTTP payload or feeder UUID is sent to any private endpoint.

All four transient units are collected afterward. Independent verification finds
none remaining, all 17 original managed files/metadata exact, original core/member
PIDs and outgoing masks retained, and the complete before/after nftables JSON
ruleset identical. No paid journal, provider API, relay or feeder application
request is used by these checks.

## Evidence and remaining limits

Private evidence is retained under
`/var/lib/lightning-goats-upgrades/cyberherd-host-network-20261111-a`:
exact probe source/configuration, temporary-unit properties/original-unit hashes,
stdout/status, before/after firewall policy and live BPF program/map evidence.
These records contain private host inventory and stay out of public artifacts.
No writable checkout code ran as root; root orchestration used fixed standard
library/system utilities, and probes executed only as the non-admin users.

| Receipt | SHA256 |
| --- | --- |
| Three CyberHerd role probes | `5788fd1c4f44f30c9142b1f72080a8d70e6be015fd9ed65d1f24bb344afe2908` |
| Receive UID probe | `93f4ea202a6f93075a72178b8258fe8479bce2e3c3175df8bf337083261ffa44` |
| Independent verification | `fea92e7c932cee867f71110f23659e971e93c3a1d4075c814fc0f6ad89d1875f` |

This establishes the stated actual-host policy observations and public IPv4/IPv6
egress. It does not prove public inbound IPv6, functional forwarding from another
WireGuard peer, all public recipients, native payout/provider operations, an
actual private IPv6 destination on this network, or recovery adoption. The exact
live IPv6 deny-map contents supplement the earlier positive-control isolated
IPv6 tests; no live private IPv6 target was fabricated or routed for this check.

No new financial limit, signer, publication path, operator permission or physical
feeding is activated. The full twelve-part goal and draft PR remain incomplete.
