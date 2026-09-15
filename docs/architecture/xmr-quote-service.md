# XMR quote service (#96)

This implements a quote lifecycle on top of #102/#103. It does not install any
public HTTP route, poll a wallet, create a MoneroPay address, grant credit or alter
the running Strike pilot. The daemon does not construct the service unless a future
#98 integration explicitly configures it. No live source or monetary policy is
silently enabled by linking this module.

## Lifecycle and immutable contract

`QuoteService::create` validates a trusted context and an allowed goat/target. It
reserves the request in SQLite BEFORE contacting the read-only rate source. The
reservation is serialized with other quote creators (`BEGIN IMMEDIATE`) and binds
request UUID, target, goat, client bucket, provider, network and account. There is
no provider I/O while a database write lock is held. A duplicate in-flight request
returns in-progress instead of issuing another oracle call. A completed request
returns its exact stored terms without contacting the oracle, even after expiry or
policy changes. Expiry remains visible in the response; it does not mean repricing.

The internal UUID is NOT authorization. #98 must issue an independent status
capability and derive the opaque 64-hex client bucket from trusted ingress context,
not accept a browser-selected bucket. No caller-controlled upstream URL exists.
Keep create/status same-origin and apply ingress per-client/global read rates,
method/body/deadline limits and capacity authorization before exposing these APIs.
The library enforces durable creation budgets and four nonqueued operations per
service instance, not a substitute for those HTTP controls.

A failed request remains failed. A cancelled process leaves a 30-second reservation;
once expired it no longer consumes pending capacity, but the original ID can never
be silently reused/repriced. An explicit new request requires a new UUID and budget.
A late result cannot commit after reservation expiry. A conditional failure update
cannot replace a quote whose commit succeeded but reply was lost. These are quote
reservations, not physical-owner leases; they authorize no actuator action.

Stored quote documents include original rational rate/source/observation time,
local fetch time, bounded raw evidence and its SHA-256, exact atomic requested
amount, target credit, expiry and a snapshot of policy. Fields are private and
revalidated on read. `terms_json()` deliberately exposes only version/quote ID,
asset, exact decimal XMR/atomic amount, target credit and lifetime. It contains NO
receive address, wallet/account identifier, client bucket or raw oracle evidence.
Its bytes remain identical for the same quote. It is not a payable invoice and
must not be broadcast as a successful-payment announcement.

`bind` is a trusted internal call made AFTER #97 verifies a canonical receive
address. The quote must still be valid for a NEW binding. Registration of the #95
credit valuation and the immutable quote-address binding share one transaction;
either both exist or neither does. An exact existing binding remains retrievable
after expiry; a different address cannot replace it. #97 still owns lost-address-
creation recovery and must not expose an address/URI until this transaction commits.
If the upstream response arrives after expiry, preserve its private mapping and
monitor any funds; do not publish it as an in-window payable quote or issue a refund.

Timely receipts may unlock after expiry. Each later top-up has its own trusted
first-seen eligibility; an early partial does not extend the quote. The existing
#95 receipt API preserves partial/dust/late funds and uses cumulative rounding.
Quote expiry never deletes a payment, reverses a feed or stops receipt monitoring.

## Policy and rate safety

There are no production defaults for target amounts, credit ceiling, XMR size,
rate sanity range, quote lifetime, maximum market-data age, creation-rate window,
per-client/global/pending/retained request limits. `QuotePolicy::validate` rejects
inconsistent/zero policies. One-day maxima on time inputs are technical bounds,
not recommended deployment values. Select practical values explicitly at activation.
The fixed 10-second source deadline fits inside the 30-second durable reservation.

Creation quotas are shared through SQLite and survive process restart. Failures
count toward the creation budget. The retained-request cap fails closed instead
of deleting expired/failed identities to regain room; reviewed archival is future
work. Quotes, bindings, request identities and rate high-water marks cannot be
silently overwritten/deleted. A backward clock blocks new issuance/binding rather
than extending expiry or resetting budgets. An older market observation, or a
different rate for the same source timestamp, is rejected across process restarts.
No fallback to stale rates and no revaluation of prior credit exists. Exact retries
and BTC identity settlement do not depend on an available oracle.

## Read-only Kraken candidate

`KrakenOhlcOracle` uses only a fixed unauthenticated HTTPS GET:

```text
https://api.kraken.com/0/public/OHLC?pair=XMRXBT&interval=1
```

It expects the legacy response pair key `XXMRXXBT`. A different/missing pair, error
response, malformed/duplicate schema, non-string price, inconsistent OHLC/volume,
nonordered/future candle, redirect, wrong content type, oversized/chunked body or
timeout fails closed. No trading, account, key, transfer or caller-supplied endpoint
is supported. Environment proxies and redirects are disabled. Tests substitute a
loopback endpoint only inside the module's test build.

The candidate uses the newest nonempty CLOSED one-minute candle's VWAP, denominated
in BTC per XMR. Kraken documents that the final OHLC row is still forming, so that
row is excluded. The candle START is the conservative market-observation timestamp;
fetch time never renews old data. Source is `kraken-xmrbtc-closed-vwap-v1`.
Decimal strings are parsed as bounded integer fractions and multiplied by 10^8
sats/BTC. Floats, scientific notation, signs, excessive precision and overflow are
rejected. Requested piconero round up; #102's agreed target/atomic ratio guarantees
an exact full payment earns exactly the requested credit. This is valuation, not
an executable exchange quote and not a claim that XMR has been sold.

Primary references inspected 2026-09-15:
- [Kraken OHLC documentation](https://docs.kraken.com/api-reference/market-data/get-ohlc-data): tuple schema, price strings, timestamps, final unfinished row, up to 720 entries.
- [Public endpoint access](https://support.kraken.com/hc/articles/360000919986-public-endpoint-examples-you-can-try-them-directly-in-a-web-browser-): public data requires no Kraken account.
- [Kraken terms](https://www.kraken.com/legal/global-terms), especially content/use restrictions: public accessibility is not proof of an unrestricted redistribution license.

The live XMR pair endpoint could not be independently fetched in this implementation
session. No real market-response conformance or unlimited data-use permission is
claimed. Before activating this candidate, confirm exact pair availability/schema
from the VPS and the intended private valuation/derived-amount use under applicable
provider terms. Do not publish raw market data as a feed. A different source may
implement `RateProvider`; source replacement requires an explicit policy change,
not an automatic fallback. These provider choices do not block the Strike pilot.

## Tests and remaining integration

Synthetic file-backed tests cover exact retries/reopen, immutable inputs and policy,
request/price/size limits, failed/cancelled/late requests, concurrent creators,
shared creation budgets, clock/rate regression, expiry and binding, transaction
fault injection and actual #95 partial/late credit behavior. Parser/loopback tests
cover money precision, source shape, closed-candle selection and transport bounds.
No live oracle, MoneroPay, real credentials, payment or physical action is used.

#97/#98 still supply authenticated receive bindings, first-seen/finality evidence,
complete history reconciliation, public capabilities and HTTP admission/config.
#99 implements the operator-approved native-plus-sats announcements (BTC sats-only).
#100 covers the full provider/daemon/gateway restore path and host activation.
Back up and quiesce all writers before a live schema upgrade. Retain the quote tables
with the receipt/ledger/bridge mapping state on restore; never reset paid state or
use stale quote evidence to authorize a new physical retry.
