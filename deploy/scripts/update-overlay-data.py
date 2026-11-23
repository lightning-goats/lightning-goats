#!/usr/bin/env python3
"""Refresh presentation-only weather and BTC data. No financial credentials/state.

Weather comes only from the approved narrow gateway. Market data is indicative;
this process cannot set feed prices, credit payments, sign Nostr or actuate feeds.
"""
import argparse
from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import tempfile
import time
import urllib.request

WEATHER_URL = "http://10.8.0.6:8789/v1/weather"
MARKET_URL = ("https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd"
              "&include_24hr_change=true&include_last_updated_at=true")
MAX_BODY = 16384


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def read_json(url):
    if url not in (WEATHER_URL, MARKET_URL):
        raise ValueError("unapproved presentation source")
    request = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "Lightning-Goats-Overlay/1.0"})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open(request, timeout=8) as response:
        if response.status != 200 or response.headers.get_content_type() != "application/json":
            raise ValueError("unexpected presentation response")
        length = response.headers.get("Content-Length")
        if length and int(length) > MAX_BODY:
            raise ValueError("presentation body too large")
        body = response.read(MAX_BODY + 1)
        if len(body) > MAX_BODY:
            raise ValueError("presentation body too large")
    return json.loads(body)


def number(value, low, high):
    if isinstance(value, bool) or not isinstance(value, (float, int)) or not math.isfinite(value) or not low <= value <= high:
        raise ValueError("invalid display number")
    return value


def epoch(raw):
    if not isinstance(raw, str):
        raise ValueError("observation time missing")
    parsed = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("observation time needs timezone")
    return parsed.timestamp()


def fresh(observed, now):
    if not math.isfinite(observed) or now - observed > 300 or observed - now > 30:
        raise ValueError("stale or future display observation")


def weather_payload(data, now):
    observed = epoch(data.get("observed_at"))
    fresh(observed, now)
    result = {"observed_at": datetime.fromtimestamp(observed, timezone.utc).isoformat()}
    for key, limits in {"temperature_f": (-100, 150), "humidity": (0, 100),
                        "wind_speed_mph": (0, 300), "uv_index": (0, 30)}.items():
        if data.get(key) is not None:
            result[key] = number(data[key], *limits)
    return result


def market_payload(data, now):
    quote = data["bitcoin"]
    observed = number(quote.get("last_updated_at"), 0, 10**11)
    fresh(observed, now)
    result = {"price_usd": number(quote.get("usd"), 1, 10000000),
              "observed_at": datetime.fromtimestamp(observed, timezone.utc).isoformat(),
              "source": "CoinGecko"}
    change = quote.get("usd_24h_change")
    if change is not None:
        result["change_24h_percent"] = number(change, -100, 10000)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("/var/cache/lightning-goats-display/overlay-data.json"))
    args = parser.parse_args()
    result = {"weather": None, "market": None}
    for name, url, normalize in (("weather", WEATHER_URL, weather_payload), ("market", MARKET_URL, market_payload)):
        try:
            result[name] = normalize(read_json(url), time.time())
        except (OSError, ValueError, KeyError, TypeError, OverflowError):
            # No raw provider body, URL parameters or upstream error text logged.
            print(f"{name}: unavailable")
    result["generated_at"] = datetime.now(timezone.utc).isoformat()
    target = args.output
    if target.is_symlink() or target.parent.is_symlink() or not target.parent.is_dir():
        raise ValueError("unsafe/missing cache destination")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=target.parent, prefix=".overlay-", delete=False) as output:
            temporary = Path(output.name)
            json.dump(result, output, separators=(",", ":"), allow_nan=False)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
            os.fchmod(output.fileno(), 0o644)
        os.replace(temporary, target)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
