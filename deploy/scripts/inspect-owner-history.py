#!/usr/bin/env python3
"""Validate an exported owner JDBC response offline. Never authorize feeding.

This diagnostic performs no network requests, database writes or commands. A
persisted complete row is an observation, not an irrevocable owner receipt.
"""
import argparse
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import time
import uuid

MAX_BODY = 262144
MAX_ROWS = 64
MAX_LEDGER = 8192
MAX_ENTRIES = 32
ITEM = "GoatFeeder_ManualRequest"
VERSION = "feeder-request-ledger/v1"


@dataclass(frozen=True)
class HistoryReport:
    records: int
    matching_records: int
    latest_observation: str
    completion_is_authoritative: bool = False
    release_reservation: bool = False
    reason: str = "Owner may replace persisted complete with failed; retain UUID and never resend."


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def decode(raw):
    try:
        return json.loads(raw, object_pairs_hook=unique_object)
    except (json.JSONDecodeError, UnicodeError, RecursionError) as error:
        raise ValueError("invalid JSON") from error


def timestamp_ms(value):
    if not isinstance(value, str) or not re.fullmatch(
        r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)", value
    ):
        raise ValueError("invalid ledger timestamp")
    try:
        stamp = datetime.fromisoformat(value.replace("Z", "+00:00"))
        delta = stamp - datetime(1970, 1, 1, tzinfo=timezone.utc)
        return delta.days * 86400000 + delta.seconds * 1000 + delta.microseconds // 1000
    except ValueError as error:
        raise ValueError("invalid ledger timestamp") from error


def inspect(raw, request_id, *, now_ms=None):
    target = str(uuid.UUID(request_id))
    if len(raw) > MAX_BODY:
        raise ValueError("history body exceeds limit")
    root = decode(raw)
    if not isinstance(root, dict) or set(root) != {"name", "datapoints", "data"}:
        raise ValueError("unexpected history schema")
    rows = root["data"]
    count = root["datapoints"]
    if root["name"] != ITEM or not isinstance(rows, list) or len(rows) > MAX_ROWS:
        raise ValueError("wrong Item or row limit exceeded")
    if not isinstance(count, str) or not re.fullmatch(r"0|[1-9][0-9]{0,3}", count) or int(count) != len(rows):
        raise ValueError("datapoints must equal returned row count")
    now_ms = time.time_ns() // 1000000 if now_ms is None else now_ms
    previous_time = -1
    previous_state = None
    observations = []
    for row in rows:
        if not isinstance(row, dict) or set(row) != {"time", "state"}:
            raise ValueError("unexpected persistence row schema")
        recorded, state = row["time"], row["state"]
        if type(recorded) is not int or recorded < 0 or recorded > now_ms + 30000:
            raise ValueError("invalid persistence timestamp")
        if recorded < previous_time or (recorded == previous_time and state != previous_state):
            raise ValueError("unordered or contradictory same-time records")
        if not isinstance(state, str) or len(state.encode("utf-8")) > MAX_LEDGER:
            raise ValueError("ledger exceeds byte limit")
        ledger = decode(state)
        if not isinstance(ledger, dict) or set(ledger) != {"version", "entries"} or ledger["version"] != VERSION:
            raise ValueError("unexpected ledger schema/version")
        entries = ledger["entries"]
        if not isinstance(entries, list) or len(entries) > MAX_ENTRIES:
            raise ValueError("ledger entry limit exceeded")
        seen = set()
        for entry in entries:
            required = {"requestId", "status", "reason", "at"}
            if not isinstance(entry, dict) or not required <= set(entry) or set(entry) - required - {"updatedAt"}:
                raise ValueError("unexpected ledger entry schema")
            identity, status, reason = entry["requestId"], entry["status"], entry["reason"]
            if not isinstance(identity, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]{7,127}", identity) or identity in seen:
                raise ValueError("invalid or duplicate ledger identity")
            seen.add(identity)
            if not isinstance(status, str) or status not in {"accepted", "running", "complete", "denied", "failed"} or not isinstance(reason, str):
                raise ValueError("invalid ledger outcome")
            admitted = timestamp_ms(entry["at"])
            updated = timestamp_ms(entry["updatedAt"]) if "updatedAt" in entry else admitted
            if admitted < 0 or updated < admitted or updated > recorded:
                raise ValueError("contradictory ledger timestamps")
            if status in {"complete", "failed", "denied"} and "updatedAt" not in entry:
                raise ValueError("terminal entry lacks update timestamp")
            if status == "complete" and reason != "complete":
                raise ValueError("contradictory completion reason")
            if identity == target:
                observations.append(status)
        previous_time, previous_state = recorded, state
    # Even the newest returned complete row cannot prove owner finalization or
    # complete query coverage. Do not turn absence into a no-dispatch result.
    return HistoryReport(len(rows), len(observations), observations[-1] if observations else "absent")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("response", type=Path)
    parser.add_argument("request_id")
    args = parser.parse_args()
    try:
        with args.response.open("rb") as stream:
            report = inspect(stream.read(MAX_BODY + 1), args.request_id)
    except (ValueError, OSError) as error:
        # Avoid dumping raw history or parser context to operational logs.
        parser.exit(2, f"History not accepted: {type(error).__name__}\n")
    print(json.dumps(asdict(report), indent=2))


if __name__ == "__main__":
    main()
