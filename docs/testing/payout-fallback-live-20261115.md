# Zap-preferred payout fallback — live 2026-11-15

The operator requested ordinary Lightning fallback when zap functionality fails,
plus CyberHerd wording in comments/memos. The [outgoing contract](../architecture/cyberherd-outgoing-invoices.md)
is deployed from PRs [#135](https://github.com/lightning-goats/lightning-goats/pull/135)
and [#136](https://github.com/lightning-goats/lightning-goats/pull/136).

New requests prefer zaps. Missing/invalid zap capability or unavailable signing
can freeze an ordinary callback. Callback failures get at most one ordinary retry
to the same frozen recipient and amount. Valid signed ordinary invoices may use
text descriptions; signature/mainnet/amount/expiry, known hash commitments,
reservations, provider fees and execution identities remain checked. A plain
invoice is not reported as a successful NIP-57 zap. Optional LNURL comments use
`CyberHerd Treats!` within the receiver's advertised limit; its wallet controls
presentation. Incoming invoices keep strict hash validation.

## Observed payout and signer repair

The latest feeding was confirmed at 09:59:51 America/Denver. Its frozen roster
contained Zap Cooking and Sat; Conor joined at 10:13:58, after that cutoff. The
958-sat provider-backed portion funded a 94-sat reservation; existing test-credit
funding did not become reward authority.

The outgoing signer initially retained intents but produced no signatures. Using
its existing credential under UID 973, a non-signing public-key connection check
with the configured five bunker relays timed out, while Primal-only returned the
expected key in 7.44 seconds. Only its bunker transport list changed to
`wss://relay.primal.net`, matching the working receive connection. Public publishing
relays and other services/configurations were guarded unchanged.

Protected repair review:
`/var/lib/lightning-goats-upgrades/payout-signer-relay-repair-20261115-review`.
`RESULT.json` SHA256:
`94cacd8af828f11b1518ff828628b1d7eb5b1bfc4dfdc65a6a679ea703085b0f`.
Signer configuration SHA256:
`647b9e15c7e8612199ab36c32796940888889b2ff640699cfe9a06417c1f487d`.

Signing resumed. Zap Cooking's first 93-sat invoice plus 1-sat fee exceeded its
93-sat gross share, so the existing fee engine reduced it to 92 sats plus 1 sat
fee. The retained authoritative provider observation is `COMPLETED`, with one
payment and 93 sats total. Sat's 1-sat request received text-description invoices
that the old zap-required code rejected; it reached its normal deadline and closed
as `expired` before this rollout. No completed or closed payout was recreated,
no manual transfer was marked paid, and no fee budget was increased.

## Qualification and identities

PR #135's exact integration source passed 451 library tests plus integrations,
audit, isolated nak, browser/package and native systemd checks. One older
no-fallback assertion failed on earlier revisions; its replacement still rejects
changes after callback bytes are frozen and rejects unknown description hashes.

The first maintenance refused before program replacement because the native
reader assumed every signer ID existed in final `payout_invoice_intents`. Two of
three valid signer intents existed only as unpromoted fee probes. A read-only
check of the refused copy found no missing originals across both tables, no
document mismatch and no callback mismatch. Both incomplete snapshot attempts
remain unaccepted. Original programs/units resumed with healthy services:
`/var/lib/lightning-goats-upgrades/payout-fallback-20261115-review/ORIGINAL-RESUMED.json`,
SHA256 `2a9f6328bcb666c8e75a22812db48553bb31f335c48dc610092b0ed3f050291d`.

PR #136 repairs only that reader correlation. Promoted copies must agree;
missing/conflicting/oversized originals and changed signed callback bytes still
fail. Legacy schema-8 images without probe tables retain their original lookup.
A valid unused signature with an ordinary callback is not payment evidence.

Final exact-source checks passed: [Rust/Clippy/root tests](https://github.com/lightning-goats/lightning-goats/actions/runs/43746027688)
(452 library tests, six ignored, plus integrations),
[audit](https://github.com/lightning-goats/lightning-goats/actions/runs/43746027711),
[isolated nak](https://github.com/lightning-goats/lightning-goats/actions/runs/43746027836)
and [deployment/systemd](https://github.com/lightning-goats/lightning-goats/actions/runs/43746027659).
The browser job initially failed while downloading Ubuntu dependencies from an
unreachable mirror, before website tests. A targeted rerun passed; passing native
checks were retained. Both failed qualification logs and installation attempts
remain recorded.

| Source identity | Value |
| --- | --- |
| Reviewed reader-fix head | `6b3b732c9acf59a95a4695973da61c29b388e276` |
| Tested integration source in artifacts | `08d071d4a576c34f75eaf3a1a2bf2ffccd43bbe0` |
| Merged main | `4230636c4286dab3ddc1e993d18544d893b31870` |
| Identical tree | `df5aea8ce13cf3bc5a19ec016e6f1056ead0ac16` |
| Core archive SHA256 | `c3f5a119843fb5f6f8678f64b50bb9aa5ae35b938ab10786899b9ba8ededdec9` |
| CyberHerd archive SHA256 | `f34213bb5bb0caff056a16692378c9519bffa47d3372404f359b425d8a331fa2` |

Core artifact 14174213661 ZIP SHA256:
`dc554709cebed339546662cd07e126f03371ea494bce9721ba0f7c0bfebba633`.
CyberHerd artifact 14175030023 ZIP SHA256:
`77090c5f59fb101e0c8299d28c3c532ea69de815bc4cd17ea1c3940b851088d4`.
ZIP digests matched the authenticated GitHub artifact API; archive hashes, complete
payload manifests and exact source were checked before candidate execution.

## Successful protected installation

The fresh review is
`/var/lib/lightning-goats-upgrades/payout-fallback-20261115-repaired-review`.
Preparation SHA256:
`3ff0c9a9789d77a112f56942fc396865b887f3bfa30041ebd352b6894a3f23e4`.
Final `RESULT.json` SHA256:
`2c93d956eaafc6291fd96a78596d06e4d0e466d193bbe95bab909694a4374209`.

All three candidate worker parsers accepted retained configurations under their
original UIDs with isolated networking. All four writers were stopped/masked for
a fresh snapshot using the qualified corrected statectl; a separate native
invocation verified that bundle. The known-broken installed reader was not
claimed to have accepted it, and neither refused copy was promoted or reused.
Original databases/sidecars stayed byte-identical through installation. There was
no migration, restore, manual ledger rewrite, epoch rotation or credential change.

| Installed program | SHA256 |
| --- | --- |
| lightning-goatsd | `b4157d40075944e2479c48dda39345cbf705b16ea00b138c4ec9552c4565af85` |
| lightning-goatsctl | `cf54d5776250616b3f522c3da5cdffeb2482f776e4039da399286b93463c9937` |
| cyberherdd | `cc164db72c91a9aebaba52ea65a2ebb403d3eee4cb50dbef37315f21f8ac4fcc` |
| cyberherd-payoutd | `b05e1d4fbd4b80b7e6e3e9cf7f1e243d1ddbf5111a8b853a5c7eb6544a82fba6` |
| cyberherd-signerd | `803e3070565fabf3a618f24afb268fd34c6c3d9b903ae0f759424ae5fe82f1c8` |
| cyberherd-statectl | `8d56ea2535a40c76a3a0859469f20e1c54e545bee57a15ff2926446e0f6dda83` |

Original units/enablement resumed. All four services run under their original
non-admin identities with pinned executables; core/member health checks passed
and a later check again confirmed stable services. Configuration, financial limits,
credentials, feeder controls/override, public relay policy, lifecycle/chat opt-outs
and the static site stayed unchanged during the program rollout. No payment or
physical feeding test was submitted and no downstream Nostr event-visibility
measurement ran. Ordinary fallback has synthetic fee/restart/lost-execute
qualification; a new real ordinary reward settlement has not been observed here.

Old records retain identical zero-default serialization. New comment terms and
ordinary invoices require compatible programs/operator tools; rollback after new
records appear requires compatibility review and preservation of current journals.
The existing external recovery-adoption blocker remains separate.
