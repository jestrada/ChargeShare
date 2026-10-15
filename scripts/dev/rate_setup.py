import argparse
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


def setup_command(database, charger, active_from):
    if not Path(database).is_absolute():
        raise ValueError("Absolute database path required.")
    instant = datetime.fromisoformat(active_from.replace("Z", "+00:00"))
    if instant.utcoffset() != timezone.utc.utcoffset(None) or instant.microsecond:
        raise ValueError("Integral UTC timestamp required.")
    definition = Path(__file__).resolve().parents[2] / "dev/cpsf-green-rate.json"
    return ["cargo", "run", "--quiet", "--locked", "--package", "chargeshare-ingestion", "--", "registry", "set-rate",
            "--database", str(database), "--charger", charger, "--active-from", str(int(instant.timestamp())), "--definition", str(definition)]


class SetupParser(argparse.ArgumentParser):
    def error(self, message):
        self.exit(2, "Rate setup: invalid command.\n")


def main():
    parser = SetupParser(description="Assign the public CleanPowerSF Green rate to one charger while ingestion is stopped.")
    parser.add_argument("--database", required=True)
    parser.add_argument("--charger", required=True)
    parser.add_argument("--active-from", required=True)
    options = parser.parse_args()
    try:
        command = setup_command(options.database, options.charger, options.active_from)
        return subprocess.run(command, cwd=Path(__file__).resolve().parents[2], check=False).returncode
    except (OSError, ValueError, OverflowError):
        print("Rate setup failed; check the explicit charger and UTC timestamp.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
