# Zap-aware payment and feeding messages — live 2026-11-14

[PR #121](https://github.com/lightning-goats/lightning-goats/pull/121) is merged
and deployed. Payment and confirmed-feeding notices identify a verified zapper
with a Nostr profile mention and use the actual settled zap amount. Ordinary
Lightning wording, the ten-sat message floor, one feeding announcement per
payment, custom templates/opt-outs, optional goat naming and frozen signed
retries remain intact. CyberHerd stream chat stays disabled.

## Attribution boundary

Attribution comes from the authoritative settled credit, its exact signed NIP-57
request, invoice description hash, signed invoice amount and payment hash.
Comments and wallet labels cannot identify the donor. Anonymous requests use a
generic label and no sender mention/tag. Distinct settlements keep distinct event
identity even when their signed request is reused.

Core Nostr and overlay paths derive this context read-only. The separate feeder
presenter matches retained authenticated payment input to the core's immutable
epoch/owner/consumed-credit announcement identity before the frozen feeding
barrier. A later donor sending the same amount cannot take attribution. No
accounting, schema, credential, financial-policy or feeder-control change is made.

The current upstream explanation was fetched again on 2026-11-14; its SHA256
remains `e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`.
The wider [full-port inventory](cyberherd-acceptance-matrix.md) remains open.

## Exact-source qualification

Source: `fd1bfa41ae4d1f702eea2d6d4fde01915ec0e0ed`.
Tested artifact merge: `76951b1868945a35a67bcbc3f88d7feee56d1339`.
Merged main: `ff9f55f3c420b891bd937474d4059ca885bd56f1`.
Tested/merged tree: `bf70000aa2354dbb95699c9c5f89304ea0ef0abb` (identical).

All ten workflows passed. [Rust run 43628853119](https://github.com/lightning-goats/lightning-goats/actions/runs/43628853119)
passed formatting, strict locked Clippy, 432 library tests (six ignored),
integrations, mandatory actual-root fixtures and inactive preparation. The
same-amount/interleaved-donor regression, signed-author/invoice checks, anonymous
labels, announcement suppression and persisted custom-template/opt-out tests
passed. [Deployment run 43628853333](https://github.com/lightning-goats/lightning-goats/actions/runs/43628853333)
passed package, website, nginx, canary and native systemd job `132552924361`.
Security run `43628853219`, real isolated signer run `43628853150` and all other
home/network checks passed. The initial draft's compiler failures were fixed;
none of its failed artifacts was installed.

CI artifacts and checksums:

| Artifact | ID | ZIP SHA256 | Retained archive SHA256 |
| --- | --- | --- | --- |
| Membership review | 14127720433 | `acf5ee17df5ec83be570b00d16a8e2d4b2ee552dcaf660d9c4347e410351ce39` | `d406a87344bc331fefe08962e5db63836b505d47d1451b6a4bfc1934ea603a6b` |
| Core release | 14126509656 | `45aa41cdd7d12bfba67d9896f7eeb1602083df7083b5a7e53001294bde5c4b3e` | `ffe1eaf8b1a548edb04d4c1eb9fa7749846a37bebc8eec911fbebd0c346ffd41` |

Outer/internal checksums, full payload/source verification and authenticated
root-tool verification passed before staging. A full temporary filesystem caused
the first downloads to stop; only those two incomplete task files were removed,
and verified artifacts were staged on the disk-backed workspace instead.

## Protected actual maintenance

The sealed transaction replaced only `/usr/local/bin/lightning-goatsd` and
`/usr/local/bin/cyberherdd`. Inactive checks ran the core CLI and membership
configuration parser under their original non-admin UIDs with isolated networking.
The native parser accepted the retained membership configuration. Exact original
file metadata/SELinux labels and the existing service/unit/configuration bindings
were retained. Other programs and all configurations stayed unchanged.

The approved four-service messaging maintenance stopped/masked all writers, took
and verified a fresh native four-journal snapshot, compared actual main/WAL/SHM
fingerprints through replacement and again before resumption, and restored the
original units/enablement. No journal was restored, manually edited or reset.
Services resumed under the original runtime identities. Original payout limits,
credentials, feeder controls and the operator's chat opt-out were preserved.

Protected evidence directory:
`/var/lib/lightning-goats-upgrades/zap-payment-messaging-fd1bfa4-review`.

- PREPARED: `6a7db26aeebd3c8c62e51ee3f57962bc0356b3638c27f9fc4ab1c8c448ae698b`
- APPLIED: `0d7c2b95dbd980e4afce752f1309077b79bb8fbfa07069cc55a6ee4f2f32f831`
- RESUMED: `56174fb684a23ab66f9081eaab911336c4311c19304750a1d7be247370905bef`
- RUNTIME-VERIFIED: `ba2a8c8131246d509d2e87f35164148ef8f4406ce3afb83918db66d425ca4b36`

Installed core SHA256:
`181f38650a89edaea9015817b53138a32ce0ba02535ce58561684761bee42b6c`.
Installed membership SHA256:
`3f8691e4fbde9bd9ae669874e97ff8629c5c60108867bd835c7608319f4dcfbc`.

## Runtime observation and remaining acceptance

At 102 seconds after resumption, all four services were running/enabled with
zero automatic restarts and zero main-process exit status. Core health/status
and the membership API passed. Six enabled zap-feeding variants were present,
ordered ingress had no pending work and there were zero new CyberHerd chat
receipts. There had been no new payments or confirmed feedings in that window.
No test transaction, credit or physical feeding was submitted for this release.

This establishes installation, configuration preservation and running-service
acceptance. A real incoming zap, public relay receipt and OBS observation of the
new wording remain to be observed; source fixtures are not substituted for that
external acceptance. The wider payout/provider/recovery/full-port requirements
remain separate and unfinished.

A later read-only acceptance observation confirmed ready roster revision 25,
capacity/active count three, allocation 10%, notes/lifecycle enabled and chat
disabled. The membership journal had two `no_payout` plans; the payout journal
had no grants, recipient rows or provider payment records. No provider was
contacted and no financial state was changed for this observation. Protected
`ACCEPTANCE-OBSERVATION.json` SHA256:
`0357be81fc9cba0e96e43d6e2a986248f08a8a0c0faff939062e5d43c1a43de7`.
