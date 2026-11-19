# Lightning Goats stack audit, 2026-11-19

## Findings and regressions

* A populated overlay snapshot or roster refresh opened CyberHerd cards. New accepted Nostr interaction notices now carry the ranked public cards frozen with that interaction. The browser suppresses replay, omits effects from notices older than its server checkpoint, and clears member cards on a daily reset or empty live roster. Operator edits, payouts and ordinary roster messages have no card trigger. Previously queued notices retain their original receipt encoding. Browser and bridge regressions cover startup, replay, updates, escaping, exact sat amounts and highlighting.
* The socket's 16 KiB per-event rendering limit could reset clients receiving a valid CyberHerd message. The member notice path uses the bounded private bridge frame limit and retains the aggregate socket queue limit.
* A settled zap could block the ordered membership queue indefinitely when its author had never published a profile. A recent completed lookup with no profile permits the existing paid-join rules to run without a payout address. A failed lookup still waits. An ingress regression follows a paid join through the subsequent feed barrier.
* A batched profile lookup could use one author's successful write relay as evidence of a completed lookup for another author. The lookup now requires evidence that a relay listed by the particular author completed. The regression uses two disjoint outboxes and one completion.

## Scope and evidence

| Area | Paths and checks |
| --- | --- |
| Lightning intake | Lightning Address admission, invoice verification, authoritative Strike settlement, duplicate notification and restart tests in core CI |
| Cashu and Nutzaps | Wallet acquisition, receipt delivery, mint/correlation checks, melt recovery, independent Strike deposit verification and combined wallet/core fixtures |
| Accounting and payouts | Feed source attribution, frozen net Lightning proceeds, exact reservation and payout execution claims, independent result reconciliation |
| Physical feeder | Gateway safety read, serialized durable intent, correlation UUID, ambiguous-result recovery, disposable gateway and owner fixtures |
| CyberHerd and Nostr | Ordered ingress, verified profiles, reward lookup completion, frozen message rendering, isolated signing, publication and retry |
| Public clients | Index, payment controls, contact and roster views, OBS overlay, live/replay handling and browser geometry |
| Operations | Package and migration fixtures, backup and restore checks, systemd, nginx, isolated network and peer-routing jobs, dependency and SQLite vendor gates |

The [pull request checks](https://github.com/lightning-goats/lightning-goats/pull/152/checks) record each run against its exact revision. Fixture outcomes and live outcomes are separate. The live service-state inspection covered the receive, Cashu wallet, Cashu signer, CyberHerd membership, payout and signer services. Live settlement, physical actuation and community-funded Nutzap acceptance require independent transaction evidence.
