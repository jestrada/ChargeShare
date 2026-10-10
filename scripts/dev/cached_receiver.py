import argparse
import copy
import hashlib
import json
import os
import re
import sys
from pathlib import Path

from runtime import ProjectRuntime, RuntimeFailure, read_json_file, resolved_compose_configuration, run_command


BUILD_INPUTS = ("dev/receiver.Dockerfile", "dev/upstream-pins.json", "dev/.dockerignore")
BUILD_LABEL = "dev.chargeshare.receiver-build-inputs"
REVISION_LABEL = "org.opencontainers.image.revision"


def build_identity(runtime):
    pins = read_json_file(runtime.checkout / "dev/upstream-pins.json", "Reviewed upstream input pins")
    revision = pins["receiver_source"]["revision"]
    digest = hashlib.sha256()
    for name in BUILD_INPUTS:
        digest.update(name.encode() + b"\0")
        digest.update((runtime.checkout / name).read_bytes())
        digest.update(b"\0")
    return {
        "tag": runtime.project_name + "-receiver:" + revision[:7],
        "revision": revision,
        "inputs_sha": digest.hexdigest(),
        "cache_scope": "receiver-v1-linux-amd64-" + digest.hexdigest(),
    }


def validate_loaded_image(identity, images, expected_id):
    if not isinstance(expected_id, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", expected_id):
        raise RuntimeFailure("Cached receiver requires the image identity from this job's reviewed build.")
    if not isinstance(images, list) or len(images) != 1 or not isinstance(images[0], dict):
        raise RuntimeFailure("Cached receiver image inspection is invalid.")
    image = images[0]
    labels = image.get("Config", {}).get("Labels") or {}
    if image.get("Id") != expected_id or image.get("Os") != "linux" or image.get("Architecture") != "amd64" or labels.get(BUILD_LABEL) != identity["inputs_sha"] or labels.get(REVISION_LABEL) != identity["revision"]:
        raise RuntimeFailure("Cached receiver does not match this job's reviewed Linux build inputs.")


def compose_with_loaded_receiver(configuration):
    prepared = copy.deepcopy(configuration)
    del prepared["services"]["receiver"]["build"]
    return prepared


def loaded_compose(runtime, command_runner=run_command):
    configuration = resolved_compose_configuration(runtime, command_runner)
    identity = build_identity(runtime)
    try:
        images = json.loads(command_runner(["docker", "image", "inspect", identity["tag"]], runtime.environment()))
    except json.JSONDecodeError:
        raise RuntimeFailure("Cached receiver image inspection is unreadable.") from None
    validate_loaded_image(identity, images, os.environ.get("CHARGESHARE_CI_RECEIVER_IMAGE_ID"))
    return compose_with_loaded_receiver(configuration)


def main():
    parser = argparse.ArgumentParser(description="Reuse only this job's pinned, reviewed receiver image.")
    parser.add_argument("action", choices=("identity", "compose"))
    options = parser.parse_args()
    runtime = ProjectRuntime.at(Path(__file__).resolve().parents[2])
    try:
        if options.action == "identity":
            identity = build_identity(runtime)
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                for name, value in identity.items():
                    output.write(name + "=" + value + "\n")
            print("Selected pinned Linux receiver build and input-scoped cache.")
        else:
            print(json.dumps(loaded_compose(runtime)))
    except (RuntimeFailure, OSError, KeyError) as failure:
        if isinstance(failure, RuntimeFailure):
            print("Cached receiver: " + str(failure), file=sys.stderr)
        else:
            print("Cached receiver: reviewed build inputs or job output are unavailable.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
