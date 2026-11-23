# Native household incoming receipt signing — 2026-11-10

**Passed:** the actual VPS's isolated non-admin helper signed kind **9735**
through the household bunker using the existing core authentication client.
`NakClient::sign_zap_receipt` verified the mainnet invoice's signature/amount/exact
request description hash, then native frozen-field/hash/BIP-340 checks verified
the returned project receipt. No ledger was opened, invoice requested, provider
contacted or receipt published. The invoice was locally generated with a public
test key and synthetic payment hash; it is not evidence of payment.

Qualified code `2269680c22f46a617ba39750a05f93eb8338f2ac` and tested merge
`7eb1ce69ac1b8fdaf71ce4a9c41b56a2471971a7` share tree
`04ceab857a24f5674790328d3add62f5dad46ef8`. All ten workflows passed:

- [CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42877381263):
  format, strict locked all-target/all-feature Clippy, tests and required root checks.
- [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42877381047):
  all five jobs, explicit offline example test, archives/browser/nginx and disposable
  native/systemd checks. Package job `130044193321`; systemd `130047368401`.
- [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42877381052):
  existing dependency/provenance/security gate without a dependency change.
- [Real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42877380808):
  native receipt signing and no-9735-publication assertion, plus existing durable
  retries, four-client/cross-version and bounded connect-observer checks.

The first new real-nak regression incorrectly called the kind-1-only verifier
after native receipt signing. That extra check failed on source
`85244ce6e7235efb035b265cc98f8edfe7dd8eeb`; the native receipt itself had passed.
The correction uses the receipt path's existing native verifier and preserves
the kind-1 guard. No application verification or publication policy was widened.

Test-only artifact `13792457414` has ZIP SHA256
`aa484458f2612d636e3dcbb758d5dc5d967de4ea12adeb77347755ef9ce25ac9`.
Its outer digest, all four inner checksum entries and export source were checked
before selecting `SIGNING-ONLY-check-zap-receipt`, SHA256
`4fd4bad9aab2d94d9153b3868120110a7666c2b13b6ec8d5f811b582f0e2fd6d`.
The tested merge's tree was independently compared with the selected code tree.
The relevant `nostr.rs`, `zap.rs`, `zap_receipts.rs`, `bolt11.rs` and `Cargo.lock`
git blobs also match the installed core's qualified build source. No application
binary was replaced.

The helper was installed in a new protected prefix and its assembled one-shot
unit independently hashed/verified. An extra per-process `/proc` mount added to
the first unit conflicted with systemd's namespace setup: exit 226 before helper
execution. Its exact unit, revision and sanitized failure remain private. Removing
only that extra mount restored the reviewed template; the next launch passed.
No credential or application setting changed to obtain success.

The successful unit loaded only its existing NIP-46 client credential, hid the
running receive service's credential directory and production state/configuration,
used strict filesystem/private-network restrictions and a private runtime, and
had no capabilities or core dumps. Its sanitized report confirms native signing,
exact author/unsigned bytes/hash/signature and zero ledger/provider/publication
operations. The request's exact independent fixture bytes, including Unicode and
whitespace, are preserved in the description-hash/receipt binding.

After terminal success, the exact revised unit was removed after byte comparison.
Its private runtime was gone. Original three role-journal hashes, four
configurations/service states and running receive executable hash matched before
and after. Receive health/status returned 200. The real CyberHerd units remain
masked/inactive, all new limits zero and the live core bridge/native zap config
disabled. Private evidence retains before/after/source/result and both launch
observations; no signed receipt or invoice is retained or emitted.

Together with [membership/payout signing](cyberherd-bunker-checkpoint-20261110.md),
this qualifies the required household signing kinds. It does **not** qualify live
incoming-zap settlement or public receipt delivery, enable CyberHerd, authorize
outgoing payments or establish the full twelve-part implementation as complete.
See the [bounded procedure](../deployment/cyberherd-zap-receipt-qualification.md).
