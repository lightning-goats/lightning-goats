"""Poll outstanding Breez project invoices and claim their Cashu proofs."""

import argparse
import json
import logging
import time
from pathlib import Path

import httpx

from claim_store import ClaimStore
from collect import collect, recover_payout_refund


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-directory", type=Path, required=True)
    parser.add_argument("--pinned-keysets", type=Path, required=True)
    args = parser.parse_args()
    keys = json.loads(args.pinned_keysets.read_text())
    store = ClaimStore(args.state_directory)
    with httpx.Client(base_url="https://mint.lightning-goats.com", timeout=10,
                      follow_redirects=False, trust_env=False) as client:
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
            request_id = store.reserve_poll(int(time.time()), 15)
            if request_id is None:
                time.sleep(1)
                continue
            try:
                collect(store, client, request_id, keys)
            except (httpx.HTTPError, ValueError, KeyError, TypeError) as error:
                # Provider errors may embed the quote or blinded message in their URL/body.
                logging.warning("Project invoice collection deferred (%s)", type(error).__name__)


if __name__ == "__main__":
    main()
