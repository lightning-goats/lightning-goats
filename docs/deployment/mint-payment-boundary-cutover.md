# Goats payment boundary cutover

## VPS claim worker

On 2026-11-23 at 16:45:52 UTC the VPS switched the inbound claim worker to the collector from main revision `d07a567` (PR #167, feature revision `a089d97`). Its previous entrypoint was `/opt/lightning-goats-project-collector/collector/worker.py`. The release directory is `/opt/lightning-goats-project-collector/releases/claims-a089d97`. The live database and pinned-key configuration paths come from the existing unit. This stage has no database migration.

1. Compare the installed programs with the release. Verify the Git blob IDs of `worker.py`, `claim_store.py`, `collect.py`, `project_quote.py` and `operator_proofs.py` against the merged revision. Install those five files in a root-owned revision directory. With the unit's Python runtime and module search path, run the staged worker with `--help` as the collector identity.
2. Retain the original executable path and install a service drop-in that resets `ExecStart` and selects the new worker. Keep the unit's existing arguments, identity, environment and filesystem controls. Run `systemctl daemon-reload` and restart only `lightning-goats-project-claims.service`.
3. Verify the effective executable path, service start time and restart count. Observe the public mint capability and match quote IDs, invoices, units and amounts against journal bindings without printing them. To confirm a payment, correlate the signed invoice hash, collected proofs and the application receipt. Count a feed credit only for its original eligible receipt.
4. To revert this stage, remove only the `60-nut29-a089d97.conf` drop-in, reload systemd, and restart the claims service. Resume from the current journal. A paid or issued quote can require the original output secrets even after invoice expiry.

The production mint advertised BOLT11 batch checks with a maximum of 1,000. A read-only status comparison returned two unpaid quotes and one issued quote, all bound to the expected journal records. An isolated mint with a simulated Lightning backend produced identical per-quote proofs using one batch status call and three individual status calls. The fixture covered a lost issuance reply and a collector restart.

## Legacy records to reconcile

The read-only VPS inventory on 2026-11-23 found 109 Strike invoice records (last issued 2026-11-21 17:05:39 UTC), 33 Strike settlement records, 109 scan entries and 90 completed webhook inbox entries. Invoice and settlement counts measure different events. An invoice's expiry and the absence of a webhook do not prove nonpayment. Before retiring the historical recovery path, check original invoice identities against authenticated provider records and replay late positive evidence through the existing uniqueness constraints.

Four Strike treasury intents for 1 sat each, issued on 2026-11-19, have closed delivery inspections. All four arrived in the isolated wallet journal. None has a prepared melt, bound melt request, wallet result, or recorded Strike deposit. Keep their identifiers and historical proof ownership records for reconciliation. Do not resend those invoices or transfer their amounts based on missing local evidence. The legacy payout provider journal had three submitted executions, each with a bound payment and a terminal completed record, and zero recorded conflicts.

## Raw token intake and website

The second VPS release uses main `d039400` (PR #168). Its CI archive tree matched the merge tree. The receive daemon and isolated Cashu wallet both passed their native configuration checks, then restarted in that order. Both units were active with zero automatic restarts after activation. The live descriptor advertised only `https://mint.lightning-goats.com`. Today's production policy therefore accepts Goats tokens. A foreign-token settlement test requires an approved foreign issuer with qualified exit and conversion funding.

The site retains its deployed Lightning LNURL and serves the new payment JavaScript under a content-hashed filename. The served index and script were compared byte-for-byte to the staged files. Restore the previous page from `/var/backups/lightning-goats/raw-token-d039400/index.html` for a UI rollback. The old daemon binaries are `/usr/local/bin/lightning-goatsd.pre-pr168` and `/usr/local/bin/lightning-goats-cashud.pre-pr168`. For a backend rollback, restore the old wallet first and then the old receiver, restarting the respective units. Preserve their current journals. There were no schema changes in this release.

One 1-sat BOLT11 from the live herd Lightning Address was decoded and verified as mainnet, reference `128620ddcb3b`. The collector recorded the invoice. Viewer signing was unavailable and no settlement, fee or feed credit has been verified for it.

### Live viewer Lightning settlement

On 2026-11-23 the operator reported paying 1 sat from Strike by scanning the stream overlay. The herd invoice was created at 17:06:35 UTC. A read-only comparison of both production journals matched shortened payment hash `fb1f17b19c40`. The deployed collector (`d07a567`) recorded issued proofs totaling 1 sat. The application (`d039400`) recorded verified collection, one 1,000-msat herd settlement and one +1 sat HERD_RECEIPT in the shared herd pool. The invoice amount and the collected amount matched exactly. The sender's routing fee is outside these journals. The earlier terminal invoice remained uncollected with zero ledger entries when checked. A separate quote-only diagnostic for that invoice returned HTTP 400 from the Goats mint because its project quotes require an external Lightning payment.

### Goats ecash contribution and collection fee

At 17:31:48 UTC on 2026-11-23 the live wallet acquired 21 sats of Goats ecash (receipt prefix `e7ca7781`), paid a 1-sat receive fee, and retained 20 spendable sats. The receiver at `d039400` credited 20 sats and published that amount. The operator authorized a feeder-only adjustment. The unique adjustment `cashu-receive-fee-e7ca7781` added 1 sat once, taking the shared pool from 21 to 22 sats with zero feeds due. Keep that adjustment and the original event on replay. The payout funding lot is still 20 sats.

Before rolling back gross contribution accounting, count retained receipts whose original ledger delta exceeds the acquired net amount. A pre-change receiver can read the old net-credit rows but cannot reconcile the newer gross-credit rows. Once any such row exists, use a rollback build that retains gross-or-net receipt verification and the original public event amount. Preserve the database, including funding lots, fee records and feeder adjustments.

PR #170 deployed to the receiver at 17:53:31 UTC on 2026-11-23. Main revision `4806664` and CI archive revision `e6671c5` had the same Git tree. The verified running receiver SHA-256 was `fe924e78c712adeadf70083a37e35512db81abd70a2a273dcceff736378b4236`. The previous receiver is at `/usr/local/bin/lightning-goatsd.pre-pr170`. The archive checksums, package smoke check and offline production configuration validation passed. The receiver, isolated wallet and claim worker were active after restart. The first inspection found zero gross-above-net receipt rows and exactly one 1-sat adjustment for the earlier receipt.

At 17:54:55 UTC receipt `ddbd78d0` verified the deployed change. The acquired Goats token had a gross value of 10 sats and a 1-sat receive fee. The wallet retained 9 sats. Exactly one ledger entry credited 10 sats, taking the pool from 22 to 32. Event 156905 carried 10 sats. At 17:54:59 UTC the signed-message outbox recorded note `67225ae67732` as published, announcing a gift of 10 sats and 968 sats until the next feeding.

### Verified zap settlement source

At 18:48:37 UTC on 2026-11-23 the VPS receive service activated main revision `19e02fe` (PR #171). The CI package and merged main had the same Git tree, `9f9270e6419c8d83abbc20db6d2d5b77c5d5d5e4`. All package checksums passed, and the staged CLI validated the installed receive configuration under the service account. The executable of the running process had SHA-256 `1c3b8965443b28ddc8840e6532b87fdfbbf12031e5a7c2395b47d3d25284502c`. The previous gross-credit-capable receiver is preserved at `/usr/local/bin/lightning-goatsd.pre-pr171`. This release has no schema change.

The loopback health check returned OK. Status reported active mode, a reachable gateway, all six Lightning Address users and 32 of 1,000 sats. The pool also held 32 sats immediately before restart. The typed settlement path passed the Rust zap, bridge and CyberHerd regression tests. A signed mainnet zap and its published kind-9735 receipt still need to be correlated on this revision.

## CyberHerd member reward delivery

This stage needs coordinated core, membership, payout, signer and collector releases. Quiesce writers and take a verified current backup before applying the additive migrations. Check the archive includes `member_publication.py`, the updated delivery worker and its Nostr wrapper. Install the accompanying `lightning-goats-member-publication.socket` and service on the collector host. The socket must grant access to the payout signer group and the service must verify the connecting signer UID. Its only source is the collector journal. The projection service requires Unix sockets only.

Enable that socket and restart the isolated delivery worker and updated signer before enabling member opt-ins. The payout worker requests a signature using the feed ID, member and frozen terms hash. The signer independently obtains the 9321 event fields from the collector and uses its existing payout NIP-46 client. Verify the returned signature against the project pubkey. Member selection, locking key, advertised mint spelling and proof amounts come from the original assignment. Preserve the signer journal, `member_publications` and the delivery outbox during rollback or recovery. Outstanding ecash assignments require a compatible recovery worker.

A public token-relay acknowledgement permits the member delivery receipt. Missing acknowledgements on other token or NIP-65 read relays retry the same signed event. For private delivery the worker publishes the original encrypted envelope to the frozen request's relays. Acknowledgement records relay acceptance. Recipient redemption requires separate evidence. Use an authenticated member who has opted in for a production payout test. Current acceptance evidence for member ecash comes from disposable wallets and relays.

### Fixed-amount checkout in production, 2026-11-23 UTC

The merged checkout revision is `424c8e44`. CI built the receiver and wallet from `31a20171`; the only source difference is the mixed-payment ledger regression. The installed process executables matched the verified release digests. The public index preserved its existing deployment customizations and loaded the new versioned checkout script.

A live POST for the herd froze 968 sats with 32 sats of credit toward a 1,000-sat feeding. Repeating the same checkout ID returned byte-identical Lightning and Cashu payment options. The collector had one original project invoice and zero issued proofs for that intent when verified. The redacted checkout correlation is `1da2865d2d9b`. A viewer payment and confirmed feeding are separate acceptance steps. For recovery, retain the original checkout and quote journals. The pre-checkout binaries and web files are in the protected VPS directory `/var/backups/checkout-172-xbpkmubz`.

### Treasury destination after direct-provider retirement

Configure the treasury destination for the Goats mint and use the existing project collector socket. The production worker selects only reservations already bound to that mint and wallet epoch. Historical reservations retain their original destination, signed invoice, funding lots and payment hash. Read or reconcile them under their original identity. Changing the configured mint cannot turn an old reservation into a project quote or create an additional feeder credit.

### VPS payout worker, 2026-11-23

The VPS installed the CyberHerd payout worker and service unit from main commit `91ab30df2972c5780b11ec8b778e14c22f4736ef` (PR #175). The release verifier checked all 114 package files. After restart the running executable matched SHA-256 `caf2dc572d4c29dfffe26b1c62d78dc448469e66369467197943a1d8ced387ad`, with zero automatic restarts. The service used retained Cashu funding and had no Strike payout API key mounted.

The ledger contained three historical Strike payment bindings before deployment. All three had a final completed payment record. The installer compared their original intent IDs, payment IDs and final states before and after restart and found them identical, with zero conflicts. The previous executable and unit are in `/var/backups/retire-direct-payout-175-7hqdl5c8` on the VPS. This release check involved no transfers. Live verification of a new opted-in member payout requires an eligible member and confirmed feeding.

### Member reward rollout, 2026-11-23 UTC

Production runs the member payout release merged at `8729c347`. The staged package was built from `2fa5876a`. Its only difference from the merged tree is the mixed-feed regression test. The running core, membership, payout and signer executables each matched the packaged SHA-256 digest.

Maintenance stopped all 11 writer services, the three activation sockets and the reclaim timer. Eleven verified SQLite backups are under the root-only directory `/var/backups/member-rewards-169-smbv9f37` on the VPS. The upgrade retained all original rows in 88 core, 46 membership and 41 payout tables. Schema versions after startup were 39, 14 and 15 respectively. All 15 original units resumed. The new signer projection socket accepted its installed signer UID and refused an empty request. The delivery worker was active and the reclaim service last exited successfully.

The public CyberHerd page and its versioned reward script returned HTTP 200. An unsigned POST to the member preference endpoint returned HTTP 401. The feeder reported 32 sats and zero feeds due. At rollout there were zero member preferences, zero new member dispositions and zero ready receipts. Live member ecash delivery still requires an explicitly opted-in recipient and a subsequent eligible feed. Do not backdate a preference or issue a replacement for a frozen allocation.

### Paid contact rollout, 2026-11-23 UTC

PR #173 deployed at main revision `e00e07e9`. The CI build `77648ce0` and merged revision shared Git tree `e931412826300feb8f39f8e6aeb1cfce452267f9`. The installer stopped 12 writer services, four activation sockets and the reclaim timer, verified 11 database backups and applied core migration 40. The migration preserved the original rows in all 89 tables, including private delivery row IDs. The previous programs, site files, configuration and journal backups are under `/var/backups/contact-173-a46dohbe`. All 17 units resumed. The running receiver and isolated wallet matched the packaged executable hashes.

The public contact endpoint returned HTTP 402 with a single-use 100-sat request accepting the Goats mint and requiring in-band tender. The request bound the original contact ID. A malformed tender returned HTTP 400. Retrying returned the same request bytes. A read-only journal check found one challenge and zero entitlements, tender rows, deliveries or Lightning invoice intents for redacted correlation `7a5441c414ea`. The public page loaded the verified `contact-fee-24deed3bc630.js` script. The funded check requires an actual 100-sat tender for the contact, verification of one entitlement and one accepted delivery, and zero feeder credit. Contributions sent to the feeder exercise a separate payment purpose.

### Treasury receiver release and historical invoice inventory

On 2026-11-23 the VPS installed the receiver from PR #176, merged at `31c7e590`. All 20 PR checks passed. CI built commit `5a93f4f6`. The only source difference from merged main was the historical settlement replay test. After restart the local health endpoint returned HTTP 200, the service reported zero automatic restarts and its running executable matched SHA-256 `66efb734722da99283e44634834d67dd59581459982d0eaed475e0a381086379`. The installed policy selected the Goats mint and project collector. The rollback binary is in `/var/backups/treasury-176-x3qdsi4f`. The release required no database migration.

The live ledger had 16 historical treasury intents bound to their original Strike invoices. Twelve had recorded deposits. Four had issued invoices for 1 sat each and lacked a recorded deposit. Preserve their original intent, invoice, payment hash, funding sources and wallet result for reconciliation. The absence of a deposit alone cannot establish whether a wallet submitted a payment. The installer compared every original binding and deposit before and after restart and found them identical.

For the four issued invoices without deposits, the original wallet journal contained four bound instructions, four quote requests and four source holds. It contained zero recorded quotes, split submissions, melt requests, melt replies or results for those IDs. The receiver-v2 journal contained no matching instructions or downstream records. Retain those holds pending reconciliation.

The operator also confirmed browser contact invoice issuance. The journal recorded one 100-sat contact invoice at 20:19:58 UTC, with zero fee entitlements and delivery jobs when inspected. Funded delivery remains pending.

### Late receipts on original Strike invoices

The 2026-11-23 read-only VPS inventory counted 109 original invoices and 109 scan cursor rows. Of 90 contribution invoices, 33 matched a retained Strike settlement by payment hash or original request ID. The remaining 57 had no matching settlement. The latest contribution invoice was issued on November 21. Three original contact invoices had zero recorded contact fees. The other 16 invoices were the treasury bindings listed above. All 90 inbox items were done, and every original Strike zap receipt job had a recorded relay publication. A scan cursor records polling history and is not evidence of a payment.

If a late contribution or paid contact is reported, use the private original request and receive UUIDs. Run `lightning-goatsctl --config /etc/lightning-goats/config.toml reconcile-historical-receive --request-id <original-request-uuid> --receive-id <original-receive-uuid>` in the operator maintenance context with the read-only `strike-api-key` systemd credential. This command applies the authenticated result. Verify the protected configuration points to the intended existing journal. The command validates the retained invoice, reads the original receive at the pinned provider and uses the existing atomic settlement path. A duplicate produces the existing outcome. Treasury reconciliation continues through `reconcile-cashu-settlement` with its wallet evidence and original approval. Retain the historical reader until the operator reconciles or closes outstanding original obligations.

The receiver configuration uses `[lnurl.collector]` for the protected project invoice socket and peer UID. Existing `[lnurl.breez_collector]` configurations remain readable. The receiver reports `payment_backend=cashu_mint`. New treasury reservations require the project mint destination. The operator recovery command validates the original digest-bound configuration separately so it can still reconcile pre-cutover treasury reservations.

### Rehearsing rollback with a funded journal

On 2026-11-23 the installed PR #176 receiver (SHA-256 `66efb734722da99283e44634834d67dd59581459982d0eaed475e0a381086379`) and its saved PR #173 predecessor (`db0e1f4a1a4482b0c8226994d0c3513fd774f2aedc1092702a227bc8b0c75d64`) ran in a disposable loopback namespace with the pinned local mint. The installed receiver credited a 7-sat signed LNURL invoice to Dexter. The previous executable reopened that journal and credited an additional 11-sat invoice. Restarting the installed release preserved both original invoices, settlement rows, proof claim digests, ledger entries and payment events. The collector held 18 fixture sats, feeder credit was 18 sats, and the ledger contained zero feed attempts. No production transactions were involved.

Repeat `deploy/breez-reserve/tests/receiver_rollback.py RELEASE_EXECUTABLE ROLLBACK_EXECUTABLE` under the pinned mint test environment with a non-root user in a fresh network namespace with loopback up. It creates its own core, collector and mint journals and uses the pre-cutover collector config spelling, understood by both release binaries.
