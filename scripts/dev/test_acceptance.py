import contextlib
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import acceptance
import ingestion_runner
from runtime import ProjectRuntime, RuntimeFailure, SYNTHETIC_LABEL
from source_epoch import ensure_source_epoch, verify_source_epoch, write_private_json
from test_source_epoch import EpochDocker
from test_runtime import synthetic_pins


def complete_status():
    from test_viewing import complete_results
    golden = acceptance.PRICING_GOLDEN
    scopes = []
    for suffix, energy in [("a", "0.500000"), ("b", "0.250000")]:
        result = complete_results("owner-" + suffix, "vehicle-" + suffix, energy)
        scopes.append({"vehicle": "vehicle-" + suffix, "observed_ac": energy + " kWh", "eligible": energy + " kWh", "sessions": 1, "classifications": ["SharedCharger"], "classification_reasons": ["InsideGeofence"], "quality_flags": 0, "pricing": result["pricing"], "session_prices": [result["sessions"][0]["pricing"]]})
    return {"accepted": 6, "rejected": 0, "variants": 6, "positions": [{"partition": 0, "next_offset": 6}], "scopes": scopes,
        "golden_reference": {"exact_costs": golden["exact_costs"], "exact_subtotal": golden["exact_subtotal"], "rounded_subtotal": golden["rounded_subtotal"]},
        "actual_exact_subtotal": golden["exact_subtotal"], "actual_rounded_subtotal": golden["rounded_subtotal"]}



class AssertionTests(unittest.TestCase):
    def test_exact_observed_energy_scope_classification_progress_and_evidence_are_required(self):
        acceptance.assert_complete(complete_status())
        changes = [lambda value: value.update(accepted=5), lambda value: value.update(variants=7),
                   lambda value: value.update(rejected=1), lambda value: value.update(positions=[{"partition": 0, "next_offset": 7}]),
                   lambda value: value["scopes"][0].update(observed_ac="0.600000 kWh"),
                   lambda value: value["scopes"][1].update(vehicle="vehicle-a"),
                   lambda value: value["scopes"][1].update(eligible="0.000000 kWh"),
                   lambda value: value["scopes"][0].update(classifications=["Held"]),
                   lambda value: value["scopes"][0].update(sessions=2),
                   lambda value: value["scopes"][0].update(quality_flags=1), lambda value: value["scopes"][0]["pricing"].update(exact_subtotal="0.1997000000000000 USD"), lambda value: value.update(actual_exact_subtotal="0.2995500000000000 USD")]
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


    def test_coordinate_privacy_rejects_coordinate_values_and_location_fields(self):
        checkout = Path(__file__).resolve().parents[2]
        acceptance.assert_coordinate_privacy("SharedCharger InsideGeofence eligible=0.500000", checkout)
        for text in ('"latitude":12.3456789', '"locationValue":{}', '12.3456789', '999.1234501'):
            with self.assertRaisesRegex(acceptance.AcceptanceFailure, "coordinate-privacy"):
                acceptance.assert_coordinate_privacy(text, checkout)

    def test_location_acceptance_requires_exact_classification_reason_energy_and_safe_shape(self):
        expected = {
            "connection-1": ("SharedCharger", "InsideGeofence"),
            "location-edge": ("SharedCharger", "InsideGeofence"),
            "location-outside": ("OtherCharger", "OutsideGeofence"),
            "location-missing": ("Held", "LocationUnavailable"),
            "location-invalid": ("Held", "LocationUnavailable"),
            "location-mixed": ("Held", "LocationAmbiguous"),
            "location-partial": ("Held", "LocationAmbiguous"),
        }
        sessions = []
        for connection, (classification, reason) in expected.items():
            sessions.append({"vehicle":"vehicle-a", "connection":connection, "classification":classification, "classification_reason":reason, "evidence_state":"complete", "observed_ac_kwh":"0.500000", "eligible_shared_charger_kwh":"0.500000" if classification == "SharedCharger" else "0.000000", "quality_flags":[], "exclusion_reasons":[] if classification == "SharedCharger" else ["OtherCharger" if classification == "OtherCharger" else reason]})
        from test_viewing import complete_results
        for session in sessions:
            session["pricing"] = complete_results()["sessions"][0]["pricing"] if session["classification"] == "SharedCharger" else {"state": "ineligible", "hold_reason": "IneligibleSession", "quote": None}
        result = {"pricing": {}, "owner":"owner-a", "vehicle":"vehicle-a", "state":"held", "observed_ac_kwh":"3.500000", "eligible_shared_charger_kwh":"1.000000", "evidence_boundary":"synthetic", "revision":"synthetic", "provenance":{}, "sessions":sessions}
        acceptance.assert_location_sessions(result, expected, "1.000000")
        for change in [lambda value: value.update(eligible_shared_charger_kwh="0.000000"), lambda value: value["sessions"][0].update(classification="OtherCharger"), lambda value: value["sessions"][0].update(classification_reason="LocationUnavailable"), lambda value: value["sessions"][0].update(latitude=12.3456789), lambda value: value["sessions"][0].update(quality_flags=["ConflictingEvidence"]), lambda value: value["sessions"][2].update(pricing={"state": "quoted", "hold_reason": None, "quote": {"exact_cost": "0.1 USD"}})]:
            changed = json.loads(json.dumps(result))
            change(changed)
            with self.assertRaisesRegex(acceptance.AcceptanceFailure, "location-classification"):
                acceptance.assert_location_sessions(changed, expected, "1.000000")


class RunnerOwnershipTests(unittest.TestCase):
    def test_disposable_consumer_uses_private_network_pinned_image_and_owned_storage_without_host_ports(self):
        from test_source_epoch import EpochDocker
        from source_epoch import ensure_source_epoch

        with tempfile.TemporaryDirectory() as temporary:
            project = ProjectRuntime.at(temporary)
            dev = project.checkout / "dev"
            dev.mkdir()
            pins = synthetic_pins()
            (dev / "upstream-pins.json").write_text(json.dumps(pins))
            docker = EpochDocker(project)
            ensure_source_epoch(project, docker)
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

            for machine, expected_platform, field in (("x86_64", "linux/amd64", "reference"), ("aarch64", "linux/arm64", "arm64_reference")):
                with self.subTest(machine=machine), mock.patch("runtime.platform.machine", return_value=machine):
                    command = ingestion_runner.runner_command(project, "synthetic-runner", "synthetic-run", ["consume"], command_runner=command_runner)
                    self.assertEqual(command[command.index("--platform") + 1], expected_platform)
                    self.assertEqual(command[-2:], [pins["kafka_image"][field], "consume"])
            self.assertIn("--rm", command)
            self.assertIn("--read-only", command)
            self.assertEqual(command[command.index("--network") + 1], network["Name"])
            self.assertEqual(command[command.index("--user") + 1], str(os.getuid()) + ":" + str(os.getgid()))
            self.assertEqual(command[command.index("--entrypoint") + 1], "/run/ingestion")
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
        (self.runtime.checkout / "dev").mkdir()
        (self.runtime.checkout / "dev/upstream-pins.json").write_text(json.dumps(synthetic_pins()))
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
        for result in [subprocess.CompletedProcess([], 0, "", reason + "\n"), subprocess.CompletedProcess([], 2, "payload", reason + "\n"), subprocess.CompletedProcess([], 2, " \n", reason + "\n"), subprocess.CompletedProcess([], 2, "", "unrelated failure\n"), subprocess.CompletedProcess([], 2, "", reason + " \n")]:
            with mock.patch.object(acceptance.subprocess, "run", return_value=result), self.assertRaises(acceptance.AcceptanceFailure):
                self.driver.checked(["synthetic-command"], expected_reason=reason)
        with mock.patch.object(acceptance.subprocess, "run", return_value=subprocess.CompletedProcess([], 2, "", reason + "\n")):
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


class RefusalScenarioTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.runtime = ProjectRuntime.at(temporary.name)
        (self.runtime.checkout / "dev").mkdir()
        (self.runtime.checkout / "dev/upstream-pins.json").write_text(json.dumps(synthetic_pins()))
        self.docker = EpochDocker(self.runtime)
        self.source = ensure_source_epoch(self.runtime, self.docker)
        self.marker = self.runtime.directory / "source-epoch.json"
        self.manifest_path = self.runtime.directory / "manifest.json"
        self.manifest = json.loads((Path(__file__).resolve().parents[2] / "dev/ingestion-manifest.json").read_bytes())
        write_private_json(self.manifest_path, self.manifest)
        self.database = self.runtime.directory / "ingestion.sqlite3"
        self.database.write_bytes(b"fictional-retained-sqlite-evidence")
        self.before = complete_status()
        self.driver = acceptance.Acceptance(self.runtime)
        patches = contextlib.ExitStack()
        self.addCleanup(patches.close)
        patches.enter_context(mock.patch.object(acceptance, "verify_source_epoch", side_effect=lambda project: verify_source_epoch(project, self.docker)))
        self.runner = patches.enter_context(mock.patch.object(acceptance, "runner_command", return_value=["synthetic-consumer"]))
        patches.enter_context(mock.patch.object(self.driver, "remove_runner", side_effect=self.driver.runners.discard))
        self.status = patches.enter_context(mock.patch.object(self.driver, "status", return_value=self.before))
        self.process = patches.enter_context(mock.patch.object(acceptance.subprocess, "run"))

    def assert_restored(self):
        self.assertEqual(json.loads(self.marker.read_bytes()), self.source)
        self.assertEqual(json.loads(self.manifest_path.read_bytes()), self.manifest)
        self.assertEqual(self.database.read_bytes(), b"fictional-retained-sqlite-evidence")
        self.assertEqual(self.driver.runners, set())

    def test_changed_epoch_and_missing_marker_require_source_reason_and_independent_preservation_checks(self):
        observed = []

        def refuse(command, **arguments):
            observed.append(json.loads(self.marker.read_bytes()) if self.marker.exists() else None)
            return subprocess.CompletedProcess(command, 2, "", "source: broker runtime identity mismatch\n")

        self.process.side_effect = refuse
        with contextlib.redirect_stdout(io.StringIO()):
            self.driver.mismatch(self.before)
        self.assertNotEqual(observed[0]["epoch"], self.source["epoch"])
        self.assertIsNone(observed[1])
        self.assertEqual(self.status.call_count, 2)
        self.assert_restored()

    def test_each_source_refusal_fails_if_replay_evidence_or_progress_changes(self):
        self.process.return_value = subprocess.CompletedProcess([], 2, "", "source: broker runtime identity mismatch\n")
        for changed_at in (0, 1):
            with self.subTest(changed_at=changed_at):
                self.status.side_effect = [self.before] * changed_at + [dict(self.before, accepted=7)]
                with contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(acceptance.AcceptanceFailure, "source-mismatch"):
                    self.driver.mismatch(self.before)
                self.assert_restored()

    def test_wrong_epoch_reason_fails_and_restores_the_marker(self):
        self.process.return_value = subprocess.CompletedProcess([], 2, "", "storage: frozen configuration mismatch\n")
        with self.assertRaisesRegex(acceptance.AcceptanceFailure, "refusal"):
            self.driver.mismatch(self.before)
        self.assertEqual(self.process.call_count, 1)
        self.status.assert_not_called()
        self.assert_restored()

    def test_changed_manifest_requires_frozen_configuration_reason_and_preserves_sqlite(self):
        def refuse(command, **arguments):
            changed = json.loads(self.manifest_path.read_bytes())
            self.assertNotEqual(changed["manifest_version"], self.manifest["manifest_version"])
            self.assertEqual(changed["mappings"], self.manifest["mappings"])
            self.assertEqual(json.loads(self.marker.read_bytes()), self.source)
            return subprocess.CompletedProcess(command, 2, "", "storage: frozen configuration mismatch\n")

        self.process.side_effect = refuse
        with contextlib.redirect_stdout(io.StringIO()):
            self.driver.frozen_configuration(self.before)
        self.status.assert_called_once()
        self.assert_restored()

    def test_changed_manifest_restores_configuration_on_wrong_reason_or_changed_progress(self):
        for result, status, reason in [
            (subprocess.CompletedProcess([], 2, "", "source: broker runtime identity mismatch\n"), self.before, "refusal"),
            (subprocess.CompletedProcess([], 2, "", "storage: frozen configuration mismatch\n"), dict(self.before, positions=[{"partition": 0, "next_offset": 7}]), "frozen-configuration"),
        ]:
            with self.subTest(reason=reason):
                self.process.return_value = result
                self.status.return_value = status
                with self.assertRaisesRegex(acceptance.AcceptanceFailure, reason):
                    self.driver.frozen_configuration(self.before)
                self.assert_restored()

    def test_timeout_proof_requires_the_exact_consumer_deadline_and_preserves_state(self):
        self.process.return_value = subprocess.CompletedProcess([], 2, "", "ingestion: bounded deadline exceeded\n")
        with self.assertRaisesRegex(acceptance.AcceptanceFailure, "^timeout$"):
            self.driver.prove_timeout(self.before)
        self.assertEqual(self.runner.call_args.args[3], ["consume", "/run/.local-runtime", "kafka:9092", "1", "1"])
        self.status.assert_called_once()
        self.assert_restored()

    def test_timeout_proof_rejects_success_wrong_failure_runner_timeout_and_changed_state(self):
        for result, status, reason in [
            (subprocess.CompletedProcess([], 0, "", ""), self.before, "refusal"),
            (subprocess.CompletedProcess([], 2, "", "source: broker operation failed\n"), self.before, "refusal"),
            (subprocess.TimeoutExpired([], 45), self.before, "refusal"),
            (subprocess.CompletedProcess([], 2, "", "ingestion: bounded deadline exceeded\n"), dict(self.before, accepted=7), "timeout-progress"),
        ]:
            with self.subTest(reason=reason):
                self.process.side_effect = result if isinstance(result, Exception) else None
                self.process.return_value = result
                self.status.return_value = status
                with self.assertRaisesRegex(acceptance.AcceptanceFailure, "^" + reason + "$"):
                    self.driver.prove_timeout(self.before)
                self.assert_restored()


class ExecutionProofTests(unittest.TestCase):
    def test_each_proof_mode_reaches_its_own_injection_before_later_refusals(self):
        for mode in ("setup", "assertion", "timeout"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary, contextlib.ExitStack() as patches:
                runtime = ProjectRuntime.at(temporary)
                driver = acceptance.Acceptance(runtime, mode)
                driver.private.mkdir(parents=True)
                progress = 0

                def start():
                    runtime.directory.mkdir(mode=0o700)
                    write_private_json(runtime.directory / "last-run.json", {"starting_offsets": {"0": 0}})
                    write_private_json(runtime.directory / "decoded-capture.json", {"fictional": True})

                def checked(command, timeout=30, expected_reason=None):
                    if command == ["bash", "-c", "exit 7"]:
                        raise acceptance.AcceptanceFailure("setup")
                    return "f" * 40 if command == ["git", "rev-parse", "HEAD"] else ""

                def ingest(arguments, directory=None, expected_reason=None):
                    nonlocal progress
                    if arguments[0] == "consume" and expected_reason is None:
                        progress += int(arguments[3])
                    return ""

                def crash(boundary):
                    nonlocal progress
                    progress += int(boundary == "after")

                def status(directory=None):
                    return dict(complete_status(), accepted=progress, variants=min(progress, 6), positions=[{"partition": 0, "next_offset": progress}])

                patches.enter_context(mock.patch.object(acceptance, "preflight"))
                patches.enter_context(mock.patch.object(acceptance, "project_broker_volume", return_value=None))
                patches.enter_context(mock.patch.object(acceptance, "verify_source_epoch", return_value={"epoch": "synthetic-" + "a" * 32}))
                patches.enter_context(mock.patch.object(acceptance.ProjectRuntime, "at", return_value=runtime))
                patches.enter_context(mock.patch.object(acceptance, "Acceptance", return_value=driver))
                patches.enter_context(mock.patch.object(driver, "start", side_effect=start))
                patches.enter_context(mock.patch.object(driver, "checked", side_effect=checked))
                consumer = patches.enter_context(mock.patch.object(driver, "ingest", side_effect=ingest))
                patches.enter_context(mock.patch.object(driver, "crash", side_effect=crash))
                patches.enter_context(mock.patch.object(driver, "backup_gap"))
                patches.enter_context(mock.patch.object(driver, "status", side_effect=status))
                mismatch = patches.enter_context(mock.patch.object(driver, "mismatch", side_effect=acceptance.AcceptanceFailure("unexpected-refusal")))
                frozen = patches.enter_context(mock.patch.object(driver, "frozen_configuration"))
                cleanup = patches.enter_context(mock.patch.object(driver, "cleanup"))
                with contextlib.redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(acceptance.main(["all", "--prove-failure", mode]), 1)
                self.assertIn("FAIL: " + mode + "\n", output.getvalue())
                self.assertIn("Result: FAIL", output.getvalue())
                mismatch.assert_not_called()
                frozen.assert_not_called()
                cleanup.assert_called_once()
                if mode == "timeout":
                    consumer.assert_called_with(["consume", "/run/.local-runtime", "kafka:9092", "1", "1"], expected_reason="ingestion: bounded deadline exceeded")


if __name__ == "__main__":
    unittest.main()
