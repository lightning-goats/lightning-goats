# Disposable CyberHerd OS policy probes — 2026-11-10

Qualified code `caa4e78c4846ac78d00da89be62ca2331ca43ee7` and tested merge
`8fb0244e6d97b364ea169d3a0f0c54bb2443b556` share tree
`1f5a97d3aa247045ad712a1af090faf4f3971868`.
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42834236108),
job `129892048841`, passed format, strict locked all-target/all-feature Clippy and
the locked all-feature suite: **495 passed, zero failed, four ignored**. Four
required separate root completion markers passed; root repeats and inactive
example tests remain excluded from 495.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42834236005)
passed all five jobs. Systemd job `129894451781` repeated the actual synthetic
core upgrade and three-role inactive installation, then ran three new Python
policy probes with copies of the installed unit protections. The real application
units remained masked; no application worker started or journal bytes changed.

Each probe positively checked its actual UID/GID/groups, NoNewPrivs, empty kernel
effective/permitted/bounding/ambient capabilities and exact private network
namespace. Its own synthetic encrypted credential was available with exact
expected bytes; other credential paths and foreign journals were inaccessible.
Installed programs/configuration were non-writable, while private own-state
unnamed scratch writes succeeded. A mock public IPv4 endpoint was reachable and
mock trusted IPv4/private IPv6 endpoints were denied. Root positive controls first
proved all three endpoints reachable in that same isolated namespace.

These are actual OS policy checks with Python probes and synthetic data, not
Rust-worker/socket, DNS/recipient SSRF, provider, household, browser or live-host
qualification. The harness refuses an ordinary/non-root/non-CI/host-network run.
It was not executed on the live VPS.

Systemd artifact `13772499068`, ZIP SHA256
`14620e61f618478b62ed56d702aefc0bfea6f2c9b2ba498e23871a0a7427b724`,
was downloaded and its digest, internal checksum manifest and both rehearsal
receipts inspected. The initial sandboxed download failed DNS resolution before
writing files; the approved network-enabled read succeeded. No artifact code was
executed locally. The receipt reports three actual policy probes, unchanged
journals, no application workers and false provider/household/live-host flags.

[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42834236159),
job `129892048321`, passed 315 locked dependencies against 1,446 advisories,
original SQLite driver provenance and RSA unreachability, without a SQLite waiver.
[Isolated real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42834235493),
job `129892047299`, also passed its separate synthetic nak/relay/bunker fixtures.
It does not authorize or qualify the new actual household clients.

The separate [live core preparation](cyberherd-live-core-upgrade-20261110.md)
retains the earlier qualified program build; this CI extension is not a new live
deployment. PR111 stays draft and the full twelve-part goal remains active.
