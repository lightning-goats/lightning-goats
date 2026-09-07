# WireGuard Topology and VPS Migration

Status: canonical Phase 1 network plan.

## Existing production network

Lightning Goats currently uses the established WireGuard subnet:

```text
10.8.0.0/24
```

Known addresses relevant to Phase 1:

```text
10.8.0.1   existing production VPS / WireGuard hub
10.8.0.6   in-house OpenHAB + weather host
```

Do not invent a replacement application subnet unless a later reviewed design explicitly requires one. Phase 1 should reuse this network and harden access with peer/host firewall policy and the narrow in-house integration gateway.

## Staging topology

During parallel staging:

```text
                    existing production hub
                          10.8.0.1
                              |
             +----------------+----------------+
             |                                 |
      existing home peers               new VPS staging peer
                                                10.8.0.X
                                                new keypair
```

`10.8.0.X` means an unused address selected only after inventorying the current WireGuard peer/address assignments. Do not hardcode an example address before checking the live network.

Requirements:

- old VPS remains `10.8.0.1` and production-authoritative;
- new VPS uses its own new WireGuard private/public keypair;
- new VPS uses one unused temporary `10.8.0.x` address;
- existing clients continue using the old VPS as their hub;
- production DNS remains unchanged;
- trusted-side firewall rules allow the staging VPS only the explicitly approved integration-gateway port on `10.8.0.6`;
- direct staging-VPS access to `10.8.0.6:5000` weather service, OpenHAB REST, SSH, and unrelated hosts/ports must remain blocked.

Before selecting the staging IP, inventory at least:

```sh
wg show
ip -br address
```

and the active WireGuard configuration/peer assignments on the old hub and relevant home peers.

## In-house integration host

The OpenHAB/weather server is:

```text
10.8.0.6
```

Phase 1 should expose exactly one narrow Lightning Goats integration-gateway TCP port from this host to the VPS.

The gateway is responsible for:

- feeder override/status reads;
- feeder UUID request/ack protocol;
- optional temperature/status reads;
- sanitized read-only weather data for the overlay.

The VPS must not receive generic access to:

```text
10.8.0.6:5000   legacy weather Flask service
10.8.0.6:8080   OpenHAB REST/UI if using the standard port
10.8.0.6:22     SSH
```

unless a later explicit operator-approved exception is documented.

## Production cutover preference

To minimize changes to existing peer routing and firewall assumptions, preserve the established hub WireGuard address `10.8.0.1` after cutover.

Preferred sequence:

1. finish all staging tests with new VPS on temporary `10.8.0.X`;
2. freeze production side effects;
3. stop WireGuard on the old VPS and verify it is no longer active;
4. activate the reviewed production hub configuration on the new VPS using `10.8.0.1/24`;
5. update existing clients' hub peer configuration to the **new VPS public key** and **new public Internet endpoint**;
6. preserve client private keys and their existing `10.8.0.x` addresses unless a specific migration requires otherwise;
7. verify required peer handshakes and routing;
8. replace temporary staging firewall allowances with the final production rules for source `10.8.0.1` where applicable;
9. remove the temporary staging `10.8.0.X` assignment/rules.

The old and new VPS must never both claim `10.8.0.1` at the same time.

## Client peer cutover

A typical client-side hub peer transition is conceptually:

```ini
# before
[Peer]
PublicKey = <old-vps-public-key>
Endpoint = <old-vps-public-ip>:<wg-port>
AllowedIPs = <existing values>

# after
[Peer]
PublicKey = <new-vps-public-key>
Endpoint = <new-vps-public-ip>:<wg-port>
AllowedIPs = <preserve existing values unless explicitly reviewed>
```

Do not copy the old VPS private key merely to avoid updating peer public keys. The new hub should have a new identity.

## Routed traffic vs VPS-local traffic

The new VPS may act as a WireGuard router/hub for existing clients, but local application processes should not automatically gain broad trusted-network access.

Treat these as distinct policies:

```text
peer A -> hub -> peer B             routed client traffic
lightning-goatsd/nginx -> home      locally originated VPS traffic
```

Home-side firewall rules are required on sensitive peers even when hub routing is permitted. In particular, `10.8.0.6` must restrict locally originated production-hub traffic to the dedicated Lightning Goats gateway port.

Do not rely on WireGuard `AllowedIPs` alone as application authorization.

## Rollback

Before meaningful new-epoch payments, rollback may restore:

- client hub peer public key/endpoint to the old VPS;
- old VPS WireGuard `10.8.0.1` configuration;
- old DNS values as required.

Never have both hubs active as `10.8.0.1` simultaneously.

After the new Strike-backed system accepts payments, preserve its SQLite/event/feed-credit state before any network rollback. See `production-cutover.md`.
