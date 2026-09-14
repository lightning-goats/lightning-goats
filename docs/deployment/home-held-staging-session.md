# HOME held-canary staging session candidate

Preparation only. No household policy or network listener changed. This extends
the reviewed direct-peer plan in `home-staging-peer-candidate.md` for the
installed generation2 held fixture; it does not activate the physical owner.

## Exact HOME scope

Use `deploy/nftables/home-held-staging-peer.nft.example`: only authenticated
staging source10.8.0.12 to destination10.8.0.6 TCP8791 is admitted by the early
guards. TCP8790 and8789 are denied. The original echo8790 policy is preserved.
Both candidates intentionally use `inet lg_home_staging_peer`; they are mutually
exclusive. Require that table absent before application; never stack or silently
replace an existing policy. Direct peer key binding and removal ordering remain
mandatory. An IP address alone is not peer authentication.

Use only `lightning-goats-held-canary.service`, binary
`/usr/local/bin/lightning-goats-held-canary`, canonical config
`/etc/lightning-goats-held-canary/config.toml`, protocol `uuid_held_canary`,
fixed `LightningGoatsHeldCanary2Request`/`LightningGoatsHeldCanary2Ack` bindings.
Keep its installed credential and state paths unchanged. Credentials stay HOME.
The original echo service is outside this change and stays loopback-only.

Adapt the earlier plan's steps7–8 only for this service and8791: produce a fresh
root-owned volatile config whose sole parsed change is service.listen from
127.0.0.1:8791 to10.8.0.6:8791; runtime drop-in targets the held service and invokes
its own installed binary. Preserve its ExecStart arguments, credential name,
state path and hardening. Bind lifetime to wg-quick@wg0 and require the reviewed
peer marker. No service enablement, persistent config edit or production start.
A reviewed private manifest must pin the rendered files and their hashes before
approval; this document does not substitute unfilled values for that manifest.

## Baseline and release custody

Read-only capture at2026-09-13T22:47:58.906936Z verified source SHA256
`1cacb11569ab90db03bc0ee2f94fd3fec9d0e2132bc816d4f5982d4fd7d6a88c`, count2,
two released deliveries, HoldON and remoteOFF. Full capture retained privately,
SHA256 `b27f4164e2ce931acc13ae59e2bf7ae400835290a7865ef69d75eca571d2bcae`.
No Request or Release command was sent. Refresh and compare immediately before
execution; never reset history to make this baseline current.

HOME owns exact-UUID release and immutable pre-dispatch intent. VPS supplies the
synthetic run UUID, final reviewed source/artifact/tool hashes and scenario plan.
Preserve gateway and daemon state as a pair; no fresh request after ambiguity.
For late-result recovery, retain the original UUID across gateway restart, then
HOME releases only that recorded held UUID. GET recovery and replay must add no
owner delivery. Exactly two additional distinct deliveries must match two
confirmed debits and2340→1340→340 accounting. Use the corrected independently
reviewed PR82 checker; its original6cd52ba checker has a chronology false pass.

## Gates and rollback

Before exposure: fresh private baseline/drift guard, exclusive administration
window, independent recovery ACK, exact reciprocal peer mappings, fresh direct
handshake and restrictive VPS output policy with inspection exceptions removed.
Apply only after the specified HOME household-network approval and coordinated
harmless-test approval. VPS's reported approval does not apply HOME policy.
No packet tests below establish actual cross-host containment.

After approval, use the existing plan's guard-before-exposure sequence with the
held substitutions above. Verify actual listener/process/config and bounded
negative accesses using listeners and filter counters. Preserve all captures.
Close exposure before peer rollback: remoteOFF when reachable, stop held service,
prove8791 absent, then remove only unchanged runtime drop-in/marker, direct peer
and finally the unchanged project table under drift checks. Retain both stores,
intent and pending evidence. Never restore stale financial state as routine
rollback or remove the peer while the listener is exposed.

## Tested preparation

Both `sudo unshare --net -- python3 -B deploy/nftables/test-home-staging.py`
and its `--held` variant pass13 packet cases plus table-only rollback. Cases
include the other canary and production ports, SSH/OpenHAB, ACK bypass,
pre-DNAT/container forwarding, alternate destination and local DNAT; legacy
sources retain existing behavior. Two existing drift regressions pass. The first
local candidate test failed due to harness variable shadowing; fixed before the
successful rerun. No Rust source/build or live network change was involved.

Outstanding: independent VPS review, current-main CI, corrected accounting
candidate, paired restore scenario details and completed private execution
manifest/approval. Final household containment and physical acceptance remain
separate gates; production HOLD.
