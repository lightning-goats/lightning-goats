# Ordered admission progress — 2026-11-14

Conor's current repost `9d3b30ec543a52be7dab76616ef48de78fe03e55c72d5b5d7ecde89c6871aa38`
was observed and staged against today's project note. It remained at ingress 57
behind a like at ingress 55 from an account whose profile was unavailable; his
like was at ingress 56. No current-cycle admission decision had rejected Conor.
The three-seat roster consisted of zero-zap social members, so a verified repost
could displace a member under the existing rule.

Two progress defects were identified. Independent presentation workers consumed
the core's global bounded connection allowance and caused intake broken pipes.
A profileless nonmember like also waited indefinitely even though a like cannot
enter an already full herd, regardless of the missing metadata.

The membership runtime now shares one paced FIFO client across intake, view,
reward/result, overlay and publishing actors. Authenticated operator actions
retain their existing direct client and short deadlines; they do not bypass any
server admission or authorization check. The server's admission, UID,
capability, database and financial limits remain unchanged. Fresh feeder state
and target/day projection are checked after lifecycle I/O and pacing waits.

An atomic capacity-only path can decline a nonmember like without a profile,
only for a confirmed-current target with feeding ready and capacity still full
inside the same write transaction. It cannot grant a seat, invent a payout
address, bypass pause, or treat missing metadata as a negative payout address.
If any condition changes it retains the profile wait. Reposts and eligible
open-seat interactions still require authenticated profile data. Existing
receipts remain frozen and ordered input is completed through the normal worker.

Regressions cover six concurrent paced clients against actual server limits,
profileless full-herd likes followed by an admissible repost, pause preservation,
missing-profile repost deferral and open-seat deferral. [PR #119](https://github.com/lightning-goats/lightning-goats/pull/119) is now
merged and deployed. All ten workflows passed for source
`44748de420aa64a2e2de74e069f7fd16f5f53f48`, including strict Clippy, full locked
Rust/integration/root tests, security audit and installed systemd rehearsal.
The tested artifact merge was `c1be5d941906c2000efe7c3226e69ee38886f8e8`;
Rust run `43623825909`, deployment run `43623825977`, systemd job `132536628713`.

Only the membership binary was replaced (SHA256
`29636aea0b37c990d85fd0153d428ba88106d12ccc9472ffafacb213df898dde`).
The original runtime UID accepted the unchanged live configuration offline.
Approved messaging maintenance took a fresh verified native four-journal backup,
guarded live journal bytes unchanged through replacement and before resumption,
and restored all four original service identities and enablement. Configuration,
financial policy, credentials, other programs and feeder controls were preserved;
stream chat remains disabled.

Protected evidence directory:
`/var/lib/lightning-goats-upgrades/cyberherd-admission-44748de-review`.

- PREPARED: `cc23a49439ab07d8fc2e3d85234115608a236d52765f48fadefba2b2581abfdb`
- APPLIED: `154edca4dcd1bc4d561d1ddcf81ac6e635ca771fe596f5b8af399c470d91b437`
- RESUMED: `7d3ef8f7cc318787c694ab0611c5692345253d5cbebf7c0b2c9104f8577e8d33`
- ADMISSION-VERIFIED: `f5e05858217bf5e7056b5a5567498849c380bcbe2aad8314be01be2adcb7aa0d`

The normal worker recorded the blocking like as Full, then Conor's like as Full,
and his repost as Displaced (victim
`228056c3400fb707ec673aa5c7a0ad73095ffe592c561765e9acf6bc08cb2578`).
Read-only verification found the queue drained to zero pending inputs and no
recent intake broken-pipe errors. The live public roster was ready at revision
25 with Conor, Sat and Destiny at four points each. No membership or accounting
was manually inserted or rewritten; the existing admission rule processed the
original retained signed events.

Deployment staging space was recovered by clearing only the disposable Rust
incremental compiler cache after verifying no compiler was running. Source,
programs and production backups were retained. Two redundant CI ZIP wrappers
were also removed only after every contained file was proven byte-identical to
retained extracted files or binaries inside the retained release tar archives.
