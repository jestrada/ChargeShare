import argparse
import contextlib
import hashlib
import json
import os
import platform
import re
import signal
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path


COMMAND_TIMEOUT_SECONDS = 15
REQUIRED_HOST_PORTS = (8787, 5173, 8443, 10350)
TILT_HOST_PORTS = (8787, 5173, 10350)
TILT_SHUTDOWN_TIMEOUT_SECONDS = 30
TILT_EXEC_TIMEOUT_SECONDS = 3
SYNTHETIC_LABEL = "dev.chargeshare.synthetic-only"
KAFKA_TMPFS = {
    "/etc/kafka/secrets:uid=1000,gid=1000,mode=0700",
    "/mnt/shared/config:uid=1000,gid=1000,mode=0700",
}
RECEIVER_CONFIGURATION = {
    "host": "0.0.0.0",
    "port": 4443,
    "status_port": 8080,
    "log_level": "info",
    "json_log_enable": True,
    "use_default_eng_ca": False,
    "namespace": "chargeshare_synthetic",
    "reliable_ack_sources": {"V": "kafka"},
    "transmit_decoded_records": True,
    "records": {"V": ["kafka"], "connectivity": ["kafka"]},
    "kafka": {
        "bootstrap.servers": "kafka:9092",
        "acks": "all",
        "queue.buffering.max.messages": 1000,
        "queue.buffering.max.ms": 50,
        "message.timeout.ms": 10000,
    },
    "tls": {
        "ca_file": "/runtime/certificates/ca.crt",
        "server_cert": "/runtime/certificates/server.crt",
        "server_key": "/runtime/certificates/server.key",
    },
}


class RuntimeFailure(Exception):
    pass


@dataclass(frozen=True)
class ProjectRuntime:
    checkout: Path

    @classmethod
    def at(cls, checkout):
        return cls(Path(checkout).resolve(strict=True))

    @property
    def project_name(self):
        checkout_digest = hashlib.sha256(os.fsencode(self.checkout)).hexdigest()[:12]
        return "chargeshare-local-" + checkout_digest

    @property
    def directory(self):
        return self.checkout / ".local-runtime"

    @property
    def certificates(self):
        return self.directory / "certificates"

    @property
    def broker_volume(self):
        return self.project_name + "_kafka-data"

    @property
    def run_lock(self):
        return self.checkout / ".local-runtime-run.lock"

    @property
    def compose_file(self):
        return self.checkout / "dev" / "compose.yaml"

    @property
    def receiver_configuration_file(self):
        return self.checkout / "dev" / "receiver.synthetic.json"

    @property
    def marker(self):
        return {"synthetic_only": True, "project_name": self.project_name}

    def environment(self):
        environment = os.environ.copy()
        environment.update({
            "CHARGESHARE_PROJECT_NAME": self.project_name,
            "CHARGESHARE_RUNTIME_DIR": str(self.directory),
            "CHARGESHARE_BROKER_VOLUME": self.broker_volume,
            "CHARGESHARE_UID": str(os.getuid()),
            "CHARGESHARE_GID": str(os.getgid()),
        })
        return environment

    def identity(self):
        return {
            "project_name": self.project_name,
            "runtime_dir": str(self.directory),
            "broker_volume": self.broker_volume,
        }


def run_command(command, environment=None):
    try:
        completed = subprocess.run(
            command,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
            timeout=COMMAND_TIMEOUT_SECONDS,
        )
    except FileNotFoundError:
        raise RuntimeFailure("Required tool is missing: " + command[0] + ". Enter nix develop first.") from None
    except subprocess.TimeoutExpired:
        raise RuntimeFailure("Local tooling exceeded its 15-second deadline; check the Docker daemon.") from None
    if completed.returncode != 0:
        raise RuntimeFailure("Local command failed: " + " ".join(command[:3]) + ". Check the local Docker daemon and project configuration.")
    return completed.stdout


def read_json_file(path, description):
    try:
        if path.is_symlink() or path.resolve() != path or not path.is_file():
            raise RuntimeFailure(description + " must be a regular project file.")
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        raise RuntimeFailure(description + " is unreadable or is not valid JSON.") from None


def validate_receiver_configuration(runtime):
    configuration = read_json_file(runtime.receiver_configuration_file, "Synthetic receiver configuration")
    if configuration != RECEIVER_CONFIGURATION:
        raise RuntimeFailure("Unsafe receiver configuration: restore dev/receiver.synthetic.json to the reviewed local-test-only configuration.")


def check_supported_platform(system=None, machine=None):
    system = platform.system() if system is None else system
    machine = platform.machine() if machine is None else machine
    if system != "Linux" or machine.lower() not in {"x86_64", "amd64"}:
        raise RuntimeFailure("Unsupported host platform: this harness currently requires Linux x86_64.")


def check_local_docker(runtime, command_runner=run_command):
    docker_host = os.environ.get("DOCKER_HOST", "")
    if docker_host and not docker_host.startswith("unix://"):
        raise RuntimeFailure("Use a local Unix-socket Docker daemon; remote Docker endpoints are outside this synthetic harness.")
    try:
        daemon = json.loads(command_runner(["docker", "info", "--format", "{{json .}}"], runtime.environment()))
    except RuntimeFailure:
        raise RuntimeFailure("Docker daemon is stopped, inaccessible or unresponsive. Start a permitted local daemon and ensure your user can access it; no host settings were changed.") from None
    except (json.JSONDecodeError, TypeError):
        raise RuntimeFailure("Docker daemon returned unreadable platform information.") from None
    if not isinstance(daemon, dict) or daemon.get("OSType") != "linux" or str(daemon.get("Architecture", "")).lower() not in {"x86_64", "amd64"}:
        raise RuntimeFailure("Unsupported Docker platform: this harness requires Linux x86_64 containers.")
    docker_context = os.environ.get("DOCKER_CONTEXT", "")
    if not docker_host or docker_context:
        try:
            context_command = ["docker", "context", "inspect"]
            if docker_context:
                context_command.append(docker_context)
            contexts = json.loads(command_runner(context_command, runtime.environment()))
            endpoint = contexts[0]["Endpoints"]["docker"]["Host"]
        except (json.JSONDecodeError, TypeError, KeyError, IndexError):
            raise RuntimeFailure("Cannot verify that the selected Docker context uses a local Unix socket.") from None
        if not isinstance(endpoint, str) or not endpoint.startswith("unix://"):
            raise RuntimeFailure("Use a local Unix-socket Docker context; remote endpoints are outside this synthetic harness.")
    return daemon


def compose_command(runtime, *arguments):
    return [
        "docker", "compose",
        "--project-name", runtime.project_name, "--file", str(runtime.compose_file),
        *arguments,
    ]


def resolved_compose_configuration(runtime, command_runner=run_command):
    try:
        configuration = json.loads(command_runner(compose_command(runtime, "config", "--format", "json"), runtime.environment()))
    except (json.JSONDecodeError, TypeError):
        raise RuntimeFailure("Docker Compose did not return readable resolved JSON configuration.") from None
    validate_compose_configuration(runtime, configuration)
    return configuration


def require_synthetic_labels(resource, description):
    labels = resource.get("labels", {})
    if not isinstance(labels, dict) or labels.get(SYNTHETIC_LABEL) != "true":
        raise RuntimeFailure("Unsafe Compose configuration: " + description + " lacks its synthetic-only label.")


def validate_service_mount(runtime, service_name, mount):
    if not isinstance(mount, dict):
        raise RuntimeFailure("Unsafe Compose configuration: resolved mounts must be explicit objects.")
    if mount.get("type") == "volume":
        if service_name != "kafka" or mount.get("source") != "kafka-data" or mount.get("target") != "/var/lib/kafka/data":
            raise RuntimeFailure("Unsafe Compose configuration: unexpected persistent volume.")
        return
    if service_name != "receiver" or mount.get("type") != "bind" or mount.get("read_only") is not True:
        raise RuntimeFailure("Unsafe Compose configuration: bind mounts must be read-only local-test inputs.")
    source = Path(mount.get("source", ""))
    allowed_sources = {runtime.certificates, runtime.receiver_configuration_file}
    if source not in allowed_sources or source.is_symlink():
        raise RuntimeFailure("Unsafe Compose configuration: unexpected host input mount.")
    if source == runtime.certificates and mount.get("target") != "/runtime/certificates":
        raise RuntimeFailure("Unsafe Compose configuration: generated trust must mount at /runtime/certificates.")
    if source == runtime.receiver_configuration_file and service_name != "receiver":
        raise RuntimeFailure("Unsafe Compose configuration: receiver settings may only mount into receiver.")


def validate_compose_configuration(runtime, configuration):
    if not isinstance(configuration, dict) or configuration.get("name") != runtime.project_name:
        raise RuntimeFailure("Unsafe Compose configuration: use this checkout's derived project name.")
    if configuration.get("secrets") or configuration.get("configs"):
        raise RuntimeFailure("Unsafe Compose configuration: external secrets and configuration selection are excluded.")
    services = configuration.get("services", {})
    if not isinstance(services, dict) or set(services) != {"kafka", "receiver"}:
        raise RuntimeFailure("Unsafe Compose configuration: unexpected synthetic service graph.")
    pins = read_json_file(runtime.checkout / "dev" / "upstream-pins.json", "Reviewed upstream input pins")
    try:
        kafka_image = pins["kafka_image"]["reference"]
        receiver_revision = pins["receiver_source"]["revision"]
        source_repository = pins["receiver_source"]["repository"]
    except (KeyError, TypeError):
        raise RuntimeFailure("Reviewed upstream pins do not identify the local service inputs.") from None
    if not isinstance(kafka_image, str) or not re.fullmatch(r"apache/kafka:[^@]+@sha256:[0-9a-f]{64}", kafka_image) or not isinstance(receiver_revision, str) or not re.fullmatch(r"[0-9a-f]{40}", receiver_revision) or source_repository != "https://github.com/teslamotors/fleet-telemetry":
        raise RuntimeFailure("Reviewed local service inputs must pin official Kafka and receiver sources.")
    if services["kafka"].get("image") != kafka_image or services["kafka"].get("build"):
        raise RuntimeFailure("Unsafe Compose configuration: Kafka must use its exact reviewed immutable image.")
    expected_receiver_build = {"context": str(runtime.checkout / "dev"), "dockerfile": "receiver.Dockerfile"}
    if services["receiver"].get("image") != runtime.project_name + "-receiver:" + receiver_revision[:7] or services["receiver"].get("build") != expected_receiver_build:
        raise RuntimeFailure("Unsafe Compose configuration: receiver must use its reviewed project-local build inputs.")
    receiver_port_count = 0
    for service_name, service in services.items():
        if not isinstance(service, dict):
            raise RuntimeFailure("Unsafe Compose configuration: service definitions must be objects.")
        require_synthetic_labels(service, service_name)
        if service.get("platform") != "linux/amd64" or service.get("volumes_from"):
            raise RuntimeFailure("Unsafe Compose configuration: service platform and mounts must remain isolated Linux amd64 inputs.")
        if service.get("privileged") or service.get("network_mode") or service.get("pid") or service.get("ipc") or service.get("devices") or service.get("cap_add"):
            raise RuntimeFailure("Unsafe Compose configuration: host access and expanded container privileges are excluded.")
        if service.get("environment"):
            environment = service["environment"]
            if not isinstance(environment, dict) or any(value not in (None, "") for key, value in environment.items() if any(part in str(key).lower() for part in ("token", "password", "secret", "oauth"))):
                raise RuntimeFailure("Unsafe Compose configuration: credentials are excluded from synthetic services.")
        if not isinstance(service.get("ports", []), list) or not isinstance(service.get("volumes", []), list):
            raise RuntimeFailure("Unsafe Compose configuration: ports and mounts must be resolved lists.")
        for published_port in service.get("ports", []):
            if not isinstance(published_port, dict) or service_name != "receiver" or published_port.get("host_ip") != "127.0.0.1" or str(published_port.get("published")) != "8443" or str(published_port.get("target")) != "4443" or published_port.get("protocol", "tcp") != "tcp":
                raise RuntimeFailure("Unsafe Compose configuration: only receiver TCP 127.0.0.1:8443 may be published; profiler/status ports stay private.")
            receiver_port_count += 1
        for mount in service.get("volumes", []):
            validate_service_mount(runtime, service_name, mount)
        tmpfs = service.get("tmpfs", [])
        expected_tmpfs = KAFKA_TMPFS if service_name == "kafka" else set()
        if not isinstance(tmpfs, list) or any(not isinstance(mount, str) for mount in tmpfs) or set(tmpfs) != expected_tmpfs or len(tmpfs) != len(expected_tmpfs):
            raise RuntimeFailure("Unsafe Compose configuration: only the exact broker synthetic tmpfs mounts are allowed.")
    if receiver_port_count != 1:
        raise RuntimeFailure("Unsafe Compose configuration: receiver must publish exactly one loopback synthetic transport port.")
    receiver = services["receiver"]
    receiver_user = str(os.getuid()) + ":" + str(os.getgid())
    if receiver.get("command") != ["-config", "/etc/chargeshare/receiver.json"] or receiver.get("entrypoint") or receiver.get("user") != receiver_user or receiver.get("environment"):
        raise RuntimeFailure("Unsafe Compose configuration: receiver must use the reviewed synthetic config and current-user private credentials.")
    receiver_mounts = {(mount.get("source"), mount.get("target")) for mount in receiver.get("volumes", [])}
    expected_receiver_mounts = {
        (str(runtime.certificates), "/runtime/certificates"),
        (str(runtime.receiver_configuration_file), "/etc/chargeshare/receiver.json"),
    }
    if receiver_mounts != expected_receiver_mounts or len(receiver.get("volumes", [])) != 2:
        raise RuntimeFailure("Unsafe Compose configuration: receiver requires exactly the generated trust and reviewed synthetic config mounts.")
    volumes = configuration.get("volumes", {})
    if not isinstance(volumes, dict) or set(volumes) != {"kafka-data"} or not isinstance(volumes["kafka-data"], dict) or volumes["kafka-data"].get("name") != runtime.broker_volume or volumes["kafka-data"].get("external"):
        raise RuntimeFailure("Unsafe Compose configuration: broker storage must use this checkout's exact synthetic volume.")
    if volumes["kafka-data"].get("driver_opts") or volumes["kafka-data"].get("driver", "local") != "local":
        raise RuntimeFailure("Unsafe Compose configuration: broker storage may not bind other host paths or external volume drivers.")
    if len(services["kafka"].get("volumes", [])) != 1:
        raise RuntimeFailure("Unsafe Compose configuration: Kafka requires exactly its named synthetic data volume.")
    require_synthetic_labels(volumes["kafka-data"], "broker volume")
    networks = configuration.get("networks", {})
    if not isinstance(networks, dict) or set(networks) != {"default"} or not isinstance(networks["default"], dict) or networks["default"].get("name") != runtime.project_name + "_default" or networks["default"].get("external") or networks["default"].get("driver", "bridge") != "bridge":
        raise RuntimeFailure("Unsafe Compose configuration: services must use the isolated project bridge network.")
    require_synthetic_labels(networks["default"], "private network")


def check_ports_available(ports=REQUIRED_HOST_PORTS):
    for port in ports:
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", port))
        except OSError:
            raise RuntimeFailure("Required loopback port " + str(port) + " is occupied or cannot be checked. Stop Tilt and preview/services first; no process was stopped.") from None


def validate_runtime_directory(runtime):
    directory = runtime.directory
    if directory.is_symlink():
        raise RuntimeFailure("Refusing a symlinked .local-runtime directory.")
    if not directory.exists():
        return
    if not directory.is_dir():
        raise RuntimeFailure(".local-runtime must be a private project directory.")
    marker_path = directory / "synthetic-runtime.json"
    if read_json_file(marker_path, "Local runtime ownership marker") != runtime.marker:
        raise RuntimeFailure("Refusing runtime state that is not owned by this synthetic checkout.")
    for parent, directories, files in os.walk(directory, followlinks=False):
        for name in directories + files:
            if (Path(parent) / name).is_symlink():
                raise RuntimeFailure("Refusing symlinks inside synthetic runtime state.")
        for name in files:
            path = Path(parent) / name
            if not path.is_file() or path.stat().st_nlink != 1:
                raise RuntimeFailure("Refusing special files or hard links inside synthetic runtime state.")


def ensure_runtime_directory(runtime):
    validate_runtime_directory(runtime)
    if not runtime.directory.exists():
        runtime.directory.mkdir(mode=0o700)
        marker_path = runtime.directory / "synthetic-runtime.json"
        with marker_path.open("x", encoding="utf-8") as marker_file:
            json.dump(runtime.marker, marker_file, sort_keys=True)
            marker_file.write("\n")
        marker_path.chmod(0o600)
    runtime.directory.chmod(0o700)


def ensure_certificate_directory(runtime):
    ensure_runtime_directory(runtime)
    runtime.certificates.mkdir(mode=0o700, exist_ok=True)
    runtime.certificates.chmod(0o700)
    return runtime.certificates


def prepare_runtime(runtime, command_runner=run_command):
    validate_receiver_configuration(runtime)
    ensure_runtime_directory(runtime)
    command_runner(["bash", str(runtime.checkout / "scripts" / "dev" / "certificates.sh")], runtime.environment())


def validate_live_configuration(runtime, command_runner=run_command):
    check_supported_platform()
    validate_receiver_configuration(runtime)
    validate_runtime_directory(runtime)
    daemon = check_local_docker(runtime, command_runner)
    resolved_compose_configuration(runtime, command_runner)
    project_broker_volume(runtime, command_runner)
    return {"synthetic_only": True, "host": "Linux x86_64", "docker_version": daemon.get("ServerVersion", "unknown"), "project_name": runtime.project_name}


def preflight(runtime, command_runner=run_command, port_checker=check_ports_available):
    result = validate_live_configuration(runtime, command_runner)
    port_checker()
    return result


def project_containers(runtime, command_runner):
    identifiers = command_runner(["docker", "ps", "--all", "--filter", "label=com.docker.compose.project=" + runtime.project_name, "--format", "{{.ID}}"], runtime.environment()).split()
    if not identifiers:
        return []
    try:
        containers = json.loads(command_runner(["docker", "inspect", *identifiers], runtime.environment()))
    except (json.JSONDecodeError, TypeError):
        raise RuntimeFailure("Cannot inspect project container ownership safely.") from None
    if not isinstance(containers, list) or len(containers) != len(identifiers):
        raise RuntimeFailure("Cannot establish exact project container ownership.")
    for container in containers:
        labels = container.get("Config", {}).get("Labels", {})
        if labels.get("com.docker.compose.project") != runtime.project_name or labels.get(SYNTHETIC_LABEL) != "true" or labels.get("com.docker.compose.service") not in {"kafka", "receiver"}:
            raise RuntimeFailure("Refusing reset: unexpected or unrelated container ownership.")
        state = container.get("State", {})
        if state.get("Running") is not False or state.get("Restarting") or state.get("Paused"):
            raise RuntimeFailure("Refusing reset while project containers are active. Run tilt down first.")
        if not isinstance(container.get("Id"), str) or not container["Id"]:
            raise RuntimeFailure("Cannot establish exact project container identity.")
    return containers


def project_broker_volume(runtime, command_runner):
    names = command_runner(["docker", "volume", "ls", "--filter", "label=com.docker.compose.project=" + runtime.project_name, "--format", "{{.Name}}"], runtime.environment()).split()
    if any(name != runtime.broker_volume for name in names):
        raise RuntimeFailure("Refusing reset: unexpected project volumes require manual inspection.")
    exact_names = command_runner(["docker", "volume", "ls", "--filter", "name=" + runtime.broker_volume, "--format", "{{.Name}}"], runtime.environment()).split()
    if runtime.broker_volume not in exact_names:
        return None
    try:
        volumes = json.loads(command_runner(["docker", "volume", "inspect", runtime.broker_volume], runtime.environment()))
        volume = volumes[0]
        labels = volume.get("Labels") or {}
    except (json.JSONDecodeError, TypeError, IndexError, AttributeError):
        raise RuntimeFailure("Cannot inspect synthetic broker volume ownership safely.") from None
    if len(volumes) != 1 or volume.get("Name") != runtime.broker_volume or labels.get("com.docker.compose.project") != runtime.project_name or labels.get("com.docker.compose.volume") != "kafka-data" or labels.get(SYNTHETIC_LABEL) != "true":
        raise RuntimeFailure("Refusing reset: broker volume ownership labels do not match this checkout.")
    return runtime.broker_volume


@contextlib.contextmanager
def exclusive_synthetic_run_lock(runtime):
    import fcntl

    try:
        descriptor = os.open(runtime.run_lock, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            lock_metadata = os.fstat(descriptor)
            if not stat.S_ISREG(lock_metadata.st_mode) or lock_metadata.st_nlink != 1:
                raise RuntimeFailure("Refusing an unsafe synthetic run lock file.")
            os.fchmod(descriptor, 0o600)
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise RuntimeFailure("Refusing reset while a synthetic harness run or another reset is active.") from None
            yield
        finally:
            os.close(descriptor)
    except OSError:
        raise RuntimeFailure("Cannot acquire the private synthetic run lock safely; stop the environment before reset.") from None


def reset_runtime(runtime, confirmed=False, command_runner=run_command, port_checker=check_ports_available):
    check_supported_platform()
    port_checker()
    with exclusive_synthetic_run_lock(runtime):
        return reset_stopped_runtime(runtime, confirmed, command_runner, port_checker)


def reset_stopped_runtime(runtime, confirmed, command_runner, port_checker):
    check_supported_platform()
    port_checker()
    validate_runtime_directory(runtime)
    refuse_active_tracked_tilt(runtime)
    refuse_active_harness(runtime)
    check_local_docker(runtime, command_runner)
    containers = project_containers(runtime, command_runner)
    volume = project_broker_volume(runtime, command_runner)
    scope = {
        "project_name": runtime.project_name,
        "stopped_containers": [container["Id"] for container in containers],
        "broker_volume": volume,
        "runtime_directory": str(runtime.directory),
        "retained_run_lock": str(runtime.run_lock),
    }
    print(json.dumps({"synthetic_reset_scope": scope}, sort_keys=True), flush=True)
    if not confirmed:
        raise RuntimeFailure("No data was deleted. To remove exactly this stopped synthetic state, repeat with --confirm-synthetic-reset.")
    port_checker()
    project_containers(runtime, command_runner)
    if containers:
        command_runner(["docker", "rm", *scope["stopped_containers"]], runtime.environment())
    if volume:
        command_runner(["docker", "volume", "rm", volume], runtime.environment())
    port_checker()
    if project_containers(runtime, command_runner):
        raise RuntimeFailure("Project containers appeared during reset; runtime files were preserved. Stop the environment before retrying.")
    validate_runtime_directory(runtime)
    refuse_active_tracked_tilt(runtime)
    if runtime.directory.exists():
        shutil.rmtree(runtime.directory)
    return scope


def tilt_command_index(arguments):
    index = 0
    while index < len(arguments):
        argument = arguments[index]
        if argument in {"-d", "--debug", "-v", "--verbose"} or argument.startswith("--klog="):
            index += 1
        elif argument == "--klog" and index + 1 < len(arguments):
            index += 2
        elif argument.startswith("-"):
            raise RuntimeFailure("Unsupported Tilt global option; put the lifecycle command after documented global flags.")
        else:
            return index
    return None


def normalized_tilt_arguments(runtime, arguments):
    command_index = tilt_command_index(arguments)
    if command_index is None or arguments[command_index] not in {"up", "ci", "down"}:
        raise RuntimeFailure("The lifecycle helper requires Tilt up, ci or down.")
    action = arguments[command_index]
    retained = list(arguments[:command_index + 1])
    remaining = arguments[command_index + 1:]
    tiltfile_arguments = None
    index = 0
    while index < len(remaining):
        argument = remaining[index]
        if argument == "--":
            tiltfile_arguments = remaining[index + 1:]
            break
        option = argument.split("=", 1)[0]
        if option in {"--host", "--port", "--file", "-f"}:
            if "=" in argument:
                value = argument.split("=", 1)[1]
            elif index + 1 < len(remaining):
                index += 1
                value = remaining[index]
            else:
                raise RuntimeFailure("Tilt lifecycle option is missing its value.")
            if option == "--host" and (action == "down" or value != "127.0.0.1"):
                raise RuntimeFailure("Tilt lifecycle requires host 127.0.0.1; unsafe host overrides are refused.")
            if option == "--port" and (action == "down" or value != "10350"):
                raise RuntimeFailure("Tilt lifecycle requires port 10350; conflicting overrides are refused.")
            if option in {"--file", "-f"}:
                candidate = Path(value)
                candidate = candidate if candidate.is_absolute() else runtime.checkout / candidate
                if candidate.resolve() != runtime.checkout / "Tiltfile":
                    raise RuntimeFailure("Tilt lifecycle requires this checkout's reviewed Tiltfile.")
        else:
            retained.append(argument)
        index += 1
    retained.extend(["--file", str(runtime.checkout / "Tiltfile")])
    if action in {"up", "ci"}:
        retained.extend(["--host", "127.0.0.1", "--port", "10350"])
    if tiltfile_arguments is not None:
        retained.extend(["--", *tiltfile_arguments])
    return action, retained


def is_tilt_help_request(arguments):
    index = 0
    while index < len(arguments):
        argument = arguments[index]
        if argument in {"-d", "--debug", "-v", "--verbose"} or argument.startswith("--klog="):
            index += 1
        elif argument == "--klog" and index + 1 < len(arguments):
            index += 2
        else:
            break
    remainder = arguments[index:]
    return remainder in [["-h"], ["--help"], ["--version"], ["up", "-h"], ["up", "--help"], ["ci", "-h"], ["ci", "--help"], ["down", "-h"], ["down", "--help"]]


def linux_process_identity(pid):
    if not isinstance(pid, int) or pid <= 1:
        raise RuntimeFailure("Invalid tracked Tilt process identity.")
    process_directory = Path("/proc") / str(pid)
    try:
        process_fields = (process_directory / "stat").read_text(encoding="utf-8").rpartition(") ")[2].split()
        return {
            "pid": pid,
            "start_time": process_fields[19],
            "state": process_fields[0],
            "executable": str((process_directory / "exe").resolve(strict=True)),
            "checkout": str((process_directory / "cwd").resolve(strict=True)),
        }
    except FileNotFoundError:
        return None
    except (OSError, IndexError, UnicodeError):
        raise RuntimeFailure("Cannot verify tracked Tilt process ownership; stop it manually before retrying.") from None


def owned_tilt_identity(runtime, binary, record, process_reader=linux_process_identity):
    if not isinstance(record, dict) or record.get("project_name") != runtime.project_name or record.get("executable") != str(binary) or record.get("checkout") != str(runtime.checkout):
        return None
    current = process_reader(record.get("pid"))
    if current is None or current.get("state") in {"Z", "X"}:
        return None
    required_fields = ("pid", "start_time", "executable", "checkout")
    if any(current.get(field) != record.get(field) for field in required_fields):
        return None
    return current


def signal_owned_tilt(runtime, binary, record, process_reader=linux_process_identity):
    if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
        raise RuntimeFailure("Safe Tilt shutdown requires Linux pidfd support; stop Tilt manually before retrying.")
    current = owned_tilt_identity(runtime, binary, record, process_reader)
    if current is None:
        return False
    try:
        descriptor = os.pidfd_open(current["pid"])
        try:
            if owned_tilt_identity(runtime, binary, record, process_reader) is None:
                return False
            signal.pidfd_send_signal(descriptor, signal.SIGINT)
        finally:
            os.close(descriptor)
    except ProcessLookupError:
        return False
    except OSError:
        raise RuntimeFailure("Cannot signal the verified Tilt process; stop it manually before retrying.") from None
    return True


def refuse_active_tracked_tilt(runtime, process_reader=linux_process_identity):
    record_path = runtime.directory / "tilt-process.json"
    if not record_path.exists():
        return
    record = read_json_file(record_path, "Tracked Tilt process")
    if not isinstance(record, dict) or not isinstance(record.get("executable"), str):
        raise RuntimeFailure("Cannot verify tracked Tilt state; stop the environment before reset.")
    if owned_tilt_identity(runtime, Path(record["executable"]), record, process_reader) is not None:
        raise RuntimeFailure("Refusing reset while this checkout's Tilt process is active. Run tilt down first.")


def refuse_active_harness(runtime):
    import fcntl

    lock_path = runtime.directory / "harness.lock"
    if not lock_path.exists():
        return
    try:
        with lock_path.open("r", encoding="utf-8") as lock:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise RuntimeFailure("Refusing reset while a synthetic harness run is active. Wait for it to finish and stop the environment first.") from None
    except OSError:
        raise RuntimeFailure("Cannot verify harness lock ownership safely; stop the environment before reset.") from None


def stop_tracked_tilt(runtime, binary, process_reader=linux_process_identity, signal_sender=signal_owned_tilt, port_checker=check_ports_available, sleeper=time.sleep, monotonic=time.monotonic):
    validate_runtime_directory(runtime)
    record_path = runtime.directory / "tilt-process.json"
    record = read_json_file(record_path, "Tracked Tilt process") if record_path.exists() else None
    current = owned_tilt_identity(runtime, binary, record, process_reader) if record is not None else None
    owned_process_was_active = current is not None
    if owned_process_was_active:
        signal_sender(runtime, binary, record, process_reader)
    deadline = monotonic() + TILT_SHUTDOWN_TIMEOUT_SECONDS
    while True:
        current = owned_tilt_identity(runtime, binary, record, process_reader) if record is not None else None
        try:
            port_checker(TILT_HOST_PORTS)
            listeners_stopped = True
        except RuntimeFailure:
            listeners_stopped = False
        if current is None and listeners_stopped:
            return
        if not owned_process_was_active and not listeners_stopped:
            raise RuntimeFailure("An untracked Tilt or preview listener is active. Stop it manually; no unrelated process was signaled.")
        if monotonic() >= deadline:
            raise RuntimeFailure("Tilt or preview listeners did not stop within 30 seconds. Stop the environment manually before retrying.")
        sleeper(0.2)


def save_tilt_record(runtime, record):
    ensure_runtime_directory(runtime)
    descriptor, temporary_name = tempfile.mkstemp(prefix=".tilt-process-", dir=runtime.directory)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as record_file:
            json.dump(record, record_file, sort_keys=True)
            record_file.write("\n")
        os.replace(temporary_name, runtime.directory / "tilt-process.json")
    finally:
        if Path(temporary_name).exists():
            Path(temporary_name).unlink()


def wait_for_tilt_exec(runtime, binary, child, process_reader=linux_process_identity, sleeper=time.sleep, monotonic=time.monotonic):
    deadline = monotonic() + TILT_EXEC_TIMEOUT_SECONDS
    initial_start_time = None
    while True:
        record = process_reader(child.pid)
        if child.poll() is not None:
            return None
        if record is not None:
            if initial_start_time is None:
                initial_start_time = record["start_time"]
            if record["start_time"] != initial_start_time:
                raise RuntimeFailure("Tilt child process identity changed before startup could be verified.")
            if record["executable"] == str(binary) and record["checkout"] == str(runtime.checkout):
                return record
        if monotonic() >= deadline:
            raise RuntimeFailure("Pinned Tilt executable identity could not be established within three seconds.")
        sleeper(0.02)


def stop_spawned_tilt(child):
    if child.poll() is None:
        child.send_signal(signal.SIGINT)
    try:
        child.wait(timeout=TILT_SHUTDOWN_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        raise RuntimeFailure("The launched Tilt process did not stop within 30 seconds; inspect it manually.") from None


def launch_tilt(runtime, binary, arguments):
    if not arguments:
        raise RuntimeFailure("Tilt launcher requires a command.")
    if is_tilt_help_request(arguments):
        return subprocess.call([str(binary), *arguments], cwd=runtime.checkout, env=runtime.environment())
    action, arguments = normalized_tilt_arguments(runtime, arguments)
    if action == "down":
        check_supported_platform()
        stop_tracked_tilt(runtime, binary)
        validate_live_configuration(runtime)
        try:
            completed = subprocess.run([str(binary), *arguments], cwd=runtime.checkout, env=runtime.environment(), timeout=90)
        except subprocess.TimeoutExpired:
            raise RuntimeFailure("Tilt down exceeded its 90-second deadline; project state was preserved for inspection.") from None
        if completed.returncode == 0:
            check_ports_available()
        return completed.returncode
    with exclusive_synthetic_run_lock(runtime):
        preflight(runtime)
        ensure_runtime_directory(runtime)
        child = subprocess.Popen([str(binary), *arguments], cwd=runtime.checkout, env=runtime.environment(), start_new_session=True)
        try:
            record = wait_for_tilt_exec(runtime, binary, child)
            if record is None:
                return child.wait()
            record["project_name"] = runtime.project_name
            save_tilt_record(runtime, record)
        except (RuntimeFailure, KeyboardInterrupt, OSError):
            stop_spawned_tilt(child)
            raise
    original_handlers = {}

    def forward_shutdown(signum, frame):
        if owned_tilt_identity(runtime, binary, record) is not None:
            signal_owned_tilt(runtime, binary, record)

    try:
        for signum in (signal.SIGINT, signal.SIGTERM):
            original_handlers[signum] = signal.signal(signum, forward_shutdown)
        return child.wait()
    finally:
        for signum, handler in original_handlers.items():
            signal.signal(signum, handler)
        record_path = runtime.directory / "tilt-process.json"
        if record_path.exists() and read_json_file(record_path, "Tracked Tilt process") == record:
            record_path.unlink()


def main(arguments=None):
    arguments = list(sys.argv[1:] if arguments is None else arguments)
    runtime = ProjectRuntime.at(Path(__file__).resolve().parents[2])
    if arguments and arguments[0] == "tilt":
        try:
            if len(arguments) < 3 or not Path(arguments[1]).is_absolute():
                raise RuntimeFailure("Tilt launcher requires the absolute pinned Tilt executable and a command.")
            binary = Path(arguments[1]).resolve(strict=True)
            return launch_tilt(runtime, binary, arguments[2:])
        except (RuntimeFailure, OSError) as failure:
            print("ChargeShare local runtime: " + str(failure), file=sys.stderr)
            return 1
        except KeyboardInterrupt:
            return 130
    parser = argparse.ArgumentParser(description="Guard this checkout's synthetic local harness and disposable runtime state.")
    parser.add_argument("action", choices=("identity", "project-name", "runtime-dir", "certificate-directory", "prepare", "preflight", "validate-config", "validate-live-config", "reset"))
    parser.add_argument("--confirm-synthetic-reset", action="store_true")
    options = parser.parse_args(arguments)
    if options.confirm_synthetic_reset and options.action != "reset":
        parser.error("--confirm-synthetic-reset is valid only for reset")
    try:
        if options.action == "identity":
            print(json.dumps(runtime.identity(), sort_keys=True))
        elif options.action == "project-name":
            print(runtime.project_name)
        elif options.action == "runtime-dir":
            print(runtime.directory)
        elif options.action == "certificate-directory":
            print(ensure_certificate_directory(runtime))
        elif options.action == "prepare":
            prepare_runtime(runtime)
            print("Prepared private local-test certificates; no operating-system trust was changed.")
        elif options.action == "preflight":
            print(json.dumps(preflight(runtime), sort_keys=True))
        elif options.action == "validate-config":
            validate_receiver_configuration(runtime)
            resolved_compose_configuration(runtime)
            print("Resolved Compose configuration is project-scoped and synthetic-only.")
        elif options.action == "validate-live-config":
            print(json.dumps(validate_live_configuration(runtime), sort_keys=True))
        elif options.action == "reset":
            reset_runtime(runtime, options.confirm_synthetic_reset)
            print("Removed only this checkout's stopped synthetic runtime state.")
    except (RuntimeFailure, OSError) as failure:
        print("ChargeShare local runtime: " + str(failure), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
