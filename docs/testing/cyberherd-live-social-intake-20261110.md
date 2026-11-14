# Live reaction recovery and OBS membership — 2026-11-10

**Passed:** the operator's reaction and another user's earlier reaction/repost
now produce durable membership, exact scores, overlay announcements and actual
OBS profile cards. CyberHerd public Nostr, rewards and payouts remain disabled.
The existing payment/feeder core stayed running throughout this correction.

## Defect and exact qualification

The operator reacted to the authenticated daily note already tracked by the
worker. A read-only native Nostr query verified that reaction on a configured
relay and found a valid signed profile/Lightning address. The cycle's reachable
relay results also contained another author's reaction. Neither appeared in the
roster. Historical scanning advanced only five minutes per round from cycle start;
changing stream versions/aliases also changed the combined scan identity. This
could repeatedly restart catch-up before it reached current reactions.

Source `fb73c1d600673cf814618af7da8f9a9634cde797` separates a bounded recent
scan from historical recovery and retains stable per-event/per-coordinate cursors.
A newly discovered alias gets its own recovery scope. Complete recent reads
advance only already-covered cursors; older gaps still require actual scanning.
Quiet historical days catch up in one bounded query, saturated windows bisect
and missing quorum/full inbox never advances history. Old cursor rows and raw,
ingress and award receipts remain retained. All users follow the same eligibility
and once-per-cycle rules; there is no special operator admission or NIP-05 gate.

All ten workflows passed. Tested merge
`489e687ebf871ccb1f001dbeba838bbb10bfafa2` has zero file differences:

- [CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42908451631),
  job `130152490192`: format, strict locked all-target/all-feature Clippy, all-feature
  tests and mandatory native root fixtures. New earlier/current-user recovery,
  stream-version/alias, saturation, quorum and replay regressions passed.
- [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42908451902):
  all five jobs, including installed systemd `130155413837`, root recovery
  `130152491507` and browser `130152491310`.
- [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42908451751)
  and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42908451595).

## Bounded installed update

Review artifact `13806397170` has ZIP SHA256
`a687b554aa83941f3337a731425d6e172c67d425dc59da1f7bc7091ffd8ca35e`.
Outer ZIP, inner archive and every file checksum passed. Archive SHA256 is
`996d3351d2bf2a1653fc4c2df6ca38f35c39cd3ddd265aacf571a6d99e7a4b14`.
All **23 migration/dependency files** exactly match the original installed archive;
there is no schema, engine or dependency change. Only `cyberherdd` was selected:
SHA256 `21e14ee90d48407e2e641e6827857cbec0e43d2ef1fdf96c4024f0f21bf3fb3d`,
22,269,352 bytes. It was installed as protected candidate data and its actual
configuration check passed under the non-admin membership user, without runtime.

The existing authenticated seventeen-file generation supplied its already tested
recoverable file transaction. External launcher and manifest verification preceded
root interpretation. A new protected one-binary transaction retained the original
executable, exact old/new fingerprints and unchanged core/member configuration
and unit bindings. Prepared receipt SHA256:
`ae18035c8e795499e1c4bfa108c4a9cc2b5e616ab4797b24d18b36afd7ea17ef`.
Transaction manifest SHA256:
`db4eebc3ba61c8b6518199f684eff4a7423d346c3cfa9c8e2510ebbfd6f20643`.

Under the existing approved overlay-stage maintenance scope, only membership
was stopped/masked. Its quiesced main/WAL/SHM bytes were privately retained as
opaque evidence, **not claimed as a native four-journal backup**. The earlier
verified native backup remains retained. No database was adopted or restored.
The file transaction replaced only the worker executable, preserving metadata;
unmask/start verified the loaded executable and loopback view. Failure recovery
was limited to the original binary with all newer journal rows retained.

Protected transaction `cyberherd-reaction-update-fb73c1d` records completion.
Core PID `1434299` stayed unchanged and healthy; configuration, financial limits,
epoch, feeder history and outgoing masks stayed unchanged. The original inactive
installation receipt remains historical evidence, with this update providing the
new executable binding; it was not rewritten to manufacture a fresh installation.

## Actual observed behavior

The roster reached revision 3 with two eligible members:

- Destiny: reaction and repost awards, 4 points, 8% projected share.
- Sat: reaction award, 1 point, 2% projected share.

Both have zero paid sats. These are exact projected allocations from the
configured 10% member pool, not executed or promised payouts. The core retained
28 sats credit, 1,000-sat threshold, zero feeds due and no unresolved feed request.

Readonly replay over the actual public OBS WebSocket verified three original
announcements: Destiny's join, Destiny's repost and Sat's join. Distinct stable
message identities/sequences are retained. Actual sandboxed Chromium displayed
both human-name profile cards without page errors. Reconnect restored the roster
without replaying payment/feeding effects. No synthetic reaction, credit grant,
Lightning payment, public Nostr test message or physical feeding was initiated.

An actual membership-only restart retained cycle 1, revision 3, both members
and their exact scores/shares/awards. A further observation after catch-up found
no duplicate awards. The core PID remained unchanged throughout.
Future CyberHerd Nostr activation uses the authenticated announcement/member
conversation branches and relevant active-stream chat copies. It is still off;
weather/information remain overlay-only. The full port's remaining provider,
public-interface, boot-persistence and financial/recovery qualification stays open.
