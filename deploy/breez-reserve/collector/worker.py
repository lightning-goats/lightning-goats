"""Poll outstanding Breez project invoices and claim their Cashu proofs."""

import argparse
import json
import logging
import time
from pathlib import Path

import httpx

from claim_store import ClaimStore
from project_quote import recover_invoice
from collect import collect, quote_state, recover_payout_refund


class QuotePoller:
    """One status capability cache per mint client. Each claim keeps its own outputs."""

    def __init__(self):
        self.limit = 1
        self.refresh_after = 0

    def refresh(self, client, now):
        info = client.get("/v1/info").raise_for_status().json()
        nuts = info.get("nuts", {}) if isinstance(info, dict) else {}
        caps = nuts.get("29") if isinstance(nuts, dict) else None
        limit = 1
        if isinstance(caps, dict) and caps.get("supported", True) is True and not caps.get("disabled", False):
            methods = caps.get("methods")
            if methods is None:
                nut4 = nuts.get("4", {})
                entries = nut4.get("methods", []) if isinstance(nut4, dict) else []
                methods = [m.get("method") for m in entries if isinstance(m, dict)]
            if isinstance(methods, list) and "bolt11" in methods:
                maximum = caps.get("max_batch_size", 16)
                if type(maximum) is int and maximum > 0:
                    limit = min(maximum, 32)
        self.limit = limit
        self.refresh_after = now + 300

    def poll(self, store, client, keys, now):
        rows = store.reserve_polls(now, 15, 32)
        if not rows:
            return False
        if now >= self.refresh_after:
            try:
                self.refresh(client, now)
            except (httpx.HTTPError, ValueError):
                self.limit = 1
                self.refresh_after = now + 30
        for request_id, invoice, _ in rows:
            if invoice is None:
                try:
                    recover_invoice(store, client, request_id)
                    collect(store, client, request_id, keys)
                except (httpx.TransportError, httpx.HTTPStatusError) as error:
                    if isinstance(error, httpx.TransportError) or error.response.status_code in (429, 502, 503, 504):
                        raise
                    logging.warning("Original project quote lookup deferred (%s)", type(error).__name__)
                except (KeyError, ValueError, TypeError) as error:
                    logging.warning("Original project quote binding deferred (%s)", type(error).__name__)
        rows = [row for row in rows if row[1] is not None]
        while rows:
            group = rows[:self.limit]
            observations = [None] * len(group)
            if len(group) > 1:
                ids = ["goats-" + row[0] for row in group]
                reply = client.post("/v1/mint/quote/bolt11/check", json={"quotes": ids})
                try:
                    body = reply.json()
                except ValueError:
                    body = None
                if isinstance(body, dict) and body.get("code") == 11017:
                    self.limit = max(1, len(group) // 2)
                    continue
                if reply.status_code in (429, 502, 503, 504):
                    reply.raise_for_status()
                if reply.is_success and isinstance(body, list) and len(body) == len(group):
                    try:
                        for value, quote_id, (_, invoice, amount) in zip(body, ids, group):
                            quote_state(value, quote_id, invoice, amount, batch=True)
                    except ValueError:
                        pass
                    else:
                        observations = body
            for (request_id, _, _), observation in zip(group, observations):
                try:
                    collect(store, client, request_id, keys, observation=observation)
                except (httpx.TransportError, httpx.HTTPStatusError) as error:
                    if isinstance(error, httpx.TransportError) or error.response.status_code in (429, 502, 503, 504):
                        raise
                    logging.warning("Project invoice recovery deferred (%s)", type(error).__name__)
                except (KeyError, ValueError, TypeError) as error:
                    logging.warning("Project invoice recovery deferred (%s)", type(error).__name__)
            rows = rows[len(group):]
        return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-directory", type=Path, required=True)
    parser.add_argument("--pinned-keysets", type=Path, required=True)
    args = parser.parse_args()
    keys = json.loads(args.pinned_keysets.read_text())
    store = ClaimStore(args.state_directory)
    with httpx.Client(base_url="https://mint.lightning-goats.com", timeout=10,
                      follow_redirects=False, trust_env=False) as client:
        poller = QuotePoller()
        refund_cursor = ""
        next_refund = 0
        while True:
            if time.monotonic() >= next_refund:
                refund = store.db.execute("""
                    SELECT disposition_id,json_extract(change_proofs,'$[0].id')
                    FROM payout_melt_result r WHERE json_array_length(change_proofs)>0
                      AND NOT EXISTS (SELECT 1 FROM payout_claim_refund c
                                      WHERE c.disposition_id=r.disposition_id)
                    ORDER BY disposition_id<=?,disposition_id LIMIT 1
                """, (refund_cursor,)).fetchone()
                if refund:
                    refund_cursor, keyset = refund
                    try:
                        recover_payout_refund(store, client, refund_cursor, keyset, keys)
                    except (httpx.HTTPError, ValueError, KeyError, TypeError) as error:
                        logging.warning("Payout change recovery deferred (%s)", type(error).__name__)
                next_refund = time.monotonic() + 15
            try:
                if not poller.poll(store, client, keys, int(time.time())):
                    time.sleep(1)
            except (httpx.HTTPError, ValueError, KeyError, TypeError) as error:
                logging.warning("Project invoice polling deferred (%s)", type(error).__name__)
                time.sleep(15)


if __name__ == "__main__":
    main()
