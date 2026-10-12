import json
import os
import re
import stat
import tempfile
import uuid

from runtime import RuntimeFailure, SYNTHETIC_LABEL, ensure_runtime_directory, project_broker_volume, run_command, validate_runtime_directory


EPOCH_LABEL = "dev.chargeshare.source-epoch"
TOPIC = "chargeshare_synthetic_V"
CLUSTER_ID = "MkU3OEVBNTcwNTJENDM2Qk"


def read_marker(runtime):
    validate_runtime_directory(runtime)
    path = runtime.directory / "source-epoch.json"
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(descriptor, "rb") as marker:
            metadata = os.fstat(marker.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) != 0o600:
                raise RuntimeFailure("Source epoch marker must be an owned private regular file.")
            source = json.loads(marker.read(16385))
        if set(source) != {"epoch", "topic", "cluster_id", "topic_id", "partitions"} or not isinstance(source["epoch"], str) or not re.fullmatch(r"synthetic-[0-9a-f]{32}", source["epoch"]) or source["topic"] != TOPIC or source["cluster_id"] != CLUSTER_ID or source["topic_id"] is not None or source["partitions"] != [{"partition": 0, "initial_offset": 0}]:
            raise RuntimeFailure("Source epoch marker does not match the pinned synthetic contract.")
        return source
    except (OSError, ValueError, TypeError, KeyError):
        raise RuntimeFailure("Source epoch marker is missing or invalid; retained evidence was preserved.") from None


def write_private_json(path, value):
    descriptor, temporary = tempfile.mkstemp(prefix=".source-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            json.dump(value, output, sort_keys=True)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def verify_volume_epoch(runtime, source, command_runner):
    if project_broker_volume(runtime, command_runner) is None:
        raise RuntimeFailure("Owned source volume is missing; retained evidence was preserved.")
    try:
        volumes = json.loads(command_runner(["docker", "volume", "inspect", runtime.broker_volume], runtime.environment()))
        epoch = volumes[0]["Labels"][EPOCH_LABEL]
        if len(volumes) != 1 or epoch != source["epoch"]:
            raise RuntimeFailure("Source epoch and owned volume disagree; retained evidence was preserved.")
    except (ValueError, TypeError, KeyError, IndexError):
        raise RuntimeFailure("Owned volume lacks its source epoch; retained evidence was preserved.") from None


def verify_source_epoch(runtime, command_runner=run_command):
    source = read_marker(runtime)
    verify_volume_epoch(runtime, source, command_runner)
    return source


def ensure_source_epoch(runtime, command_runner=run_command):
    ensure_runtime_directory(runtime)
    path = runtime.directory / "source-epoch.json"
    if path.exists():
        return verify_source_epoch(runtime, command_runner)
    if project_broker_volume(runtime, command_runner) is not None or any(runtime.directory.glob("ingestion.sqlite3*")):
        raise RuntimeFailure("Missing source epoch for retained state; explicit stopped owned reset is required.")
    source = {
        "epoch": "synthetic-" + uuid.uuid4().hex,
        "topic": TOPIC,
        "cluster_id": CLUSTER_ID,
        "topic_id": None,
        "partitions": [{"partition": 0, "initial_offset": 0}],
    }
    labels = {
        "com.docker.compose.project": runtime.project_name,
        "com.docker.compose.volume": "kafka-data",
        SYNTHETIC_LABEL: "true",
        EPOCH_LABEL: source["epoch"],
    }
    command = ["docker", "volume", "create"]
    for name, value in labels.items():
        command.extend(["--label", name + "=" + value])
    command_runner([*command, runtime.broker_volume], runtime.environment())
    verify_volume_epoch(runtime, source, command_runner)
    write_private_json(path, source)
    return source


def ensure_manifest(runtime):
    path = runtime.directory / "manifest.json"
    expected = json.loads((runtime.checkout / "dev" / "ingestion-manifest.json").read_bytes())
    if path.exists():
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or stat.S_IMODE(metadata.st_mode) != 0o600 or metadata.st_uid != os.getuid():
            raise RuntimeFailure("Frozen ingestion manifest differs; retained evidence was preserved.")
        retained = json.loads(path.read_bytes())
        frozen = lambda config: {key: value for key, value in config.items() if key not in ("geofence_version", "geofences")}
        if frozen(retained) != frozen(expected) or (retained.get("geofence_version") == expected.get("geofence_version") and retained.get("geofences") != expected.get("geofences")):
            raise RuntimeFailure("Frozen ingestion manifest differs; retained evidence was preserved.")
        if retained != expected:
            write_private_json(path, expected)
    else:
        if any(runtime.directory.glob("ingestion.sqlite3*")):
            raise RuntimeFailure("Retained evidence has no frozen manifest; explicit stopped owned reset is required.")
        write_private_json(path, expected)
