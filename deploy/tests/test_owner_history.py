"""The inspected owner can persist complete before later persisting failure."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("owner_history", ROOT / "deploy/scripts/inspect-owner-history.py")
HISTORY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = HISTORY
SPEC.loader.exec_module(HISTORY)
TARGET = "00000000-0000-0000-0000-000000000001"
FIXTURE = ROOT / "tests/fixtures/openhab/jdbc-history-sanitized.json"


class OwnerHistoryTests(unittest.TestCase):
    def setUp(self):
        self.value = json.loads(FIXTURE.read_text())

    def inspect(self):
        return HISTORY.inspect(json.dumps(self.value).encode(), TARGET, now_ms=1577836810000)

    def test_sanitized_existing_records_are_observations_not_authority(self):
        result = self.inspect()
        self.assertEqual((result.records, result.matching_records, result.latest_observation), (4, 4, "complete"))
        self.assertFalse(result.completion_is_authoritative)
        self.assertFalse(result.release_reservation)

    def test_later_failure_supersedes_complete_without_searching_backwards(self):
        last = copy.deepcopy(self.value["data"][-1])
        last["time"] += 1000
        ledger = json.loads(last["state"])
        ledger["entries"][0].update(status="failed", reason="execution_error", updatedAt="2020-01-01T00:00:06Z")
        last["state"] = json.dumps(ledger)
        self.value["data"].append(last)
        self.value["datapoints"] = "5"
        result = self.inspect()
        self.assertEqual(result.latest_observation, "failed")
        self.assertFalse(result.completion_is_authoritative)
        self.assertFalse(result.release_reservation)

    def test_truncation_and_absence_do_not_authorize_recovery(self):
        self.value["data"] = self.value["data"][:1]
        self.value["datapoints"] = "1"
        self.assertEqual(self.inspect().latest_observation, "accepted")
        self.value["data"] = []
        self.value["datapoints"] = "0"
        self.assertFalse(self.inspect().release_reservation)

    def test_wrong_item_count_version_and_duplicate_identity_fail(self):
        for change in ["item", "count", "version", "identity"]:
            with self.subTest(change=change):
                self.setUp()
                if change == "item": self.value["name"] = "UnrelatedItem"
                elif change == "count": self.value["datapoints"] = 4
                else:
                    ledger = json.loads(self.value["data"][0]["state"])
                    if change == "version": ledger["version"] = "unknown"
                    else: ledger["entries"].append(ledger["entries"][0])
                    self.value["data"][0]["state"] = json.dumps(ledger)
                with self.assertRaises(ValueError): self.inspect()

    def test_unordered_conflicting_time_and_terminal_time_fail(self):
        for change in ["order", "same_time", "terminal_time"]:
            with self.subTest(change=change):
                self.setUp()
                if change == "order": self.value["data"].reverse()
                elif change == "same_time": self.value["data"][2]["time"] = self.value["data"][1]["time"]
                else:
                    ledger = json.loads(self.value["data"][2]["state"])
                    del ledger["entries"][0]["updatedAt"]
                    self.value["data"][2]["state"] = json.dumps(ledger)
                with self.assertRaises(ValueError): self.inspect()

    def test_invalid_offset_minutes_are_not_silently_normalized(self):
        for value in ["2020-01-01T00:00:00+00:60", "2020-01-01T00:00:00-00:60", "2020-01-01T00:00:00+24:00"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                HISTORY.timestamp_ms(value)
        self.assertEqual(HISTORY.timestamp_ms("2020-01-01T01:00:00+01:00"), 1577836800000)

    def test_body_ledger_and_json_key_bounds(self):
        with self.assertRaises(ValueError): HISTORY.inspect(b" " * (HISTORY.MAX_BODY + 1), TARGET)
        with self.assertRaises(ValueError): HISTORY.inspect(b'{"name":"a","name":"b"}', TARGET)
        self.value["data"][0]["state"] = " " * (HISTORY.MAX_LEDGER + 1)
        with self.assertRaises(ValueError): self.inspect()


if __name__ == "__main__":
    unittest.main()
