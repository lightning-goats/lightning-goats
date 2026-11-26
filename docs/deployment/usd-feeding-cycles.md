# US$1 feeding cycles

All six Lightning Addresses feed the shared herd. The ledger opens one
funding cycle at a time and sets its target to
`ceil(100,000,000 / BTC_USD)`. At US$80,000, a 1,250-sat target with
760 sats credited has 490 sats remaining. A later US$100,000 observation
can open a 1,000-sat successor. The original target stays at 1,250 sats
through refresh, restart, pause, and physical reconciliation.

## Price source and first rollout

On HOME, the trusted gateway obtains the state and `lastStateUpdate` from
`GET /rest/items/BTC_USD_Price` in one authenticated loopback read. Deploy
the reviewed gateway binary through the existing HOME gateway upgrade.
Preserve its WireGuard listener, OpenHAB credential, physical owner, and
gateway journal. From the VPS, confirm the exact narrow route and the
millisecond time of the source update.

```sh
curl --fail --silent --show-error http://10.8.0.6:8789/v1/btc-usd
```

Deploy the reviewed OBS and static site assets and the updated display cache
writer through the existing OBS release. Run its dry-run first and inspect
the static root and backup paths before applying it.

```sh
python3 deploy/scripts/deploy-obs-overlay.py "$CORE_ARCHIVE" "$SOURCE" "$CORE_SHA256"
sudo systemctl start lightning-goats-display.service
sudo -u lightning-goats jq '.market' /var/cache/lightning-goats-display/overlay-data.json
```

The core polls that exact cache independently of payments. It rejects rates
outside US$1–US$10,000,000 per BTC, timestamps more than five minutes old,
timestamps more than 30 seconds ahead, and observations that regress. Keep
the gateway route private to its existing peer ACL and the cache path
writable only by the display unit's identity.

## Schema 40 to 41

Qualify the exact release and paired native tooling first. Keep funding
cycle activation off until the checkout site, OBS, core, and any delegated
CyberHerd notice consumer can all read the new payload. Use the existing
[consistent snapshot](cyberherd-state-maintenance.md#consistent-snapshot)
procedure and the live installation inventory. Fence every original journal
writer and its activation sockets/timers, retain a fresh verified private
backup of the core and dependent journals, and preserve all unit states.
The current-schema program swap procedure alone cannot qualify a migration.

Rehearse the selected binary against a separate owned copy with an independently
verified schema-40 backup. Change only the configuration copy to point to the
isolated journal and use these settings under the existing `[feeder]` section.

```toml
threshold_sats = 1000
usd_cycle_pricing = true
```

Keep the old configured threshold if it differs. The retained
`feeder_credit_policy` value wins when present. The same binary, configuration
and commands apply to the fenced production journal after the copy rehearsal
and operator approval.

```sh
sudo unshare --net -- runuser -u lightning-goats -- \
  /usr/local/bin/lightning-goatsd --config /etc/lightning-goats/config.receive.toml \
  --initialize-feed-cycles-only --writers-stopped
sudo python3 deploy/scripts/verify-feeding-cycles.py \
  --database /var/lib/lightning-goats/lightning-goats.db --before "$SCHEMA_40_BACKUP_CORE"
```

Use the approved staged executable and configuration paths until installation.
Compare the paid row histories, signed bytes, pending reservations and retained
main/WAL state with the native backup and separate rehearsal evidence. Verify
that retrying initialization preserves cycle IDs. A funded legacy cycle keeps its
original threshold. Its pending or unknown physical attempt retains its UUID.
A partially funded legacy cycle finishes at its original target.

Resume the original configured service set under the existing physical override
and limits. Record the deployment commit, mode, queue, open cycle ID, exact
rate, observation time, opening time and remainder. Check both the website and
OBS against the public response.

```sh
curl --fail --silent --show-error https://lightning-goats.com/api/v1/status |
  jq '{payment_backend, feed_goal, unresolved_feed_attempt}'
sudo -u lightning-goats /usr/local/bin/lightning-goatsctl \
  --config /etc/lightning-goats/config.receive.toml status
```

## Pause, replay and rollback

The physical override holds dispatch while verified payments still accrue.
If the price goes stale, continue allocating into the locked cycle. With
no successor price, keep excess credit in receipt order and display
“Waiting for a BTC/USD price”. Resume allocation from the original receipt
when a fresh observation arrives. It can fund at most three cycles including
the one it finishes. Excess is recorded as `FEED_CREDIT_RETIRED` against that
receipt. A funded cycle may still wait for original foreign-mint custody to
be converted and verified. The physical reservation and payouts use actual
eligible funding lots.

For rollback after schema 41 is applied, stop and fence writers and take a
new accepted protected backup with the selected tooling. The selected
daemon's `--recovery-paused` mode can serve read-only status. Keep all paid
rows, current cycle assignments, receipt retirement history, funding plans,
and physical UUIDs. Resume with a reviewed cycle-aware build. A fixed-target
binary cannot take over dispatch from this journal. Before schema application
and writer resumption, a binary-only cutback is possible using the existing
program rollback procedure and the untouched schema-40 journal.

## Live acceptance

Use a project-authorized wallet. Record a redacted original receipt reference,
actual amount and fees, and cycle credit before and after. Account for other
settlements between the two observations. Verify that a fresh source update
leaves the open ID and target fixed. Wait for a genuine completion or use the
live remaining amount with explicit operator authorization. Correlate one
confirmed gateway UUID, one cycle and one exact physical debit. Verify the next
cycle with the verifier and its stored observation even if the target repeats.
If a payment response is lost, establish the original invoice or receipt state
before issuing or paying anything again.
