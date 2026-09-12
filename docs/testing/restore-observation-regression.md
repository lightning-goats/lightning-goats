# Restore-test startup observation regression

The Rust job for documentation PR58 at
`bb2a29dd8dbc468f37cafc4945e3be1d080106ff` failed while waiting five seconds for
the first harmless owner command:
https://github.com/lightning-goats/lightning-goats/actions/runs/34721940017.
That run's original cause is unproven because its daemon stdout was discarded.
Do not treat a rerun as evidence that the original failure was understood.

Source inspection shows `run_feed_worker` waits five seconds after unavailable
safety state. A five-second observation window cannot accommodate that backoff
plus a delayed initial read. The paired-store test now injects one 500ms failed
safety read against the harmless owner, asserting zero commands at that failure.
The fixture with the old observation window reproduced `Elapsed(())` locally:
one test failed in 5.21 seconds at the first-command assertion.

The candidate allows fifteen seconds for this startup/retry observation and
checks that both child processes remain alive while waiting. Production retry
intervals, request/completion deadlines, reservation behavior and the shipped
configuration are unchanged. All existing paired-store assertions remain:
quiesced backup, unresolved UUID preservation, no resend, exact signed outbox
bytes, late matching acknowledgement, and exactly one accounting debit.

Focused verification:

```sh
cargo test --locked --all-features --test gateway_admission \
  paired_store_restore_preserves_pending_identity_settlement_and_signed_bytes \
  -- --exact --nocapture
```

Run the complete `gateway_admission` suite and required Rust checks on the final
candidate. These are real local daemon/gateway processes with loopback mocks and
synthetic credit, not live provider, home owner or physical-feeding acceptance.

Local candidate results: focused test passed in 12.06 seconds; all eight
`gateway_admission` tests passed in 38.08 seconds; formatting and locked strict
Clippy passed. Exact published-head Rust/Security/Deployment checks remain
required. The negative control and these results do not identify the earlier
CI timeout's original cause or close live restore/owner acceptance.

## Restart/confirmation observation follow-up

Updated documentation PR58 head `789351856bdeddce2303591a7f85ff2d018fabd3`
failed the separate restart/confirmation test in
[Rust run 34725450576](https://github.com/lightning-goats/lightning-goats/actions/runs/34725450576).
Its loop waited five seconds and then unconditionally indexed command zero.
The list was empty, producing an index-out-of-bounds panic. The reason no
command arrived during that particular CI window remains unproven.

That test now uses the same controlled initial unavailable-safety fixture and
allows a bounded fifteen-second observation with both child-liveness checks.
It obtains the UUID from the observed first command rather than indexing after
an exhausted loop. The fixture requires the real worker's safety retry before
the command can arrive. Restart, injected confirmation-database failure,
unchanged credit, same-UUID status recovery and no-resend assertions remain.
This corrects test observation; production timing and command admission are
unchanged. The failed run is retained and is not explained away by a rerun.
