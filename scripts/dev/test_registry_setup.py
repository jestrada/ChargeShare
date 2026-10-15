import subprocess
import sys
from pathlib import Path
import unittest
from registry_setup import setup_command


class RegistrySetupTests(unittest.TestCase):
    def test_setup_explicitly_declares_charger_owner_and_display_names(self):
        command = setup_command("/private/database.sqlite3", "/private/bootstrap.json", ["vehicle-a=Car A"], "Driveway charger", "charger-owner")
        self.assertEqual(command[-2:], ["--car-name", "vehicle-a=Car A"])
        self.assertEqual(command[command.index("--charger-owner") + 1], "charger-owner")
        self.assertEqual(command[command.index("--charger-name") + 1], "Driveway charger")
        self.assertIn("migrate", command)

    def test_invalid_arguments_do_not_echo_rejected_inputs(self):
        output = subprocess.run([sys.executable, str(Path(__file__).with_name("registry_setup.py")), "--vin", "PRIVATE-INPUT-SHOULD-NOT-ECHO"], text=True, capture_output=True, check=False)
        self.assertEqual(output.returncode, 2)
        self.assertEqual(output.stderr, "Registry setup: invalid command.\n")
        self.assertEqual(output.stdout, "")

    def test_setup_refuses_relative_private_paths(self):
        for database, bootstrap in [("database.sqlite3", "/private/bootstrap.json"), ("/private/database.sqlite3", "bootstrap.json")]:
            with self.assertRaises(ValueError):
                setup_command(database, bootstrap, ["vehicle-a=Car A"], "Charger A", "owner-a")


if __name__ == "__main__":
    unittest.main()
