# Direct-peer home staging candidate

**Review only. Production HOLD. No peer, route, firewall or network listener has
been changed.** This session-only candidate preserves legacy hub access while
constraining the independently authenticated staging peer. It is not final home
containment or permission to run payments/physical feeding. PR57's existing-owner
and full-wg0 policy work remains separate; PR62 provides complementary WireGuard
peer-authentication evidence at `dbdd1a0136a4e2bf05a458e5adb439fb6ce5c50f`.

## Private peer agreement and persistence ownership

Public peer keys, current endpoints and inventory belong in private Hexmem
`lightning-goats-coordination`, not public issue comments or this repository.
Home supplied its current public key in facts1860. The new VPS public UDP
endpoint, reciprocal home peer mapping and VPS persistence owner still need a
versioned ACK. Home proposes initiating UDP directly to that endpoint with a
25-second persistent keepalive; the VPS learns the home's translated endpoint.
Neither host needs the other's private key. The old hub must not relay this
handshake as the authority for an inner source address.

Home reserves only `10.8.0.12/32` to the new VPS public key on existing wg0. VPS
must reserve home `10.8.0.6/32` to the independently pinned home public key and
remove that /32 from its old-hub peer. No new IPv6 AllowedIPs or LAN/default route
is granted. The existing home route already selects wg0; no route change is
planned. If current inventory differs, stop and revise this candidate.

**Persistence owner:** home operator owns the home session artifacts; VPS agent
owns its reciprocal peer and output filter. Neither agent enables automatic
network-canary startup. This first candidate deliberately makes no persistent
WireGuard, nftables-manager or /etc canary-config edit. All project additions
are volatile and the canary remains disabled at boot. Persistent deployment is
a later reviewed change with restart tests and a single declared policy owner.
The home unit binds its lifetime to wg-quick during this session, so stopping the
manager also stops the exposed canary. Administrative `wg set`/peer edits must
be excluded throughout the test; before removing a peer, close the listener.

The VPS currently has SSH/Hexmem inspection exceptions. Closing them ends that
inspection path. Both agents must checkpoint, establish independent recovery and
agree on the transition before application; these exceptions cannot survive in
a claimed gateway-only acceptance. Do not use this plan to lock out the operator.

## Candidate and isolated evidence

`deploy/nftables/home-staging-peer.nft.example` adds only the table
`inet lg_home_staging_peer`. Its pre-DNAT hook restricts authenticated source .12
to destination .6 TCP8790. Additional input and forward hooks prevent later DNAT
of the allowed port to another local or container service. Other IPv4 sources
and legacy IPv6 retain their existing policy. IPv6 exclusion for the staging
peer depends on its empty IPv6 AllowedIPs, independently verified on both hosts.
A broad hub can spoof .12 **until** the direct /32 peer mapping exists; deleting
that mapping reopens spoofing. The table alone is not authentication.

Run only in a disposable network namespace:

```sh
sudo unshare --net -- python3 -B deploy/nftables/test-home-staging.py
python3 -B -m unittest discover -s deploy/tests -p test_home_staging_drift.py -v
```

Observed locally: 11 packet cases and table-specific rollback pass. Tests count
both drops and packets reaching input, preserve legacy IPv4/IPv6, reject SSH,
OpenHAB, pre-DNAT alternatives, ACK-flag traffic to a denied service, forwarded
allowed-port DNAT, an alternate routed destination and local allowed-port DNAT.
This is packet-policy evidence; it does not claim a real established TCP session
or real home/VPS reachability. PR62 separately proves six cryptographic peer
controls, including the unsafe peer-removal rollback order. Its results are not
reimplemented here. CI runs this candidate in a separate namespace as well.

## Exact staged application sequence, after specific approval

These are reviewable commands and guards, not an instruction to execute them now.
Use an independent local console for application and recovery. The approved
private manifest must pin the tested source commit and filter hash, both public
keys, direct VPS endpoint, peer/AllowedIPs readbacks, original route, and pre/post
private snapshots. Do not source untrusted shell files or use `wg ... dump`.

1. Checkpoint both agents. Return only the canary remote switch OFF and stop only
   `lightning-goats-gateway-canary.service`. Verify inactive and disabled, no
   TCP8790 listener, production inactive, and no other changes in progress.
   Preserve all SQLite files. The network application never sends an Item or
   gateway command; switch state is verified by the existing home safe workflow.
2. Create a fresh root-owned 0700 evidence directory under `/run` using an
   exclusive `mkdir`. Capture the private baseline with
   `sudo python3 -B deploy/scripts/inspect-home-staging-drift.py --snapshot /run/<review>/before.json`.
   The helper refuses unsafe/existing evidence files and captures config hashes,
   peer routing, policy routes, nft rules and manager/service state. It does not
   copy private keys or tokens. Confirm wg-quick is the actual active home
   manager and the public-key/AllowedIPs/route match the agreed private manifest.
3. On the console, immediately before mutation, run
   `sudo python3 -B deploy/scripts/inspect-home-staging-drift.py --check /run/<review>/before.json`.
   Require exit zero. Ticking packet counters and IPv6 route lifetimes are
   normalized; rule handles, actions, addresses, next hops and route presence
   remain compared. Any drift means a fresh review, not an automatic overwrite.
4. Require `inet lg_home_staging_peer` absent, new peer public key absent and no
   existing specific .12 mapping. Require the exact reviewed nft file checksum
   and `sudo nft --check --file deploy/nftables/home-staging-peer.nft.example`
   success. Then `sudo nft --file deploy/nftables/home-staging-peer.nft.example`.
   It only adds that new table; it never flushes or replaces another table.
5. After the VPS reciprocal change is ready under its own approval, run the
   following with values from the approved private manifest (public values only):

   ```sh
   sudo wg set wg0 peer "$LG_REVIEWED_VPS_PUBLIC_KEY" \
     allowed-ips 10.8.0.12/32 endpoint "$LG_REVIEWED_VPS_UDP_ENDPOINT" \
     persistent-keepalive 25
   ```

   Do not run `wg syncconf`, change the old hub peer, edit wg0.conf, restart wg0,
   or add routes. Verify exact public-key/AllowedIPs mapping on both ends, a
   fresh direct handshake and expected route. If any step fails, keep canary
   stopped and the restrictive table in place pending bounded rollback.
6. Capture `/run/<review>/network-installed.json` using `--snapshot`; compare it
   with `--check` before exposure. Inspect the exact table against the reviewed
   candidate. On the VPS verify the gateway-only output filter with inspection
   exceptions removed; home SSH/Hexmem loss is expected only after checkpoint.
   No network acceptance claim is possible while those exceptions remain.
7. Prepare a **volatile** canary config under a fresh root-owned 0755
   `/run/lg-home-staging/`, mode0644, derived from the canonical installed config
   with only `service.listen` changed to `10.8.0.6:8790`. Keep the existing canary
   DB, USER credential, UUID protocol, caps and isolated Items. Compare parsed
   configs and require exactly that one difference. Do not run the loopback
   checker against this noncanonical configuration; it intentionally refuses it.
8. Only after the network gates above, create a fresh root-owned runtime drop-in
   `/run/systemd/system/lightning-goats-gateway-canary.service.d/staging.conf`
   with this exact content. Refuse any existing drop-in directory. The root-owned
   `PEER-VERIFIED` marker is written only after step6 and removed on rollback.

   ```ini
   [Unit]
   BindsTo=wg-quick@wg0.service
   After=wg-quick@wg0.service
   ConditionPathExists=/run/lg-home-staging/PEER-VERIFIED

   [Service]
   ExecStart=
   ExecStart=/usr/local/bin/lightning-goats-gateway --config /run/lg-home-staging/config.toml
   IPAddressAllow=10.8.0.12/32
   ```

   Verify the unit, daemon-reload, then start **only the canary**, without enabling
   it. Confirm actual listener, PID/config, firewall counters, remote switch OFF,
   peer binding and direct handshake. This is the first exposure step and needs
   its own inclusion in the operator-approved manifest. No production unit starts.
9. Run the separately agreed bounded network acceptance with correlated listeners
   and counters, then the 2340-synthetic-sat daemon/canary exercise only after both
   agents ACK the source/protocol. Never issue Strike payments or request port8789.
   Explicitly test denied local services and routed alternatives; retain the
   evidence outside secret files. Recheck public peer bindings before and after.

There is no unconditional "apply all" script: endpoint/reciprocal identity and
independent recovery are not yet agreed. The inventory helper and exact filter
are repeatable now; filling these missing prerequisites must precede presenting
an executable approved session manifest.

## Ordered rollback and recovery

Always **close exposure first**. Removing the direct peer while TCP8790 remains
exposed lets the old broad hub spoof .12 again.

1. Return the canary remote switch OFF using the reviewed canary-only workflow
   when reachable, then `sudo systemctl stop lightning-goats-gateway-canary.service`.
   Verify inactive and no 8790 listener. If OFF cannot be confirmed, stop still
   closes exposure and preserve the unresolved switch/state observation. Leave
   the restrictive table/peer in place until the listener is demonstrably gone.
2. Compare the private post-apply snapshot and the hashes/content of runtime files
   to the approved manifest. Drift requires console inspection; never blindly
   delete changed resources. Remove only the created staging.conf and
   PEER-VERIFIED marker after exact readback, daemon-reload, and verify the
   effective config is again the canonical loopback /etc path. Preserve DB/state.
3. With the listener still closed and the new peer proven unchanged, remove only
   that peer: `sudo wg set wg0 peer "$LG_REVIEWED_VPS_PUBLIC_KEY" remove`.
   The existing route and old broad peer were not edited. VPS separately restores
   its own original reciprocal mapping under its rollback; do not modify it here.
4. After proving the project table unchanged, remove only it:
   `sudo nft delete table inet lg_home_staging_peer`. Verify other nft tables and
   original peer/routes against the private baseline. No nft flush, UFW enable,
   route reset, broad allow or IPv6 disable is part of rollback.
5. Archive evidence and runtime config hashes; keep the canary stopped until a
   deliberate canonical loopback restart. A reboot removes volatile staging
   additions and the service is disabled, but reboot is not an authorized test
   or a substitute for the ordered local rollback. Final persistent containment,
   physical-owner finality and physical acceptance remain separate open gates.
