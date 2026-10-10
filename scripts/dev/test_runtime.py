import contextlib
import io
import json
import os
import queue
import shutil
import socket
import ssl
import stat
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
import runtime


class TiltfileLifecycleTests(unittest.TestCase):
    def evaluate(self, action, certificate_failure, configuration_failure=False, prebuilt=False):
        observed = []

        def local(command, **options):
            observed.append(command)
            if command.endswith(" prepare") and certificate_failure:
                raise runtime.RuntimeFailure(certificate_failure)
            if command.endswith(" validate-config") and configuration_failure:
                raise runtime.RuntimeFailure("Unsafe Compose configuration")
            return "synthetic"

        def resource(*arguments, **options):
            observed.append(arguments)

        namespace = {
            "config": SimpleNamespace(tilt_subcommand=action),
            "os": SimpleNamespace(putenv=lambda *arguments: None, getenv=lambda name: "1" if prebuilt else None),
            "local": local,
            "docker_compose": resource,
            "dc_resource": resource,
            "local_resource": resource,
            "probe": resource,
            "http_get_action": resource,
            "TRIGGER_MODE_MANUAL": "manual",
        }
        tiltfile = Path(__file__).resolve().parents[2] / "Tiltfile"
        exec(compile(tiltfile.read_text(), str(tiltfile), "exec"), namespace)
        return observed

    def test_shutdown_can_load_resources_with_expired_or_incomplete_trust(self):
        for certificate_failure in ["Synthetic certificate expired", "Incomplete synthetic certificate bundle"]:
            with self.subTest(certificate_failure=certificate_failure):
                observed = self.evaluate("down", certificate_failure)
                self.assertIn("python3 scripts/dev/runtime.py validate-config", observed)
                self.assertIn(("dev/compose.yaml",), observed)

    def test_startup_still_refuses_expired_or_incomplete_trust(self):
        for action in ["up", "ci"]:
            for certificate_failure in ["Synthetic certificate expired", "Incomplete synthetic certificate bundle"]:
                with self.subTest(action=action, certificate_failure=certificate_failure):
                    with self.assertRaisesRegex(runtime.RuntimeFailure, certificate_failure):
                        self.evaluate(action, certificate_failure)

    def test_shutdown_still_refuses_unsafe_configuration(self):
        with self.assertRaisesRegex(runtime.RuntimeFailure, "Unsafe Compose configuration"):
            self.evaluate("down", "Incomplete synthetic certificate bundle", configuration_failure=True)

    def test_cached_receiver_is_startup_only_and_shutdown_uses_reviewed_source_configuration(self):
        observed = self.evaluate("up", None, prebuilt=True)
        self.assertIn("python3 scripts/dev/cached_receiver.py compose", observed)
        observed = self.evaluate("down", "Incomplete synthetic certificate bundle", prebuilt=True)
        self.assertNotIn("python3 scripts/dev/cached_receiver.py compose", observed)
        self.assertIn(("dev/compose.yaml",), observed)

    def test_cached_receiver_does_not_bypass_invalid_startup_trust(self):
        with self.assertRaisesRegex(runtime.RuntimeFailure, "Incomplete synthetic certificate bundle"):
            self.evaluate("up", "Incomplete synthetic certificate bundle", prebuilt=True)


def synthetic_compose(project):
    labels = {runtime.SYNTHETIC_LABEL: "true"}
    return {
        "name": project.project_name,
        "services": {
            "kafka": {
                "labels": dict(labels),
                "image": "apache/kafka:synthetic@sha256:" + "b" * 64,
                "platform": "linux/amd64",
                "tmpfs": sorted(runtime.KAFKA_TMPFS),
                "volumes": [{"type": "volume", "source": "kafka-data", "target": "/var/lib/kafka/data"}],
            },
            "receiver": {
                "labels": dict(labels),
                "image": project.project_name + "-receiver:aaaaaaa",
                "build": {"context": str(project.checkout / "dev"), "dockerfile": "receiver.Dockerfile"},
                "platform": "linux/amd64",
                "user": str(os.getuid()) + ":" + str(os.getgid()),
                "command": ["-config", "/etc/chargeshare/receiver.json"],
                "ports": [{"host_ip": "127.0.0.1", "published": "8443", "target": 4443, "protocol": "tcp"}],
                "volumes": [
                    {"type": "bind", "source": str(project.certificates), "target": "/runtime/certificates", "read_only": True},
                    {"type": "bind", "source": str(project.receiver_configuration_file), "target": "/etc/chargeshare/receiver.json", "read_only": True},
                ],
            },
        },
        "volumes": {"kafka-data": {"name": project.broker_volume, "labels": dict(labels)}},
        "networks": {"default": {"name": project.project_name + "_default", "labels": dict(labels)}},
    }


def stopped_container(project, identifier="synthetic-container"):
    return {
        "Id": identifier,
        "Config": {"Labels": {
            "com.docker.compose.project": project.project_name,
            "com.docker.compose.service": "receiver",
            runtime.SYNTHETIC_LABEL: "true",
        }},
        "State": {"Running": False, "Restarting": False, "Paused": False},
    }


def synthetic_volume(project):
    return {"Name": project.broker_volume, "Labels": {
        "com.docker.compose.project": project.project_name,
        "com.docker.compose.volume": "kafka-data",
        runtime.SYNTHETIC_LABEL: "true",
    }}


class FakeDocker:
    def __init__(self, project):
        self.project = project
        self.commands = []
        self.daemon = {"OSType": "linux", "Architecture": "x86_64", "ServerVersion": "synthetic-test-version"}
        self.context = [{"Endpoints": {"docker": {"Host": "unix:///var/run/docker.sock"}}}]
        self.configuration = synthetic_compose(project)
        self.containers = [stopped_container(project)]
        self.volumes = [synthetic_volume(project)]
        self.daemon_failure = False

    def __call__(self, command, environment):
        self.commands.append(command)
        if environment["CHARGESHARE_PROJECT_NAME"] != self.project.project_name:
            raise AssertionError("Project scope was not passed to Docker")
        if command[:2] == ["docker", "info"]:
            if self.daemon_failure:
                raise runtime.RuntimeFailure("Synthetic unavailable daemon")
            return json.dumps(self.daemon)
        if command[:3] == ["docker", "context", "inspect"]:
            return json.dumps(self.context)
        if command[:2] == ["docker", "compose"]:
            if command[-3:] != ["config", "--format", "json"]:
                raise AssertionError("Only resolved JSON configuration was expected")
            return json.dumps(self.configuration)
        if command[:2] == ["docker", "ps"]:
            project_name = command[command.index("--filter") + 1].rsplit("=", 1)[1]
            return "\n".join(container["Id"] for container in self.containers if container["Config"]["Labels"].get("com.docker.compose.project") == project_name)
        if command[:2] == ["docker", "inspect"]:
            return json.dumps([container for container in self.containers if container["Id"] in command[2:]])
        if command[:3] == ["docker", "volume", "ls"]:
            volume_filter = command[command.index("--filter") + 1]
            if volume_filter.startswith("label="):
                project_name = volume_filter.split("=", 2)[2]
                return "\n".join(volume["Name"] for volume in self.volumes if volume["Labels"].get("com.docker.compose.project") == project_name)
            name = volume_filter.split("=", 1)[1]
            return "\n".join(volume["Name"] for volume in self.volumes if name in volume["Name"])
        if command[:3] == ["docker", "volume", "inspect"]:
            return json.dumps([volume for volume in self.volumes if volume["Name"] == command[3]])
        if command[:2] == ["docker", "rm"]:
            self.containers = [container for container in self.containers if container["Id"] not in command[2:]]
            return "\n".join(command[2:])
        if command[:3] == ["docker", "volume", "rm"]:
            self.volumes = [volume for volume in self.volumes if volume["Name"] != command[3]]
            return command[3]
        raise AssertionError("Unexpected command: " + str(command))

    def mutations(self):
        return [command for command in self.commands if command[:2] == ["docker", "rm"] or command[:3] == ["docker", "volume", "rm"]]


class RuntimeGuardTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.project = runtime.ProjectRuntime.at(self.temporary.name)
        self.project.receiver_configuration_file.parent.mkdir()
        self.project.receiver_configuration_file.write_text(json.dumps(runtime.RECEIVER_CONFIGURATION), encoding="utf-8")
        (self.project.checkout / "dev" / "upstream-pins.json").write_text(json.dumps({
            "receiver_source": {"repository": "https://github.com/teslamotors/fleet-telemetry", "revision": "a" * 40},
            "kafka_image": {"reference": "apache/kafka:synthetic@sha256:" + "b" * 64},
        }), encoding="utf-8")
        self.docker = FakeDocker(self.project)
        self.no_ports = mock.Mock()
        self.environment = mock.patch.dict(os.environ, {"DOCKER_HOST": "", "DOCKER_CONTEXT": ""})
        self.environment.start()

    def tearDown(self):
        self.environment.stop()
        self.temporary.cleanup()

    def run_reset(self, confirmed=True, port_checker=None):
        with contextlib.redirect_stdout(io.StringIO()):
            return runtime.reset_runtime(self.project, confirmed, self.docker, port_checker or self.no_ports)

    def test_project_identity_is_stable_and_resolves_checkout_symlinks(self):
        symlink = self.project.checkout / "checkout-alias"
        symlink.symlink_to(self.project.checkout, target_is_directory=True)
        self.assertEqual(runtime.ProjectRuntime.at(symlink).identity(), self.project.identity())
        self.assertRegex(self.project.project_name, r"^chargeshare-local-[0-9a-f]{12}$")
        another_checkout = self.project.checkout / "another-checkout"
        another_checkout.mkdir()
        self.assertNotEqual(runtime.ProjectRuntime.at(another_checkout).project_name, self.project.project_name)

    def test_preflight_resolves_actual_compose_json_and_never_mutates_docker(self):
        result = runtime.preflight(self.project, self.docker, self.no_ports)
        self.assertTrue(result["synthetic_only"])
        self.assertEqual(result["project_name"], self.project.project_name)
        compose = next(command for command in self.docker.commands if command[:2] == ["docker", "compose"])
        self.assertIn(self.project.project_name, compose)
        self.assertEqual(compose[-3:], ["config", "--format", "json"])
        self.assertEqual(self.docker.mutations(), [])
        self.no_ports.assert_called_once_with()

    def test_daemon_down_reports_host_prerequisite_without_changing_settings(self):
        self.docker.daemon_failure = True
        with self.assertRaisesRegex(runtime.RuntimeFailure, "stopped, inaccessible or unresponsive"):
            runtime.preflight(self.project, self.docker, self.no_ports)
        self.assertEqual(self.docker.mutations(), [])

    def test_live_configuration_validates_daemon_and_scope_without_rejecting_active_ports(self):
        with mock.patch.object(runtime, "check_ports_available") as ports:
            result = runtime.validate_live_configuration(self.project, self.docker)
        self.assertTrue(result["synthetic_only"])
        ports.assert_not_called()
        self.assertEqual(self.docker.mutations(), [])

    def test_unsupported_host_is_rejected_before_docker(self):
        for system, machine in [("Darwin", "x86_64"), ("Linux", "aarch64"), ("Windows", "AMD64")]:
            with self.subTest(system=system, machine=machine):
                with self.assertRaisesRegex(runtime.RuntimeFailure, "requires Linux x86_64"):
                    runtime.check_supported_platform(system, machine)
        with mock.patch.object(runtime.platform, "system", return_value="Darwin"):
            with self.assertRaises(runtime.RuntimeFailure):
                runtime.preflight(self.project, self.docker, self.no_ports)
        self.assertEqual(self.docker.commands, [])

    def test_non_linux_or_non_x86_64_daemon_is_rejected(self):
        for operating_system, architecture in [("windows", "x86_64"), ("linux", "aarch64")]:
            with self.subTest(operating_system=operating_system, architecture=architecture):
                self.docker.daemon.update({"OSType": operating_system, "Architecture": architecture})
                with self.assertRaisesRegex(runtime.RuntimeFailure, "requires Linux x86_64 containers"):
                    runtime.preflight(self.project, self.docker, self.no_ports)

    def test_remote_docker_context_or_host_is_rejected(self):
        self.docker.context[0]["Endpoints"]["docker"]["Host"] = "ssh://synthetic.invalid"
        with self.assertRaisesRegex(runtime.RuntimeFailure, "remote endpoints"):
            runtime.preflight(self.project, self.docker, self.no_ports)
        self.docker.commands.clear()
        with mock.patch.dict(os.environ, {"DOCKER_HOST": "tcp://synthetic.invalid:2375"}):
            with self.assertRaisesRegex(runtime.RuntimeFailure, "remote Docker endpoints"):
                runtime.preflight(self.project, self.docker, self.no_ports)
        self.assertEqual(self.docker.commands, [])

    def test_remote_context_cannot_hide_behind_local_docker_host(self):
        self.docker.context[0]["Endpoints"]["docker"]["Host"] = "ssh://synthetic.invalid"
        with mock.patch.dict(os.environ, {"DOCKER_HOST": "unix:///var/run/docker.sock", "DOCKER_CONTEXT": "synthetic-remote"}):
            with self.assertRaisesRegex(runtime.RuntimeFailure, "remote endpoints"):
                runtime.preflight(self.project, self.docker, self.no_ports)
        self.assertIn(["docker", "context", "inspect", "synthetic-remote"], self.docker.commands)
        self.assertFalse(any(command[:2] == ["docker", "compose"] for command in self.docker.commands))

    def test_synthetic_receiver_config_rejects_unknown_keys_and_external_brokers(self):
        for replacement in [dict(runtime.RECEIVER_CONFIGURATION, pubsub={"gcp_project_id": "synthetic"}), dict(runtime.RECEIVER_CONFIGURATION, kafka={"bootstrap.servers": "synthetic.invalid:9092"})]:
            with self.subTest(replacement=replacement):
                self.project.receiver_configuration_file.write_text(json.dumps(replacement), encoding="utf-8")
                with self.assertRaisesRegex(runtime.RuntimeFailure, "Unsafe receiver configuration"):
                    runtime.preflight(self.project, self.docker, self.no_ports)
        self.assertEqual(self.docker.commands, [])

    def test_invalid_receiver_json_is_rejected(self):
        self.project.receiver_configuration_file.write_text("{", encoding="utf-8")
        with self.assertRaisesRegex(runtime.RuntimeFailure, "not valid JSON"):
            runtime.preflight(self.project, self.docker, self.no_ports)

    def test_compose_rejects_non_loopback_profiler_and_unresolved_ports(self):
        for replacement in [
            {"host_ip": "0.0.0.0", "published": "8443", "target": 4443},
            {"published": "8443", "target": 4443},
            {"host_ip": "127.0.0.1", "published": "6060", "target": 6060},
            "127.0.0.1:8443:4443",
        ]:
            with self.subTest(replacement=replacement):
                configuration = synthetic_compose(self.project)
                configuration["services"]["receiver"]["ports"] = [replacement]
                with self.assertRaisesRegex(runtime.RuntimeFailure, "only receiver TCP"):
                    runtime.validate_compose_configuration(self.project, configuration)

    def test_compose_rejects_host_privileges_external_resources_and_unknown_mounts(self):
        mutations = [
            lambda configuration: configuration["services"]["receiver"].update({"privileged": True}),
            lambda configuration: configuration["services"]["receiver"].update({"network_mode": "host"}),
            lambda configuration: configuration["services"]["receiver"].update({"environment": {"API_TOKEN": "synthetic-secret"}}),
            lambda configuration: configuration["services"]["receiver"]["volumes"][0].update({"read_only": False}),
            lambda configuration: configuration["services"]["receiver"]["volumes"][0].update({"source": "/var/run/docker.sock"}),
            lambda configuration: configuration["volumes"]["kafka-data"].update({"external": True}),
            lambda configuration: configuration["networks"]["default"].update({"external": True}),
            lambda configuration: configuration["services"]["receiver"]["labels"].clear(),
            lambda configuration: configuration.update({"name": "another-checkout"}),
            lambda configuration: configuration["volumes"]["kafka-data"].update({"driver_opts": {"type": "none", "o": "bind", "device": "/synthetic/host-path"}}),
            lambda configuration: configuration["services"]["kafka"].update({"image": "apache/kafka:latest"}),
            lambda configuration: configuration["services"]["receiver"]["build"].update({"args": {"SOURCE": "synthetic.invalid"}}),
            lambda configuration: configuration["services"]["kafka"].update({"tmpfs": ["/synthetic/unreviewed"]}),
        ]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                configuration = synthetic_compose(self.project)
                mutate(configuration)
                with self.assertRaisesRegex(runtime.RuntimeFailure, "Unsafe Compose configuration"):
                    runtime.validate_compose_configuration(self.project, configuration)

    def test_occupied_loopback_port_is_detected_without_process_inspection(self):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as occupied:
            occupied.bind(("127.0.0.1", 0))
            occupied.listen()
            port = occupied.getsockname()[1]
            with self.assertRaisesRegex(runtime.RuntimeFailure, "port " + str(port) + " is occupied"):
                runtime.check_ports_available((port,))
            with self.assertRaisesRegex(runtime.RuntimeFailure, "occupied"):
                runtime.preflight(self.project, self.docker, lambda: runtime.check_ports_available((port,)))

    def test_each_preview_receiver_and_tilt_port_is_checked(self):
        self.assertEqual(set(runtime.REQUIRED_HOST_PORTS), {8787, 5173, 8443, 10350})
        with mock.patch.object(runtime.socket, "socket") as create_socket:
            listener = create_socket.return_value.__enter__.return_value
            runtime.check_ports_available()
        self.assertEqual(listener.bind.call_args_list, [mock.call(("127.0.0.1", port)) for port in runtime.REQUIRED_HOST_PORTS])

    def test_port_inspection_permission_error_fails_closed(self):
        with mock.patch.object(runtime.socket, "socket") as create_socket:
            create_socket.return_value.__enter__.return_value.bind.side_effect = PermissionError()
            with self.assertRaisesRegex(runtime.RuntimeFailure, "cannot be checked"):
                runtime.check_ports_available()

    def test_reset_refuses_running_containers_and_preserves_runtime(self):
        runtime.ensure_runtime_directory(self.project)
        self.docker.containers[0]["State"]["Running"] = True
        with self.assertRaisesRegex(runtime.RuntimeFailure, "Run tilt down first"):
            self.run_reset()
        self.assertTrue(self.project.directory.exists())
        self.assertEqual(self.docker.mutations(), [])

    def test_reset_refuses_preview_or_tilt_listener_even_with_inaccessible_docker(self):
        self.docker.daemon_failure = True
        occupied = mock.Mock(side_effect=runtime.RuntimeFailure("Tilt listener is occupied"))
        with self.assertRaisesRegex(runtime.RuntimeFailure, "Tilt listener"):
            self.run_reset(port_checker=occupied)
        self.assertEqual(self.docker.commands, [])

    def test_reset_requires_explicit_confirmation_and_prints_exact_scope(self):
        runtime.ensure_runtime_directory(self.project)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            with self.assertRaisesRegex(runtime.RuntimeFailure, "--confirm-synthetic-reset"):
                runtime.reset_runtime(self.project, False, self.docker, self.no_ports)
        scope = json.loads(output.getvalue())["synthetic_reset_scope"]
        self.assertEqual(scope["broker_volume"], self.project.broker_volume)
        self.assertEqual(scope["runtime_directory"], str(self.project.directory))
        self.assertEqual(scope["stopped_containers"], ["synthetic-container"])
        self.assertTrue(self.project.directory.exists())
        self.assertEqual(self.docker.mutations(), [])

    def test_confirmed_reset_removes_only_owned_stopped_state(self):
        runtime.ensure_runtime_directory(self.project)
        (self.project.directory / "synthetic-offsets.json").write_text("{}", encoding="utf-8")
        source = self.project.checkout / "source-preserved.txt"
        source.write_text("synthetic source", encoding="utf-8")
        unrelated_container = stopped_container(self.project, "unrelated-container")
        unrelated_container["Config"]["Labels"]["com.docker.compose.project"] = "unrelated-project"
        unrelated_container["State"]["Running"] = True
        unrelated_volume = {"Name": "unrelated-volume", "Labels": {"com.docker.compose.project": "unrelated-project"}}
        self.docker.containers.append(unrelated_container)
        self.docker.volumes.append(unrelated_volume)
        self.run_reset()
        self.assertFalse(self.project.directory.exists())
        self.assertEqual(source.read_text(encoding="utf-8"), "synthetic source")
        self.assertEqual(self.docker.containers, [unrelated_container])
        self.assertEqual(self.docker.volumes, [unrelated_volume])
        self.assertEqual(self.docker.mutations(), [["docker", "rm", "synthetic-container"], ["docker", "volume", "rm", self.project.broker_volume]])
        self.assertNotIn("prune", str(self.docker.commands))

    def test_reset_refuses_mismatched_volume_labels(self):
        self.docker.volumes[0]["Labels"][runtime.SYNTHETIC_LABEL] = "false"
        with self.assertRaisesRegex(runtime.RuntimeFailure, "ownership labels"):
            self.run_reset()
        self.assertEqual(self.docker.mutations(), [])

    def test_preflight_and_reset_refuse_exact_name_collision_with_unrelated_volume(self):
        self.docker.volumes[0]["Labels"]["com.docker.compose.project"] = "unrelated-project"
        with self.assertRaisesRegex(runtime.RuntimeFailure, "ownership labels"):
            runtime.preflight(self.project, self.docker, self.no_ports)
        with self.assertRaisesRegex(runtime.RuntimeFailure, "ownership labels"):
            self.run_reset()
        self.assertEqual(self.docker.mutations(), [])

    def test_reset_refuses_unrecognized_runtime_directory_and_symlinks(self):
        self.project.directory.mkdir()
        with self.assertRaisesRegex(runtime.RuntimeFailure, "ownership marker"):
            self.run_reset()
        self.project.directory.rmdir()
        runtime.ensure_runtime_directory(self.project)
        outside = self.project.checkout / "outside-preserved.txt"
        outside.write_text("synthetic outside data", encoding="utf-8")
        (self.project.directory / "symlink").symlink_to(outside)
        with self.assertRaisesRegex(runtime.RuntimeFailure, "symlinks"):
            self.run_reset()
        self.assertTrue(outside.exists())
        self.assertEqual(self.docker.mutations(), [])

    def test_reset_does_not_remove_runtime_if_daemon_is_inaccessible(self):
        runtime.ensure_runtime_directory(self.project)
        self.docker.daemon_failure = True
        with self.assertRaisesRegex(runtime.RuntimeFailure, "inaccessible"):
            self.run_reset()
        self.assertTrue(self.project.directory.exists())
        self.assertEqual(self.docker.mutations(), [])

    def test_external_commands_have_bounded_deadlines(self):
        with mock.patch.object(runtime.subprocess, "run", side_effect=subprocess.TimeoutExpired("docker", 15)) as command:
            with self.assertRaisesRegex(runtime.RuntimeFailure, "15-second deadline"):
                runtime.run_command(["docker", "info"])
        self.assertEqual(command.call_args.kwargs["timeout"], 15)

    def test_active_harness_lock_blocks_reset_without_deleting_state(self):
        import fcntl

        runtime.ensure_runtime_directory(self.project)
        with (self.project.directory / "harness.lock").open("w", encoding="utf-8") as harness_lock:
            fcntl.flock(harness_lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(runtime.RuntimeFailure, "harness run is active"):
                self.run_reset()
        self.assertTrue(self.project.directory.exists())
        self.assertEqual(self.docker.commands, [])

    def test_stable_harness_lock_blocks_reset_and_survives_runtime_deletion(self):
        import fcntl

        with self.project.run_lock.open("w", encoding="utf-8") as harness_lock:
            fcntl.flock(harness_lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(runtime.RuntimeFailure, "harness run or another reset is active"):
                self.run_reset()
        self.assertEqual(self.docker.commands, [])
        inode = self.project.run_lock.stat().st_ino
        runtime.ensure_runtime_directory(self.project)
        self.run_reset()
        self.assertFalse(self.project.directory.exists())
        self.assertEqual(self.project.run_lock.stat().st_ino, inode)
        self.assertEqual(stat.S_IMODE(self.project.run_lock.stat().st_mode), 0o600)

    def test_stable_run_lock_is_held_through_docker_and_runtime_deletion(self):
        import fcntl

        runtime.ensure_runtime_directory(self.project)
        calls = []

        def docker_while_locked(command, environment):
            with self.project.run_lock.open("r", encoding="utf-8") as other_run:
                with self.assertRaises(BlockingIOError):
                    fcntl.flock(other_run.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            calls.append(command)
            return self.docker(command, environment)

        original_remove = runtime.shutil.rmtree

        def remove_while_locked(path):
            with self.project.run_lock.open("r", encoding="utf-8") as other_run:
                with self.assertRaises(BlockingIOError):
                    fcntl.flock(other_run.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            original_remove(path)

        with mock.patch.object(runtime.shutil, "rmtree", side_effect=remove_while_locked):
            with contextlib.redirect_stdout(io.StringIO()):
                runtime.reset_runtime(self.project, True, docker_while_locked, self.no_ports)
        self.assertTrue(calls)
        self.assertFalse(self.project.directory.exists())

    def test_run_lock_symlink_or_hardlink_is_refused(self):
        outside = self.project.checkout / "synthetic-outside-lock"
        outside.write_text("preserved", encoding="utf-8")
        self.project.run_lock.symlink_to(outside)
        with self.assertRaises(runtime.RuntimeFailure):
            self.run_reset()
        self.project.run_lock.unlink()
        os.link(outside, self.project.run_lock)
        with self.assertRaisesRegex(runtime.RuntimeFailure, "unsafe synthetic run lock"):
            self.run_reset()
        self.assertEqual(outside.read_text(encoding="utf-8"), "preserved")


class TiltLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.project = runtime.ProjectRuntime.at(self.temporary.name)
        self.binary = self.project.checkout / "synthetic-tilt"
        self.record = {
            "pid": 4242,
            "start_time": "synthetic-start-time",
            "state": "S",
            "executable": str(self.binary),
            "checkout": str(self.project.checkout),
            "project_name": self.project.project_name,
        }

    def tearDown(self):
        self.temporary.cleanup()

    def test_global_flags_cannot_bypass_loopback_and_project_file_arguments(self):
        for prefix in [[], ["--debug"], ["-d", "-v"], ["--klog", "1"], ["--klog=1", "--verbose"]]:
            for action in ["up", "ci"]:
                with self.subTest(prefix=prefix, action=action):
                    selected, arguments = runtime.normalized_tilt_arguments(self.project, [*prefix, action, "--stream"])
                    self.assertEqual(selected, action)
                    self.assertEqual(arguments[:len(prefix) + 1], [*prefix, action])
                    self.assertEqual(arguments[-6:], ["--file", str(self.project.checkout / "Tiltfile"), "--host", "127.0.0.1", "--port", "10350"])

    def test_conflicting_host_port_and_alternate_tiltfile_are_rejected(self):
        for arguments in [
            ["up", "--host", "0.0.0.0"],
            ["ci", "--host=::"],
            ["up", "--port=0"],
            ["up", "--file", "other-Tiltfile"],
            ["down", "-f", "../Tiltfile"],
            ["--unknown", "up"],
            ["up", "--host"],
        ]:
            with self.subTest(arguments=arguments):
                with self.assertRaises(runtime.RuntimeFailure):
                    runtime.normalized_tilt_arguments(self.project, arguments)

    def test_matching_user_flags_are_normalized_to_one_safe_binding(self):
        selected, arguments = runtime.normalized_tilt_arguments(self.project, ["up", "--host=127.0.0.1", "--port", "10350", "-f", "Tiltfile"])
        self.assertEqual(selected, "up")
        self.assertEqual(arguments.count("--host"), 1)
        self.assertEqual(arguments.count("--port"), 1)
        self.assertEqual(arguments.count("--file"), 1)

    def test_forced_flags_precede_tiltfile_argument_separator(self):
        selected, arguments = runtime.normalized_tilt_arguments(self.project, ["up", "--", "receiver", "--help"])
        self.assertEqual(arguments[-3:], ["--", "receiver", "--help"])
        self.assertLess(arguments.index("--host"), arguments.index("--"))
        self.assertLess(arguments.index("--file"), arguments.index("--"))
        self.assertFalse(runtime.is_tilt_help_request(["up", "--", "--help"]))
        with self.assertRaisesRegex(runtime.RuntimeFailure, "unsafe host"):
            runtime.normalized_tilt_arguments(self.project, ["up", "--host=0.0.0.0", "--", "--help"])

    def test_only_canonical_help_forms_bypass_startup(self):
        for arguments in [["--help"], ["--version"], ["up", "--help"], ["--debug", "ci", "-h"], ["--klog", "1", "down", "--help"]]:
            with self.subTest(arguments=arguments):
                self.assertTrue(runtime.is_tilt_help_request(arguments))
                with mock.patch.object(runtime.subprocess, "call", return_value=0) as invoke:
                    with mock.patch.object(runtime, "preflight") as preflight:
                        self.assertEqual(runtime.launch_tilt(self.project, self.binary, arguments), 0)
                preflight.assert_not_called()
                self.assertEqual(invoke.call_args.args[0], [str(self.binary), *arguments])
        for arguments in [["up", "--", "--help"], ["up", "--file", "--help"], ["up", "--host=0.0.0.0", "--help"], ["--klog", "--help", "up"]]:
            self.assertFalse(runtime.is_tilt_help_request(arguments))

    def test_pid_records_are_private_and_match_project_runtime_identity(self):
        runtime.save_tilt_record(self.project, self.record)
        record_path = self.project.directory / "tilt-process.json"
        self.assertEqual(stat.S_IMODE(record_path.stat().st_mode), 0o600)
        self.assertEqual(runtime.read_json_file(record_path, "test record"), self.record)
        self.assertEqual(list(self.project.directory.glob(".tilt-process-*")), [])

    def test_stale_pid_wrong_binary_checkout_or_project_is_never_owned(self):
        for field, replacement in [("start_time", "different-start"), ("executable", "/synthetic/unrelated"), ("checkout", "/synthetic/other-checkout"), ("pid", 4243)]:
            current = dict(self.record, **{field: replacement})
            with self.subTest(field=field):
                self.assertIsNone(runtime.owned_tilt_identity(self.project, self.binary, self.record, lambda pid: current))
        self.assertIsNone(runtime.owned_tilt_identity(self.project, self.binary, dict(self.record, project_name="another-project"), lambda pid: self.record))
        self.assertIsNone(runtime.owned_tilt_identity(self.project, self.binary, self.record, lambda pid: None))

    def test_verified_tilt_receives_only_sigint_through_pidfd(self):
        with mock.patch.object(runtime.os, "pidfd_open", return_value=99) as open_pidfd:
            with mock.patch.object(runtime.os, "close") as close:
                with mock.patch.object(runtime.signal, "pidfd_send_signal") as send:
                    self.assertTrue(runtime.signal_owned_tilt(self.project, self.binary, self.record, lambda pid: self.record))
        open_pidfd.assert_called_once_with(4242)
        send.assert_called_once_with(99, runtime.signal.SIGINT)
        close.assert_called_once_with(99)

    def test_pid_reuse_between_initial_check_and_pidfd_is_not_signaled(self):
        reader = mock.Mock(side_effect=[self.record, dict(self.record, start_time="reused-pid")])
        with mock.patch.object(runtime.os, "pidfd_open", return_value=99):
            with mock.patch.object(runtime.os, "close") as close:
                with mock.patch.object(runtime.signal, "pidfd_send_signal") as send:
                    self.assertFalse(runtime.signal_owned_tilt(self.project, self.binary, self.record, reader))
        send.assert_not_called()
        close.assert_called_once_with(99)

    def test_down_waits_for_owned_tilt_and_preview_listeners_to_stop(self):
        runtime.save_tilt_record(self.project, self.record)
        active = [True]

        def signal_sender(project, binary, record, reader):
            active[0] = False
            return True

        ports = mock.Mock(side_effect=[runtime.RuntimeFailure("synthetic preview active"), None])
        sleeper = mock.Mock()
        runtime.stop_tracked_tilt(self.project, self.binary, lambda pid: self.record if active[0] else None, signal_sender, ports, sleeper)
        sleeper.assert_called_once_with(0.2)
        self.assertEqual(ports.call_args_list, [mock.call(runtime.TILT_HOST_PORTS), mock.call(runtime.TILT_HOST_PORTS)])

    def test_down_refuses_untracked_listeners_without_signaling_any_process(self):
        signal_sender = mock.Mock()
        with self.assertRaisesRegex(runtime.RuntimeFailure, "untracked Tilt or preview listener"):
            runtime.stop_tracked_tilt(self.project, self.binary, signal_sender=signal_sender, port_checker=mock.Mock(side_effect=runtime.RuntimeFailure("synthetic occupied port")))
        signal_sender.assert_not_called()

    def test_down_refuses_stale_record_listener_without_signaling_reused_pid(self):
        runtime.save_tilt_record(self.project, self.record)
        signal_sender = mock.Mock()
        with self.assertRaisesRegex(runtime.RuntimeFailure, "no unrelated process was signaled"):
            runtime.stop_tracked_tilt(self.project, self.binary, lambda pid: dict(self.record, start_time="reused"), signal_sender, mock.Mock(side_effect=runtime.RuntimeFailure("synthetic occupied port")))
        signal_sender.assert_not_called()

    def test_shutdown_deadline_never_falls_back_to_killing_unrelated_processes(self):
        runtime.save_tilt_record(self.project, self.record)
        signal_sender = mock.Mock(return_value=True)
        with self.assertRaisesRegex(runtime.RuntimeFailure, "30 seconds"):
            runtime.stop_tracked_tilt(self.project, self.binary, lambda pid: self.record, signal_sender, mock.Mock(side_effect=runtime.RuntimeFailure("synthetic occupied port")), mock.Mock(), mock.Mock(side_effect=[0, 31]))
        signal_sender.assert_called_once()

    def test_reset_refuses_verified_active_tilt_even_before_ui_binds(self):
        runtime.save_tilt_record(self.project, self.record)
        with self.assertRaisesRegex(runtime.RuntimeFailure, "Tilt process is active"):
            runtime.refuse_active_tracked_tilt(self.project, lambda pid: self.record)

    def test_linux_process_identity_identifies_the_current_process(self):
        identity = runtime.linux_process_identity(os.getpid())
        self.assertEqual(identity["pid"], os.getpid())
        self.assertEqual(identity["executable"], str(Path(sys.executable).resolve()))
        self.assertEqual(identity["checkout"], str(Path.cwd().resolve()))
        self.assertTrue(identity["start_time"].isdigit())

    def test_startup_waits_for_exact_pinned_tilt_exec_with_unchanged_start_time(self):
        child = mock.Mock(pid=4242)
        child.poll.return_value = None
        reader = mock.Mock(side_effect=[dict(self.record, executable="/synthetic/intermediate"), self.record])
        sleeper = mock.Mock()
        self.assertEqual(runtime.wait_for_tilt_exec(self.project, self.binary, child, reader, sleeper), self.record)
        sleeper.assert_called_once_with(0.02)

    def test_startup_rejects_pid_identity_change_and_is_bounded(self):
        child = mock.Mock(pid=4242)
        child.poll.return_value = None
        intermediate = dict(self.record, executable="/synthetic/intermediate")
        reader = mock.Mock(side_effect=[intermediate, dict(self.record, start_time="reused-pid")])
        with self.assertRaisesRegex(runtime.RuntimeFailure, "identity changed"):
            runtime.wait_for_tilt_exec(self.project, self.binary, child, reader, mock.Mock())
        with self.assertRaisesRegex(runtime.RuntimeFailure, "three seconds"):
            runtime.wait_for_tilt_exec(self.project, self.binary, child, lambda pid: intermediate, mock.Mock(), mock.Mock(side_effect=[0, 4]))

    @unittest.skipUnless(shutil.which("sleep") and hasattr(os, "pidfd_open") and hasattr(runtime.signal, "pidfd_send_signal"), "Linux pidfd and the sleep test utility are required")
    def test_real_owned_test_process_stops_via_pidfd_without_signaling_other_pids(self):
        binary = Path(shutil.which("sleep")).resolve()
        child = subprocess.Popen([str(binary), "30"], cwd=self.project.checkout)
        try:
            record = runtime.linux_process_identity(child.pid)
            record["project_name"] = self.project.project_name
            runtime.save_tilt_record(self.project, record)
            runtime.stop_tracked_tilt(self.project, binary, port_checker=lambda ports: None)
            self.assertEqual(child.wait(timeout=3), -runtime.signal.SIGINT)
        finally:
            if child.poll() is None:
                child.send_signal(runtime.signal.SIGINT)
                child.wait(timeout=3)


@unittest.skipUnless(shutil.which("openssl"), "OpenSSL is required for actual synthetic certificate checks")
class SyntheticCertificateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.project = runtime.ProjectRuntime.at(cls.temporary.name)
        scripts = cls.project.checkout / "scripts" / "dev"
        scripts.mkdir(parents=True)
        for filename in ("runtime.py", "certificates.sh"):
            shutil.copyfile(Path(runtime.__file__).parent / filename, scripts / filename)
        cls.script = scripts / "certificates.sh"
        cls.generated = subprocess.run(["bash", str(cls.script)], capture_output=True, text=True, timeout=30)
        if cls.generated.returncode:
            raise AssertionError(cls.generated.stderr)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_every_runtime_directory_and_private_file_has_restricted_permissions(self):
        for directory, child_directories, files in os.walk(self.project.directory):
            self.assertEqual(stat.S_IMODE(Path(directory).stat().st_mode), 0o700)
            for filename in files:
                self.assertEqual(stat.S_IMODE((Path(directory) / filename).stat().st_mode), 0o600)
        self.assertEqual(len(list(self.project.certificates.glob("*.key"))), 6)
        self.assertEqual(len(list(self.project.certificates.glob("*.crt"))), 6)

    def test_certificate_generation_is_idempotent(self):
        before = {path.name: path.read_bytes() for path in self.project.certificates.iterdir()}
        completed = subprocess.run(["bash", str(self.script)], capture_output=True, text=True, timeout=30)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual({path.name: path.read_bytes() for path in self.project.certificates.iterdir()}, before)

    def test_server_sans_and_fictional_device_identity_match_transport_contract(self):
        certificate = ssl._ssl._test_decode_cert(str(self.project.certificates / "server.crt"))
        self.assertEqual(set(certificate["subjectAltName"]), {("DNS", "receiver"), ("DNS", "localhost"), ("IP Address", "127.0.0.1")})
        for name, device_id in [("client", "device-1"), ("client-device-2", "device-2")]:
            decoded = ssl._ssl._test_decode_cert(str(self.project.certificates / (name + ".crt")))
            subject = dict(item for group in decoded["subject"] for item in group)
            issuer = dict(item for group in decoded["issuer"] for item in group)
            self.assertEqual(subject["commonName"], device_id)
            self.assertEqual(subject["organizationName"], "ChargeShare Synthetic Only")
            self.assertEqual(issuer["commonName"], "Tesla Motors Products CA")

    def authenticated_exchange(self, client_certificate):
        server_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        server_context.load_cert_chain(str(self.project.certificates / "server.crt"), str(self.project.certificates / "server.key"))
        server_context.load_verify_locations(cafile=str(self.project.certificates / "ca.crt"))
        server_context.verify_mode = ssl.CERT_REQUIRED
        client_context = ssl.create_default_context(cafile=str(self.project.certificates / "ca.crt"))
        if client_certificate:
            client_context.load_cert_chain(str(self.project.certificates / (client_certificate + ".crt")), str(self.project.certificates / (client_certificate + ".key")))
        outcomes = queue.Queue()
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.settimeout(3)
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            port = listener.getsockname()[1]

            def serve():
                try:
                    connection, address = listener.accept()
                    with connection:
                        connection.settimeout(3)
                        with server_context.wrap_socket(connection, server_side=True) as authenticated:
                            if authenticated.recv(4) == b"ping":
                                authenticated.sendall(b"pong")
                                outcomes.put(True)
                            else:
                                outcomes.put(False)
                except (ssl.SSLError, OSError):
                    outcomes.put(False)

            server = threading.Thread(target=serve, daemon=True)
            server.start()
            succeeded = False
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=3) as connection:
                    with client_context.wrap_socket(connection, server_hostname="localhost") as authenticated:
                        authenticated.sendall(b"ping")
                        succeeded = authenticated.recv(4) == b"pong"
            except (ssl.SSLError, OSError):
                succeeded = False
            server.join(timeout=5)
            self.assertFalse(server.is_alive())
            return succeeded and outcomes.get(timeout=1)

    def test_generated_test_trust_accepts_each_fictional_client(self):
        self.assertTrue(self.authenticated_exchange("client"))
        self.assertTrue(self.authenticated_exchange("client-device-2"))

    def test_generated_test_trust_rejects_untrusted_and_missing_client_certificate(self):
        self.assertFalse(self.authenticated_exchange("untrusted-client"))
        self.assertFalse(self.authenticated_exchange(None))


if __name__ == "__main__":
    unittest.main()
