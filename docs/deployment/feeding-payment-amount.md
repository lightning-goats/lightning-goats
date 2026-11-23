# Triggering payment amount in feeding announcements

Confirmed feeding messages now use the amount of the contribution that completed
the feeding's funding threshold. A 10-sat payment taking saved credit from 995 to
1,005 sats is described as the 10-sat triggering payment. The confirmed feeding
still debits 1,000 sats and leaves five sats. This is presentation metadata, not a
change to the feeder threshold, wallet policy or physical authority.

When confirming a feeding, the existing transaction saves an optional
`triggering_payment_amount_sats` alongside its existing threshold/balance fields.
A bounded reverse walk removes the newest still-unspent credits and identifies
the credit that completed this FIFO threshold. A later payment arriving during
dispatch cannot steal attribution. Each feeding funded by a large payment retains
that original payment amount. CLI test grants use the same existing normal wording.

Both Nostr and overlay rendering use the saved amount. Wording that previously
said that amount was redeemed now says the payment triggered the feeding, avoiding
a false claim that the payment amount equals the actual feeding debit.

Legacy confirmations or unavailable attribution use a neutral confirmed-feeding
message without claiming that the threshold was a payment. The lookup examines at
most 4,096 recent positive credit entries; exhausted/missing history does not block
confirmation or invent an amount. Existing signed messages retain their bytes and
IDs. No event, credit, debit, cursor or outbox history is rewritten.

The patch needs no schema or dependency change. Tests cover small threshold-crossing
payments, interleaved later payments, restart, a single payment funding multiple
feedings, bounded fallback and both render targets. Production checks/deployment
must use this isolated branch based on the installed messaging source; do not
install the unfinished CyberHerd/native-zap branch for this change.
