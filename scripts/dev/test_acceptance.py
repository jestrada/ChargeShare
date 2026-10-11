import contextlib
import io
import json
import os
import subprocess
import tempfile
import unittest
from unittest import mock

import acceptance
import ingestion_runner
from runtime import ProjectRuntime, RuntimeFailure, SYNTHETIC_LABEL


def complete_status():
    return {
        "accepted": 6, "rejected": 0, "variants": 6,
        "positions": [{"partition": 0, "next_offset": 6}],
        "scopes": [{"vehicle": vehicle, "observed_ac": energy, "eligible": "0.000000 kWh", "sessions": 1, "unconfirmed": True, "quality_flags": 0}
                   for vehicle, energy in [("vehicle-a", "0.500000 kWh"), ("vehicle-b", "0.250000 kWh")]],
    }


class AssertionTests(unittest.TestCase):
    def test_exact_observed_energy_scope_classification_progress_and_evidence_are_required(self):
        acceptance.assert_complete(complete_status())
        changes = [lambda value: value.update(accepted=5), lambda value: value.update(variants=7),
                   lambda value: value.update(rejected=1), lambda value: value.update(positions=[{"partition": 0, "next_offset": 7}]),
                   lambda value: value["scopes"][0].update(observed_ac="0.600000 kWh"),
                   lambda value: value["scopes"][1].update(vehicle="vehicle-a"),
                   lambda value: value["scopes"][1].update(eligible="0.250000 kWh"),
                   lambda value: value["scopes"][0].update(unconfirmed=False),
                   lambda value: value["scopes"][0].update(sessions=2),
                   lambda value: value["scopes"][0].update(quality_flags=1)]
        for change in changes:
            value = complete_status()
            change(value)
            with self.assertRaisesRegex(acceptance.AcceptanceFailure, "assertion"):
                acceptance.assert_complete(value)

    def test_deliberate_wrong_expectation_fails_and_restored_expectation_passes(self):
        with self.assertRaisesRegex(acceptance.AcceptanceFailure, "assertion"):
            acceptance.assert_complete(complete_status(), True)
        acceptance.assert_complete(complete_status())

    def test_atomic_recovery_requires_evidence_variants_and_progress_together(self):
        value = dict(complete_status(), accepted=3, variants=3, positions=[{"partition": 0, "next_offset": 3}])
        acceptance.assert_progress(value, 3)
        value["accepted"] = 4
        with self.assertRaises(acceptance.AcceptanceFailure):
            acceptance.assert_progress(value, 3)


class RunnerOwnershipTests(unittest.TestCase):
    def test_disposable_consumer_uses_private_network_pinned_image_and_owned_storage_without_host_ports(self):
        from test_source_epoch import EpochDocker
        from source_epoch import ensure_source_epoch

        with tempfile.TemporaryDirectory() as temporary:
            project = ProjectRuntime.at(temporary)
            docker = EpochDocker(project)
            ensure_source_epoch(project, docker)
            dev = project.checkout / "dev"
            dev.mkdir()
            image = "apache/kafka:synthetic@sha256:" + "a" * 64
            (dev / "upstream-pins.json").write_text(json.dumps({"kafka_image": {"reference": image}}))
            binary = project.checkout / "target/debug/chargeshare-ingestion"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"fictional-binary-placeholder")
            binary.chmod(0o700)
            network = {"Name": project.project_name + "_default", "Driver": "bridge", "Labels": {
                "com.docker.compose.project": project.project_name, SYNTHETIC_LABEL: "true"}}

            def command_runner(command, environment):
                if command[:3] == ["docker", "network", "inspect"]:
                    return json.dumps([network])
                return docker(command, environment)

            command = ingestion_runner.runner_command(project, "synthetic-runner", "synthetic-run", ["consume"], command_runner=command_runner)
            self.assertIn("--rm", command)
            self.assertIn("--read-only", command)
            self.assertEqual(command[command.index("--network") + 1], network["Name"])
            self.assertEqual(command[command.index("--user") + 1], str(os.getuid()) + ":" + str(os.getgid()))
            self.assertEqual(command[command.index("--entrypoint") + 1], "/run/ingestion")
            self.assertEqual(command[-2:], [image, "consume"])
            self.assertIn("type=bind,src=" + str(project.directory) + ",dst=/run/.local-runtime", command)
            self.assertIn("type=bind,src=/nix/store,dst=/nix/store,readonly", command)
            self.assertFalse(any(option in command for option in ("--publish", "--publish-all", "-p", "-P", "--privileged")))

    def test_only_exact_owned_private_bridge_network_is_accepted(self):
        project = mock.Mock(project_name="synthetic-project")
        network = {"Name": "synthetic-project_default", "Driver": "bridge", "Labels": {
            "com.docker.compose.project": "synthetic-project", SYNTHETIC_LABEL: "true"}}
        ingestion_runner.validate_network(project, [network])
        for change in ({"Name": "unrelated"}, {"Driver": "host"}, {"Labels": {}}):
            with self.assertRaises(RuntimeFailure):
                ingestion_runner.validate_network(project, [dict(network, **change)])

    def test_signal_requires_exact_container_name_run_label_and_synthetic_ownership(self):
        container = {"Id": "synthetic-container", "Name": "/synthetic-runner", "Config": {"Labels": {
            SYNTHETIC_LABEL: "true", ingestion_runner.RUN_LABEL: "synthetic-run"}}}
        self.assertEqual(ingestion_runner.owned_runner([container], "synthetic-runner", "synthetic-run"), "synthetic-container")
        for name, run_id in [("unrelated-runner", "synthetic-run"), ("synthetic-runner", "unrelated-run")]:
            with self.assertRaises(RuntimeFailure):
                ingestion_runner.owned_runner([container], name, run_id)
        container["Config"]["Labels"][SYNTHETIC_LABEL] = "false"
        with self.assertRaises(RuntimeFailure):
            ingestion_runner.owned_runner([container], "synthetic-runner", "synthetic-run")


class CommandAndCleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.runtime = ProjectRuntime.at(self.temporary.name)
        self.driver = acceptance.Acceptance(self.runtime)

    def tearDown(self):
        self.temporary.cleanup()

    def test_setup_nonzero_and_timeout_are_failures_without_emitting_unrestricted_output(self):
        for result in [subprocess.CompletedProcess([], 7, "fictional-payload", "fictional-private-text"), subprocess.TimeoutExpired([], 1)]:
            with mock.patch.object(acceptance.subprocess, "run", side_effect=result if isinstance(result, Exception) else None, return_value=result):
                with self.assertRaises(acceptance.AcceptanceFailure) as failure:
                    self.driver.checked(["synthetic-command"], 1)
                self.assertNotIn("fictional", str(failure.exception))

    def test_expected_refusal_requires_specific_safe_reason_and_nonzero_exit(self):
        reason = "source: durable position outside retained input"
        for result in [subprocess.CompletedProcess([], 0, "", reason), subprocess.CompletedProcess([], 2, "payload", reason), subprocess.CompletedProcess([], 2, "", "unrelated failure")]:
            with mock.patch.object(acceptance.subprocess, "run", return_value=result), self.assertRaises(acceptance.AcceptanceFailure):
                self.driver.checked(["synthetic-command"], expected_reason=reason)
        with mock.patch.object(acceptance.subprocess, "run", return_value=subprocess.CompletedProcess([], 2, "", reason)):
            self.driver.checked(["synthetic-command"], expected_reason=reason)

    def test_main_preserves_setup_assertion_and_timeout_failure_and_always_tears_down_owned_run(self):
        for category in ("setup", "assertion", "timeout"):
            with mock.patch.object(acceptance.ProjectRuntime, "at", return_value=self.runtime), mock.patch.object(acceptance.Acceptance, "execute", side_effect=acceptance.AcceptanceFailure(category)), mock.patch.object(acceptance.Acceptance, "cleanup") as cleanup, contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(acceptance.main(["all"]), 1)
                cleanup.assert_called_once()
                self.assertIn("FAIL: " + category, output.getvalue())
                self.assertIn("Result: FAIL", output.getvalue())

    def test_teardown_failure_cannot_turn_into_a_passing_gate(self):
        with mock.patch.object(acceptance.ProjectRuntime, "at", return_value=self.runtime), mock.patch.object(acceptance.Acceptance, "execute"), mock.patch.object(acceptance.Acceptance, "cleanup", side_effect=RuntimeFailure("unrestricted detail")), contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(acceptance.main(["all"]), 1)
            self.assertIn("FAIL: owned-teardown", output.getvalue())
            self.assertNotIn("unrestricted", output.getvalue())

    def test_preexisting_runtime_is_never_removed_on_setup_failure(self):
        self.runtime.directory.mkdir()
        with mock.patch.object(acceptance.ProjectRuntime, "at", return_value=self.runtime), mock.patch.object(acceptance.Acceptance, "execute", side_effect=acceptance.AcceptanceFailure("fresh-owned-runtime-required")), mock.patch.object(acceptance.Acceptance, "cleanup") as cleanup, contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(acceptance.main(["all"]), 1)
            cleanup.assert_not_called()


if __name__ == "__main__":
    unittest.main()
