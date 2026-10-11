import json
import os
import uuid
from pathlib import Path

from runtime import RuntimeFailure, SYNTHETIC_LABEL, read_json_file, run_command
from source_epoch import verify_source_epoch


RUN_LABEL = "dev.chargeshare.ingestion-run"


def validate_network(runtime, networks):
    if not isinstance(networks, list) or len(networks) != 1:
        raise RuntimeFailure("Cannot establish the owned private ingestion network.")
    network = networks[0]
    labels = network.get("Labels") or {}
    if network.get("Name") != runtime.project_name + "_default" or network.get("Driver") != "bridge" or labels.get("com.docker.compose.project") != runtime.project_name or labels.get(SYNTHETIC_LABEL) != "true":
        raise RuntimeFailure("Ingestion requires this checkout's owned private bridge network.")


def runner_command(runtime, name, run_id, arguments, directory=None, command_runner=run_command):
    verify_source_epoch(runtime, command_runner)
    network = runtime.project_name + "_default"
    validate_network(runtime, json.loads(command_runner(["docker", "network", "inspect", network], runtime.environment())))
    binary = runtime.checkout / "target" / "debug" / "chargeshare-ingestion"
    if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK) or not Path("/nix/store").is_dir():
        raise RuntimeFailure("Build the ingestion binary in the pinned Nix shell before acceptance.")
    directory = runtime.directory if directory is None else directory
    if directory.resolve() != directory or not directory.is_relative_to(runtime.directory):
        raise RuntimeFailure("Ingestion storage must remain inside this owned runtime.")
    pins = read_json_file(runtime.checkout / "dev" / "upstream-pins.json", "Reviewed ingestion runner input")
    return [
        "docker", "run", "--rm", "--name", name, "--platform", "linux/amd64",
        "--network", network, "--user", str(os.getuid()) + ":" + str(os.getgid()),
        "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--label", SYNTHETIC_LABEL + "=true", "--label", RUN_LABEL + "=" + run_id,
        "--mount", "type=bind,src=/nix/store,dst=/nix/store,readonly",
        "--mount", "type=bind,src=" + str(binary) + ",dst=/run/ingestion,readonly",
        "--mount", "type=bind,src=" + str(directory) + ",dst=/run/.local-runtime",
        "--workdir", "/run", "--entrypoint", "/run/ingestion",
        pins["kafka_image"]["reference"], *arguments,
    ]


def owned_runner(containers, name, run_id):
    if not isinstance(containers, list) or len(containers) != 1:
        raise RuntimeFailure("Cannot establish the exact ephemeral ingestion process.")
    container = containers[0]
    labels = container.get("Config", {}).get("Labels") or {}
    if container.get("Name") != "/" + name or labels.get(SYNTHETIC_LABEL) != "true" or labels.get(RUN_LABEL) != run_id:
        raise RuntimeFailure("Refusing to signal an unrelated ingestion container.")
    return container["Id"]


def runner_name(runtime):
    return runtime.project_name + "-ingestion-" + uuid.uuid4().hex[:12]
