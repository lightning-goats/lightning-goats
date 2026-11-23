"""Offline HTTP contract for project quote polling and legacy fallback."""
import sys
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import httpx

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "collector"))
from worker import QuotePoller, quote_state


class QuotePollingTests(unittest.TestCase):
    rows = [("a", "inv-a", 2), ("b", "inv-b", 3), ("c", "inv-c", 4)]

    def run_poll(self, capability, handler, bad=None):
        calls, results = [], []
        quotes = {}
        for (key, invoice, amount), paid, issued in zip(self.rows, [0, 3, 4], [0, 0, 4]):
            quotes["goats-" + key] = dict(quote="goats-" + key, request=invoice,
                                         amount=amount, unit="sat", amount_paid=paid, amount_issued=issued)
        def respond(request):
            import json
            path = request.url.path
            calls.append(path)
            if path == "/v1/info":
                return httpx.Response(200, json={"nuts": capability})
            if path.endswith("/check"):
                ids = json.loads(request.content)["quotes"]
                status, result = handler([quotes[q] for q in ids])
                return httpx.Response(status, json=result)
            quote_id = path.rsplit("/", 1)[-1]
            if quote_id == bad:
                return httpx.Response(404)
            return httpx.Response(200, json=quotes[quote_id])

        def observe(store, client, request_id, keys, observation):
            key = "goats-" + request_id
            quote = observation or client.get("/v1/mint/quote/bolt11/" + key).raise_for_status().json()
            results.append(quote_state(quote, key, "inv-" + request_id, quotes[key]["amount"]))

        store = Mock()
        store.reserve_polls.return_value = self.rows
        with httpx.Client(base_url="https://fixture", transport=httpx.MockTransport(respond)) as client:
            with patch("worker.collect", side_effect=observe):
                self.assertTrue(QuotePoller().poll(store, client, {}, 100))
        return calls[1:], results

    def test_batch_and_fallback(self):
        caps = {"29": {"methods": ["bolt11"]}}
        healthy, expected = self.run_poll(caps, lambda q: (200, q))
        self.assertEqual(healthy, ["/v1/mint/quote/bolt11/check"])
        self.assertEqual(expected, ["UNPAID", "PAID", "ISSUED"])
        for response in (lambda q: (200, q[::-1]), lambda q: (200, q[:1]),
                         lambda q: (200, [dict(item, unit="usd") for item in q])):
            calls, observed = self.run_poll(caps, response)
            self.assertEqual(len(calls), 4)
            self.assertEqual(observed, expected)
        calls, observed = self.run_poll({}, None)
        self.assertEqual(len(calls), 3)
        self.assertEqual(observed, expected)
        calls, observed = self.run_poll(caps, lambda q: (400, {"code": 11017}))
        self.assertEqual(len(calls), 4)
        self.assertEqual(observed, expected)
        _, observed = self.run_poll(caps, lambda q: (400, {}), bad="goats-b")
        self.assertEqual(observed, ["UNPAID", "ISSUED"])

    def test_legacy_and_conflicting_accounting(self):
        terms = dict(quote="goats-a", request="inv", state="PAID")
        self.assertEqual(quote_state(terms, "goats-a", "inv", 12), "PAID")
        for changes in [dict(amount=11), dict(amount_paid=0, amount_issued=0),
                        dict(amount_paid=12), dict(amount_paid=True, amount_issued=False)]:
            with self.assertRaises(ValueError):
                quote_state(dict(terms, **changes), "goats-a", "inv", 12)

if __name__ == "__main__":
    unittest.main()
