"""Presentation cache tests: real observations, no financial state or credentials."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("overlay_data", Path(__file__).resolve().parents[1] / "scripts/update-overlay-data.py")
DATA = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DATA)
NOW = 1794175725


class OverlayDataTests(unittest.TestCase):
    def test_weather_uses_actual_observation_and_omits_unrecognized_fields(self):
        weather = {"observed_at": "2026-11-08T22:08:30Z", "temperature_f": 65.66,
                   "humidity": 33, "arbitrary_private_field": "never forwarded"}
        result = DATA.weather_payload(weather, NOW)
        self.assertEqual(result["temperature_f"], 65.66)
        self.assertNotIn("arbitrary_private_field", result)
        self.assertEqual(DATA.epoch(result["observed_at"]), DATA.epoch(weather["observed_at"]))

    def test_stale_future_naive_and_invalid_weather_rejected(self):
        for timestamp in ("2020-01-01T00:00:00Z", "2030-01-01T00:00:00Z", "2026-11-08T22:08:30", None):
            with self.subTest(timestamp=timestamp), self.assertRaises(ValueError):
                DATA.weather_payload({"observed_at": timestamp}, NOW)
        for value in (True, float("nan"), float("inf"), 300, "65.66"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                DATA.weather_payload({"observed_at": "2026-11-08T22:08:30Z", "temperature_f": value}, NOW)

    def test_market_preserves_exact_openhab_observation_for_server_pricing(self):
        quote = {"source": DATA.MARKET_SOURCE, "observed_at": "2026-11-08T22:08:30.125Z",
                 "price_usd_exact": "79999.999999999999999999"}
        result = DATA.market_payload(quote, NOW)
        self.assertEqual(result["price_usd_exact"], quote["price_usd_exact"])
        self.assertEqual(result["price_usd"], 80000)
        self.assertEqual(result["observed_at"], quote["observed_at"])

    def test_market_rejects_bad_source_timestamp_and_price(self):
        for changes in ({"observed_at": "2020-01-01T00:00:00Z"},
                        {"observed_at": "2030-01-01T00:00:00Z"},
                        {"observed_at": None}, {"source": "unapproved"},
                        {"price_usd_exact": "0"}, {"price_usd_exact": True},
                        {"price_usd_exact": "NaN"}, {"price_usd_exact": "10000000.1"}):
            quote = {"source": DATA.MARKET_SOURCE, "observed_at": "2026-11-08T22:08:30Z",
                     "price_usd_exact": "74184", **changes}
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                DATA.market_payload(quote, NOW)

    def test_only_fixed_read_only_origins_permitted(self):
        for url in ("http://10.8.0.6:8080/rest/items", "http://10.8.0.6:5000/weather", "https://example.com"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                DATA.read_json(url)

    def test_redirects_are_not_followed(self):
        self.assertIsNone(DATA.NoRedirect().redirect_request(None, None, 302, "redirect", {}, DATA.WEATHER_URL))
