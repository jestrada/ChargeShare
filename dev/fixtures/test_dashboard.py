import json
import random
import tempfile
import unittest
from collections import defaultdict
from datetime import datetime
from decimal import Decimal
from pathlib import Path

from dev.fixtures.dashboard import FIXTURES, LOCAL_TIMEZONE, SYNTHETIC_NOTICE, generate_dashboard_fixture, write_placeholder_images


class DashboardFixtureTests(unittest.TestCase):
    def setUp(self):
        self.fixture = generate_dashboard_fixture()
        self.sessions = defaultdict(list)
        for annotation, record in zip(self.fixture["manifest"]["annotations"], self.fixture["records"], strict=True):
            self.sessions[annotation["connection"]].append((annotation, record))

    def test_generation_is_repeatable_and_leaves_global_random_state_alone(self):
        random_state = random.getstate()
        self.assertEqual(self.fixture, generate_dashboard_fixture())
        self.assertEqual(random_state, random.getstate())
        self.assertEqual(self.fixture["notice"], SYNTHETIC_NOTICE)

    def test_only_fictional_scope_and_existing_charger_coordinates_are_used(self):
        manifest = self.fixture["manifest"]
        self.assertEqual(manifest["mappings"], [{"device": "vehicle-a", "owner": "owner-a", "vehicle": "vehicle-a"}])
        self.assertEqual(manifest["owner_chargers"][0]["charger_id"], "fictional-shared-charger")
        complete = json.loads((FIXTURES / "complete.json").read_text())
        original_location = next(field["value"]["locationValue"] for field in complete["expected"][0]["payload"]["data"] if field["key"] == "Location")
        for record in self.fixture["records"]:
            self.assertEqual(record["key"], "vehicle-a")
            self.assertEqual(record["payload"]["vin"], "vehicle-a")
            self.assertEqual(record["payload"]["data"][2]["value"]["locationValue"], original_location)
        for coordinate in ("latitude", "longitude"):
            self.assertEqual(Decimal(manifest["owner_chargers"][0]["geofence"]["center"][coordinate]), Decimal(str(original_location[coordinate])))

    def test_four_busy_months_include_mostly_midnight_and_all_rate_periods(self):
        monthly_starts = defaultdict(list)
        for session in self.sessions.values():
            local_start = datetime.fromisoformat(session[0][0]["created_at"]).astimezone(LOCAL_TIMEZONE)
            monthly_starts[local_start.month].append(local_start.hour * 60 + local_start.minute)
        self.assertEqual(set(monthly_starts), {6, 7, 8, 9})
        for starts in monthly_starts.values():
            self.assertGreaterEqual(len(starts), 20)
            self.assertGreaterEqual(sum(minute <= 60 for minute in starts) / len(starts), 0.7)
            self.assertTrue(any(900 <= minute < 960 or minute >= 1260 for minute in starts))
            self.assertTrue(any(960 <= minute < 1260 for minute in starts))

    def test_sessions_have_complete_nonoverlapping_monotonic_counter_evidence(self):
        prior_end = None
        prior_counter = Decimal(0)
        energies = set()
        for session in self.sessions.values():
            self.assertEqual(session[0][0]["kind"], "start")
            self.assertEqual(session[-1][0]["kind"], "end")
            times = [datetime.fromisoformat(annotation["created_at"]) for annotation, _ in session]
            counters = [Decimal(record["payload"]["data"][1]["value"]["stringValue"]) for _, record in session]
            self.assertEqual(times, sorted(set(times)))
            self.assertEqual(counters, sorted(set(counters)))
            self.assertGreaterEqual(counters[0], prior_counter)
            if prior_end:
                self.assertGreater(times[0], prior_end)
            energy = counters[-1] - counters[0]
            hours = Decimal(int((times[-1] - times[0]).total_seconds())) / 3600
            self.assertGreaterEqual(energy / hours, Decimal("3.5"))
            self.assertLessEqual(energy / hours, Decimal("7.2"))
            energies.add(energy)
            prior_end, prior_counter = times[-1], counters[-1]
        self.assertGreater(len(energies), 30)
        self.assertEqual([int(annotation["position"]) for annotation in self.fixture["manifest"]["annotations"]], list(range(1, len(self.fixture["records"]) + 1)))

    def test_named_multiple_car_and_charger_fixture_preserves_isolated_scopes(self):
        fixture = generate_dashboard_fixture(multiple=True)
        self.assertEqual([scope["name"] for scope in fixture["display_registry"]], ["Model 3 sample", "Model Y sample"])
        self.assertEqual([scope["chargers"][0]["name"] for scope in fixture["display_registry"]], ["Driveway charger", "Garage charger"])
        self.assertEqual({record["key"] for record in fixture["records"]}, {"vehicle-a", "vehicle-b"})
        self.assertEqual(len(fixture["records"]), 2 * len(self.fixture["records"]))
        self.assertEqual([scope["color"] for scope in fixture["display_registry"]], ["#B8BDC3", "#16181C"])
        for vehicle, expected in [("vehicle-a", "82"), ("vehicle-b", "67")]:
            end = next(record for annotation, record in reversed(list(zip(fixture["manifest"]["annotations"], fixture["records"], strict=True))) if record["key"] == vehicle and annotation["kind"] == "end")
            self.assertEqual(next(field["value"]["stringValue"] for field in end["payload"]["data"] if field["key"] == "BatteryLevel"), expected)
        for mapping in fixture["manifest"]["mappings"]:
            self.assertTrue(all(record["payload"]["vin"] == mapping["vehicle"] for record in fixture["records"] if record["key"] == mapping["vehicle"]))

    def test_placeholder_images_are_repeatable_small_pngs_for_the_sample_colors(self):
        with tempfile.TemporaryDirectory(prefix="chargeshare-placeholder-", dir="/tmp") as directory:
            images = Path(directory)
            write_placeholder_images(images)
            png = (images / "b8bdc3.png").read_bytes()
            self.assertTrue(png.startswith(b"\x89PNG\r\n\x1a\n"))
            self.assertEqual(int.from_bytes(png[16:20], "big"), 120)
            self.assertEqual(int.from_bytes(png[20:24], "big"), 60)
            self.assertLess(len(png), 1024)
            self.assertEqual(sorted(path.name for path in images.iterdir()), ["16181c.png", "b8bdc3.png"])
            write_placeholder_images(images)
            self.assertEqual((images / "b8bdc3.png").read_bytes(), png)

    def test_current_old_and_never_reported_examples_keep_independent_source_ages(self):
        fresh = generate_dashboard_fixture(multiple=True, battery_example="fresh")
        old = generate_dashboard_fixture(multiple=True, battery_example="old")
        never = generate_dashboard_fixture(multiple=True, battery_example="never")
        self.assertEqual(len(fresh["records"]), len(old["records"]) + 2)
        start, sample = fresh["records"][-2:]
        self.assertEqual(start["payload"]["data"][0]["value"]["detailedChargeStateValue"], "DetailedChargeStateCharging")
        self.assertFalse(any(field["key"] == "DetailedChargeState" for field in sample["payload"]["data"]))
        self.assertEqual(next(field["value"]["stringValue"] for field in sample["payload"]["data"] if field["key"] == "BatteryLevel"), "62")
        for record, age in [(start, 12), (sample, 8)]:
            self.assertEqual(fresh["as_of_utc_seconds"] - int(datetime.fromisoformat(record["payload"]["createdAt"]).timestamp()), age * 60)
        self.assertGreater(old["as_of_utc_seconds"] - int(datetime.fromisoformat(old["records"][-1]["payload"]["createdAt"]).timestamp()), 86400)
        self.assertFalse(any(field["key"] == "BatteryLevel" for record in never["records"] for field in record["payload"]["data"]))


if __name__ == "__main__":
    unittest.main()
