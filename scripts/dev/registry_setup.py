import argparse
import subprocess
import sys
from pathlib import Path


def setup_command(database, bootstrap, car_names, charger_name, charger_owner):
    if not all(Path(path).is_absolute() for path in (database, bootstrap)):
        raise ValueError("Private absolute paths required.")
    command = ["cargo", "run", "--quiet", "--locked", "--package", "chargeshare-ingestion", "--", "registry", "migrate",
               "--database", str(database), "--bootstrap", str(bootstrap),
               "--charger-name", charger_name, "--charger-owner", charger_owner]
    for name in car_names:
        command.extend(["--car-name", name])
    return command


class SetupParser(argparse.ArgumentParser):
    def error(self, message):
        self.exit(2, "Registry setup: invalid command.\n")


def main():
    parser = SetupParser(description="Explicitly name the migrated charger and declare its owner while ingestion is stopped.")
    parser.add_argument("--database", required=True)
    parser.add_argument("--bootstrap", required=True)
    parser.add_argument("--car-name", action="append", required=True)
    parser.add_argument("--charger-name", required=True)
    parser.add_argument("--charger-owner", required=True)
    options = parser.parse_args()
    try:
        command = setup_command(options.database, options.bootstrap, options.car_name, options.charger_name, options.charger_owner)
        return subprocess.run(command, cwd=Path(__file__).resolve().parents[2], check=False).returncode
    except (OSError, ValueError):
        print("Registry setup failed; check private paths and explicit setup inputs.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
