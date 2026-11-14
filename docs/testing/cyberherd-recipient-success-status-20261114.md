# Recipient discovery success status — candidate, 2026-11-14

Read-only installed qualification found that two current CyberHerd members'
Lightning addresses passed native discovery, while Conor's Primal address did not.
A public discovery GET returned HTTP 200 and otherwise valid `payRequest`
metadata with `status: "OK"`. The native parser rejected every present status,
including that explicit success value. The candidate accepts absent status or
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
payout worker validates the already frozen typed recipient, independently checks
the invoice and preserves its existing financial execution policy. No schema,
credential, ledger, historical plan, payout ceiling or feeder change is proposed.
The read-only native helper must be rerun on all three addresses from the exact
qualified candidate before declaring the discovered Primal failure resolved.

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

Formatting and whitespace pass locally. Exact-source Rust/security/native checks
and actual corrected discovery are pending. This candidate has not been deployed.
