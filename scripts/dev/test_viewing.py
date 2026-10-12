import copy
import contextlib
import json
import unittest
import tempfile
from unittest import mock

import acceptance
import poc
import runtime
import viewing
import test_runtime as runtime_tests


def complete_results(owner="owner-a", vehicle="vehicle-a", energy="0.500000"):
    return {
        "owner": owner, "vehicle": vehicle, "state": "held", "evidence_boundary": "Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated",
        "observed_ac_kwh": energy, "eligible_shared_charger_kwh": "0.000000", "revision": "results-v1-0123456789abcdef",
        "provenance": {"source": "synthetic-receiver-kafka", "source_epoch": "synthetic-epoch", "input_contract": "tesla-v-bd076fe-v1", "mapping_version": "fictional-mapping-v1", "manifest_version": "fictional-manifest-v1", "normalizer_version": "chargeshare-normalizer-v1", "calculation_version": "scoped-core-replay-v1"},
        "sessions": [{"vehicle": vehicle, "connection": "connection-1", "classification": "Unconfirmed", "evidence_state": "complete", "observed_ac_kwh": energy, "eligible_shared_charger_kwh": "0.000000", "quality_flags": [], "exclusion_reasons": ["UnconfirmedCharger"]}],
    }


class HttpAcceptanceTests(unittest.TestCase):
    def test_scope_assertion_rejects_wrong_energy_leakage_quality_and_extra_payload(self):
        result = complete_results()
        viewing.assert_scoped_result(result, "owner-a", "vehicle-a", "0.500000")
        changes = [lambda value: value.update(observed_ac_kwh="0.600000"), lambda value: value.update(owner="owner-b"), lambda value: value.update(eligible_shared_charger_kwh="0.500000"), lambda value: value.update(raw_payload="fictional-private"), lambda value: value["sessions"][0].update(vehicle="vehicle-b"), lambda value: value["sessions"][0].update(quality_flags=["MissingBaseline"]), lambda value: value["sessions"][0].update(exclusion_reasons=[]), lambda value: value["provenance"].update(raw="fictional-private")]
        for change in changes:
            changed = copy.deepcopy(result)
            change(changed)
            with self.assertRaises(runtime.RuntimeFailure):
                viewing.assert_scoped_result(changed, "owner-a", "vehicle-a", "0.500000")

    def test_actual_http_queries_require_both_scopes_repeat_stability_and_safe_mismatch(self):
        first, second = complete_results(), complete_results("owner-b", "vehicle-b", "0.250000")
        with mock.patch.object(viewing, "scoped_results", side_effect=[first, first, second, second]), mock.patch.object(viewing, "http_document", return_value={"error": "scope_refused"}) as get:
            self.assertEqual(viewing.assert_http_scopes(), [first, second])
            get.assert_called_once_with("/api/poc/owners/owner-a/vehicles/vehicle-b/results", 404)
        changed = dict(first, revision="results-v1-aaaaaaaaaaaaaaaa")
        with mock.patch.object(viewing, "scoped_results", side_effect=[first, changed]), self.assertRaisesRegex(runtime.RuntimeFailure, "revision"):
            viewing.assert_http_scopes()
        with mock.patch.object(viewing, "scoped_results", return_value=first), self.assertRaises(runtime.RuntimeFailure):
            viewing.assert_http_scopes(True)

    def test_owning_runtime_publishes_only_api_loopback_and_uses_the_private_network(self):
        project = mock.Mock(project_name="chargeshare-local-synthetic")
        base = ["docker", "run", "--rm", "--network", "private-network", "--mount", "private-runtime", "image", "serve"]
        with mock.patch.object(viewing, "runner_command", return_value=base) as runner:
            command = viewing.owning_runtime_command(project, "synthetic-run")
        self.assertEqual(command[command.index("--publish") + 1], "127.0.0.1:8788:8788")
        self.assertNotIn("--publish-all", command)
        self.assertEqual(runner.call_args.args[3], ["serve", "/run/.local-runtime", "kafka:9092", "0.0.0.0:8788"])

    def test_acceptance_cleanup_stops_the_owner_before_other_runners_and_tilt(self):
        project = mock.Mock()
        project.checkout = runtime.Path("/synthetic")
        driver = acceptance.Acceptance(project)
        driver.started = True
        driver.runners = {"synthetic-runner"}
        observed = []
        with mock.patch.object(driver, "stop_viewing", side_effect=lambda: observed.append("owner")), mock.patch.object(driver, "remove_runner", side_effect=lambda name: observed.append("runner")), mock.patch.object(driver, "stop", side_effect=lambda: observed.append("tilt")):
            driver.cleanup()
        self.assertEqual(observed, ["owner", "runner", "tilt"])


class PersistedResetTests(unittest.TestCase):
    setUp = runtime_tests.RuntimeGuardTests.setUp
    tearDown = runtime_tests.RuntimeGuardTests.tearDown
    run_reset = runtime_tests.RuntimeGuardTests.run_reset

    def test_running_owner_refuses_reset_even_with_no_api_listener(self):
        self.runtime_owner = {"Id": "synthetic-owner", "Name": "/" + self.project.project_name + "-persisted-runtime", "Config": {"Labels": {runtime.SYNTHETIC_LABEL: "true", "dev.chargeshare.ingestion-run": self.project.project_name, "dev.chargeshare.persisted-project": self.project.project_name}}, "State": {"Running": True}}
        self.docker.containers.append(self.runtime_owner)
        runtime.ensure_runtime_directory(self.project)
        with self.assertRaisesRegex(runtime.RuntimeFailure, "persisted runtime is active"):
            self.run_reset()
        self.assertTrue(self.project.directory.exists())
        self.assertEqual(self.docker.mutations(), [])

    def test_unowned_runtime_name_or_label_never_allows_deletion(self):
        container = {"Id": "unrelated", "Name": "/unrelated", "Config": {"Labels": {"dev.chargeshare.persisted-project": self.project.project_name}}, "State": {"Running": False}}
        self.docker.containers.append(container)
        runtime.ensure_runtime_directory(self.project)
        with self.assertRaises(runtime.RuntimeFailure):
            self.run_reset()
        self.assertTrue(self.project.directory.exists())
        self.assertEqual(self.docker.mutations(), [])


class LifecycleChecks(unittest.TestCase):
    def test_readiness_failure_tears_down_only_the_attempted_owned_start(self):
        project = mock.Mock()
        project.checkout = runtime.Path("/synthetic")
        project.environment.return_value = {}
        lifecycle = poc.PersistedLifecycle(project)
        child = mock.Mock()
        child.poll.return_value = None
        with mock.patch.object(poc, "preflight"), mock.patch.object(poc.Path, "mkdir"), mock.patch.object(poc.Path, "open", mock.mock_open()), mock.patch.object(poc.subprocess, "Popen", return_value=child), mock.patch.object(poc, "run_command", return_value="ready"), mock.patch.object(poc.subprocess, "run", return_value=mock.Mock(returncode=1)), mock.patch.object(lifecycle, "down") as down:
            with self.assertRaisesRegex(runtime.RuntimeFailure, "readiness"):
                lifecycle.up()
            down.assert_called_once()
            child.terminate.assert_called_once()


class LifecycleConcurrencyTests(unittest.TestCase):
    def test_a_second_lifecycle_command_refuses_without_stopping_any_resources(self):
        with tempfile.TemporaryDirectory() as temporary:
            project = runtime.ProjectRuntime.at(temporary)
            with poc.exclusive_lifecycle(project):
                with self.assertRaisesRegex(runtime.RuntimeFailure, "active"):
                    with poc.exclusive_lifecycle(project):
                        self.fail("A competing lifecycle command acquired the lock")
            with poc.exclusive_lifecycle(project):
                pass

    def test_startup_preflight_refusal_never_tears_down_a_preexisting_stack(self):
        project = mock.Mock()
        project.environment.return_value = {}
        lifecycle = poc.PersistedLifecycle(project)
        with mock.patch.object(poc, "preflight", side_effect=runtime.RuntimeFailure("occupied")), mock.patch.object(lifecycle, "down") as down:
            with self.assertRaisesRegex(runtime.RuntimeFailure, "occupied"):
                lifecycle.up()
            down.assert_not_called()


class OwnerStartupTests(unittest.TestCase):
    def test_startup_interruption_cleans_up_the_new_owner_before_readiness(self):
        project = mock.Mock(project_name="synthetic-project")
        child = mock.Mock()
        child.poll.return_value = None
        with mock.patch.object(viewing, "check_local_docker"), mock.patch.object(viewing, "validate_runtime_directory"), mock.patch.object(viewing, "exclusive_synthetic_run_lock", return_value=contextlib.nullcontext()), mock.patch.object(viewing, "run_command", return_value=""), mock.patch.object(viewing, "owning_runtime_command", return_value=["synthetic-owner"]), mock.patch.object(viewing.subprocess, "Popen", return_value=child), mock.patch.object(viewing, "stop_owner") as stop, mock.patch.object(viewing, "http_document", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                viewing.serve(project)
            stop.assert_called_once_with(project)
            child.wait.assert_called_once_with(timeout=10)

    def test_existing_owner_is_refused_without_stopping_it(self):
        project = mock.Mock(project_name="synthetic-project")
        with mock.patch.object(viewing, "check_local_docker"), mock.patch.object(viewing, "validate_runtime_directory"), mock.patch.object(viewing, "exclusive_synthetic_run_lock", return_value=contextlib.nullcontext()), mock.patch.object(viewing, "run_command", return_value="existing-owner"), mock.patch.object(viewing.subprocess, "Popen") as spawn, mock.patch.object(viewing, "stop_owner") as stop:
            with self.assertRaisesRegex(runtime.RuntimeFailure, "already exists"):
                viewing.serve(project)
            spawn.assert_not_called()
            stop.assert_not_called()


if __name__ == "__main__":
    unittest.main()
