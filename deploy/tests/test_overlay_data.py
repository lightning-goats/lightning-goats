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

    def test_market_is_timestamped_usd_and_change_is_optional(self):
        quote = {"bitcoin": {"last_updated_at": NOW, "usd": 74184, "usd_24h_change": -1.95}}
        result = DATA.market_payload(quote, NOW)
        self.assertEqual(result["price_usd"], 74184)
        self.assertEqual(result["change_24h_percent"], -1.95)
        self.assertEqual(DATA.epoch(result["observed_at"]), NOW)
        quote["bitcoin"]["usd_24h_change"] = None
        self.assertNotIn("change_24h_percent", DATA.market_payload(quote, NOW))

    def test_market_stale_future_nonfinite_boolean_or_invalid_change_rejected(self):
        for changes in ({"last_updated_at": NOW-301}, {"last_updated_at": NOW+31},
                        {"usd": 0}, {"usd": True}, {"usd": float("nan")},
                        {"usd_24h_change": -101}):
            quote = {"last_updated_at": NOW, "usd": 74184, "usd_24h_change": -1.95, **changes}
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                DATA.market_payload({"bitcoin": quote}, NOW)

    def test_only_fixed_read_only_origins_permitted(self):
        for url in ("http://10.8.0.6:8080/rest/items", "http://10.8.0.6:5000/weather", "https://example.com"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                DATA.read_json(url)

    def test_redirects_are_not_followed(self):
        self.assertIsNone(DATA.NoRedirect().redirect_request(None, None, 302, "redirect", {}, DATA.WEATHER_URL))
