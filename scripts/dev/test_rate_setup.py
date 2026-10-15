import unittest

from rate_setup import setup_command


class RateSetupTests(unittest.TestCase):
    def test_explicit_charger_and_integral_utc_instant_reach_rust(self):
        command = setup_command("/tmp/fictional-rate-test/ingestion.sqlite3", "fictional-shared-charger", "2026-03-01T08:00:00Z")
        self.assertEqual(command[command.index("--active-from") + 1], "1772352000")
        self.assertEqual(command[command.index("--charger") + 1], "fictional-shared-charger")
        self.assertTrue(command[-1].endswith("/dev/cpsf-green-rate.json"))

    def test_invalid_instants_and_relative_paths_fail_before_running_a_command(self):
        for instant in ["2026-03-01T08:00:00", "2026-03-01T08:00:00.5Z", "2026-03-01T08:00:00+01:00", "invalid"]:
            with self.assertRaises(ValueError):
                setup_command("/tmp/fictional-rate-test/ingestion.sqlite3", "fictional-shared-charger", instant)
        with self.assertRaises(ValueError):
            setup_command("ingestion.sqlite3", "fictional-shared-charger", "2026-03-01T08:00:00Z")
