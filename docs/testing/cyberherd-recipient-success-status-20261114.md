# Recipient and invoice success status — live 2026-11-14

[PR #123](https://github.com/lightning-goats/lightning-goats/pull/123) is merged
and deployed. The corrected native discovery path passes for all three current
members under both actual worker identities. Financial state, limits and
credentials were preserved; actual payout/provider/recipient acceptance remains
unobserved.


Read-only installed qualification found that two current CyberHerd members'
Lightning addresses passed native discovery, while Conor's Primal address did not.
A public discovery GET returned HTTP 200 and otherwise valid `payRequest`
metadata with `status: "OK"`. The native parser rejected every present status,
including that explicit success value. The corrected parsers accept absent status or
exact uppercase `OK`; errors, unknown/case-changed values and malformed required
metadata still fail closed.

[LUD-06](https://github.com/lnurl/luds/blob/luds/06.md) defines successful
`payRequest` metadata and explicit `ERROR` responses. The additional success field
is observed provider compatibility, not an assertion that LUD-06 requires `OK`.
Existing amount bounds, metadata cardinality, public HTTPS/DNS admission,
callback restrictions and advertised Nostr receipt-key validation remain intact.
The regression covers equivalent absent/OK success, Nostr support and refusal of
error/unknown statuses, invalid bounds, private/reserved callbacks, ambiguous
metadata and invalid receipt keys.

Only the membership reward worker calls `Recipient::fetch` in production. The
same all-status refusal also existed in the payout invoice-response parser.
That wrapper now accepts absent status or exact `OK`, then always validates the
signed invoice against the frozen amount/description, mainnet signature and
remaining expiry. Regression fixtures cover ordinary and signed-zap invoices,
explicit errors and success wrappers around contradictory/malformed invoices.
No actual invoice callback was requested to inspect provider wrapper behavior.
The membership and payout programs therefore both require the qualified update;
all financial execution guards and historical signed intents remain intact. No schema,
credential, ledger, historical plan, payout ceiling or feeder change is proposed.
The read-only native helper was rerun on all three addresses from the exact
qualified source; its actual results and maintenance evidence follow below.

## Actual observations before the correction

The current-source helper came from CI artifact `14126424699`, outer ZIP SHA256
`4d94ddbe652bc4b9e4c74710af888115815af1a7dcac8e806aed47f755078ea6`,
tested merge `76951b1868945a35a67bcbc3f88d7feee56d1339`.
Helper SHA256:
`1bbfb6aa2ccc7b9517b72c11b9c98ed4a598fbc273ccfd6088d6e549588fde0d`.
Three transient restricted units used the actual payout identity, hid production
configuration/journals/IPC/credential stores, loaded no credential, performed no
callback/invoice/provider operation and left production service PIDs/configuration
unchanged. Reports contain sanitized acceptance and bounds, not callback URLs.
Protected result:
`/var/lib/lightning-goats-upgrades/current-member-recipients-fd1bfa4-review/RESULT.json`,
SHA256 `5b820ed1eff4f1459fba8e11bbe1d6bcc55084ef094a837bace1234f56c30803`.

Separate read-only inspection verified both existing `no_payout` plans' frozen
JSON hashes: each had zero active members/rows, a 100-sat budget and zero reserved
sats. They therefore do not indicate a stalled payout. Today's cycle starts at
`2026-11-14T00:00:00-07:00`, reset is enabled for America/Denver, and exactly one
matching midnight command receipt remains after the actual service restarts.
No reset, credit, payout or feeding was submitted for this observation.
Protected `REWARD-AND-CALENDAR-OBSERVATION.json` in the zap messaging review has
SHA256 `b96131f18f9528ce6e56937e6183dadf978091a46139ee909898bccfca85577f`.
This verifies the recorded midnight and restart retention, not every missed-day
or DST path on production. External payout/receipt/recovery acceptance remains
open in the [full-port inventory](cyberherd-acceptance-matrix.md).

## Exact-source qualification and corrected discovery

Source `5cef63b9990e08ed70dd94c8be5d8d2ad30ae591` passed all ten workflows.
[Rust run 43632973568](https://github.com/lightning-goats/lightning-goats/actions/runs/43632973568)
passed formatting, strict locked Clippy, 434 library tests (six ignored),
integration and mandatory root fixtures. Both new explicit-success regressions
passed. [Deployment run 43632973675](https://github.com/lightning-goats/lightning-goats/actions/runs/43632973675)
passed all jobs, including native systemd job `132565690055`. Security run
`43632973567`, real isolated signer run `43632973580` and all home/network checks
passed. Tested merge `95fcecffd526ab586d91b2132f1ca38ad36936d3` and actual main
merge `4c671f9326d16e85d09bf0210443dd5940d23166` have identical tree
`38962ace013dbb0f782e2654ff959b1280b82624`.

Qualified artifacts:

| Artifact | ID | ZIP SHA256 |
| --- | --- | --- |
| Program review | 14129345491 | `ad7ccbb13f374401b47f516ece9ff6c1fd92ab7c4e56d652a9da764bae1f51cc` |
| Read-only qualification | 14129280666 | `39a0811aa2b8d8c4375d96726d2d2898db4269d1df86d124a9e23f5ad6c46198` |

Retained review archive SHA256:
`6a6e599d8824e381566e282a70ffa2287ed99bcc980aa6d66a9b72b2a730bd49`.
Corrected native helper SHA256:
`f0eb08755e1aa65a646367588381db6e871eff85db274091ddf4a7400c1f841b`.
Outer/inner checksums, the full program payload and exact artifact source were
verified before staging or execution.

The first corrected native probe accepted Primal under the payout UID but its
wrapper refused an effective-property check: `systemctl show` prints complex
credential arrays as `[unprintable]`, including empty arrays. This was not a
recipient failure or a production change. Typed D-Bus inspection proved both
credential arrays empty. The corrected repeat checks the actual `a(ss)` arrays,
retains exact unit files/hashes and effective protections, and keeps failed
attempt/diagnostic evidence rather than rewriting it.

All three addresses then passed under the payout UID, followed by all three under
the membership UID where production discovery runs. Every recipient advertises
Nostr support and a 1,000-msat minimum. Their observed upper bounds are 11 billion,
1 billion and 100 billion msat respectively. No callback, invoice, Strike provider
operation or signing credential was used by these checks. The exact temporary
units were stopped/removed, and production PIDs/configuration stayed unchanged.

Protected corrected payout-UID qualification:
`/var/lib/lightning-goats-upgrades/current-member-recipients-5cef63b-typed-review/RESULT.json`,
SHA256 `07e284b4c869e1c40fe84e58708df54c32d902b36baab95b2a3eef92b35bfefd`.
Protected membership-UID qualification:
`/var/lib/lightning-goats-upgrades/membership-recipients-5cef63b-review/RESULT.json`,
SHA256 `eecfd300085df8996024a0a6e0c6b45527db490531a96c89658d79158ff6dbe2`.

## Actual protected program maintenance

The two selected programs accepted the unchanged live configurations offline
under their original UIDs, with isolated networking and no credential/state
operation. The prepared transaction replaced only `/usr/local/bin/cyberherdd`
and `/usr/local/bin/cyberherd-payoutd`, preserving original ownership, mode and
SELinux labels. Core, signer, state verifier, configuration, credentials and
financial limits were unchanged.

Maintenance stopped/masked all four writers, took and verified a fresh native
four-journal snapshot, guarded the actual main/WAL/SHM fingerprints unchanged
through replacement and again before resumption, restored the original units
and resumed all four identities/enablement. No journal was restored, reset,
manually edited or replanned. Existing execution identities and signed intents
remain retained.

Protected transaction:
`/var/lib/lightning-goats-upgrades/lnurl-success-5cef63b-review`.

- PREPARED: `895ac46e4af61a86d4a81c8b63e87dfef87d39bd495537718b355be805013ccf`
- APPLIED: `507871c995842d357ccef4ce641c39b568e05f9bce5f5f2239011a61a5bea6be`
- RESUMED: `000e39f7f5be2fafe5f11278df835ae5b81d916f5a1fd936af7bf936dc4ea428`
- RUNTIME-VERIFIED: `8e2a2a5cb1effb118f0bcf56268536ca24880df2c295be87a4e977ddb0637c6e`

Installed membership SHA256:
`8616193750221e6db9de2551b16aaca1387db1e8f1f5a5e4cd54e2f4492907bf`.
Installed payout SHA256:
`58ded6f9758c4289d25bcf24b05a2293330781e47f58ff8f2cc232c32f72afa3`.

At 103 seconds after resumption all four services were running/enabled with zero
automatic restarts and zero main-process exit status. Core health/status and the
membership API passed; ordered ingress was empty, six zap-feeding templates
remained enabled, chat stayed disabled and there were zero new chat receipts.
No new payment or confirmed feeding was observed in that window. No test payment,
credit, callback request or physical feeding was submitted for this release.
The actual invoice-wrapper fix is qualified by signed-invoice regressions;
metadata discovery does not prove an executed payout, fees or recipient receipt.
