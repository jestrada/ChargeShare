import argparse
import os
from pathlib import Path
import signal
import subprocess
import sys


def live_commands(checkout, config):
    config = Path(config)
    if not config.is_absolute() or config.resolve() != config or config.is_relative_to(checkout):
        raise ValueError("Live config must be a private absolute path outside the checkout.")
    return [
        ["cargo", "run", "--locked", "--package", "chargeshare-ingestion", "--", "live", str(config)],
        ["npm", "--prefix", "apps/web", "run", "dev:persisted", "--", "--host", "127.0.0.1"],
    ]


def run(checkout, config):
    commands = live_commands(checkout, config)
    children = []
    previous = {}

    def shutdown(signum, frame):
        raise KeyboardInterrupt

    try:
        for signum in (signal.SIGTERM, signal.SIGINT):
            previous[signum] = signal.signal(signum, shutdown)
        for command in commands:
            children.append(subprocess.Popen(command, cwd=checkout, start_new_session=True))
        while True:
            for child in children:
                try:
                    return child.wait(timeout=0.2)
                except subprocess.TimeoutExpired:
                    pass
    finally:
        for child in children:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
        for child in children:
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=5)
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def main():
    parser = argparse.ArgumentParser(description="Run live ingestion and the persisted dashboard on localhost.")
    parser.add_argument("--config", required=True)
    options = parser.parse_args()
    try:
        return run(Path(__file__).resolve().parents[2], options.config)
    except KeyboardInterrupt:
        return 130
    except (OSError, ValueError):
        print("Live runtime failed; check the private config and local prerequisites.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
