import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys


def live_commands(checkout, config, web_port=5174):
    config = Path(config)
    if not config.is_absolute() or config.resolve() != config or config.is_relative_to(checkout):
        raise ValueError("Live config must be a private absolute path outside the checkout.")
    if type(web_port) is not int or not 1 <= web_port <= 65535:
        raise ValueError("Web port must be an integer from 1 to 65535.")
    return [
        ["cargo", "run", "--locked", "--package", "chargeshare-ingestion", "--", "live", str(config)],
        ["npm", "--prefix", "apps/web", "run", "dev:persisted", "--", "--host", "127.0.0.1", "--port", str(web_port)],
    ]


def live_api_port(config):
    configuration = json.loads(Path(config).read_text())
    if not isinstance(configuration, dict):
        raise ValueError("Live config must be a JSON object.")
    port = configuration.get("api_port", 8788)
    if type(port) is not int or not 1 <= port <= 65535:
        raise ValueError("API port must be an integer from 1 to 65535.")
    return port


def run(checkout, config, web_port=5174):
    commands = live_commands(checkout, config, web_port)
    web_environment = os.environ.copy()
    web_environment["CHARGESHARE_API_PORT"] = str(live_api_port(config))
    children = []
    previous = {}

    def shutdown(signum, frame):
        raise KeyboardInterrupt

    try:
        for signum in (signal.SIGTERM, signal.SIGINT):
            previous[signum] = signal.signal(signum, shutdown)
        for command in commands:
            children.append(subprocess.Popen(command, cwd=checkout, start_new_session=True,
                                             env=web_environment if command[0] == "npm" else None))
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
    parser.add_argument("--web-port", type=int, default=5174)
    options = parser.parse_args()
    try:
        return run(Path(__file__).resolve().parents[2], options.config, options.web_port)
    except KeyboardInterrupt:
        return 130
    except (OSError, ValueError):
        print("Live runtime failed; check the private config and local prerequisites.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
