import json
import unittest

import readiness


def document(ready=False, update="ok", runtime="pending", error=""):
    return {"items": [{"metadata": {"name": "receiver-ready"}, "status": {
        "conditions": [{"type": "Ready", "status": "True" if ready else "False"}],
        "updateStatus": update,
        "runtimeStatus": runtime,
        "buildHistory": [{"error": error}],
    }}]}


class ReadinessTests(unittest.TestCase):
    def test_running_process_does_not_count_as_ready(self):
        state = readiness.resource_states(document(runtime="ok"), ["receiver-ready"])
        self.assertFalse(state["receiver-ready"]["ready"])

    def test_failed_update_exits_immediately_instead_of_waiting_for_deadline(self):
        with self.assertRaisesRegex(readiness.ReadinessFailure, "receiver-ready"):
            readiness.wait_ready(["receiver-ready"], 900, reader=lambda: document(update="error", error="never-emit-build-details"), sleeper=lambda duration: self.fail("Failed resource must not wait"), report=lambda *args, **kwargs: None)

    def test_ready_interface_succeeds(self):
        readiness.wait_ready(["receiver-ready"], 1, reader=lambda: document(ready=True), sleeper=lambda duration: self.fail("Ready resource must not wait"), report=lambda *args, **kwargs: None)

    def test_missing_resource_cannot_report_ready(self):
        self.assertFalse(readiness.resource_states({"items": []}, ["receiver-ready"])["receiver-ready"]["ready"])

    def test_status_report_excludes_build_details_and_unknown_values(self):
        state = readiness.resource_states(document(update="never-emit-status-details", error="never-emit-build-details"), ["receiver-ready"])
        self.assertNotIn("never-emit", json.dumps(state))
        self.assertTrue(state["receiver-ready"]["build_failed"])

    def test_known_failure_is_classified_without_emitting_raw_details(self):
        self.assertEqual(readiness.failure_categories("telemetry-harness failed: run isolation: another harness is active; never-emit-details"), ["exclusive-run-lock"])
        self.assertEqual(readiness.failure_categories("never-emit-arbitrary-log-details"), ["unclassified"])

    def test_dependency_blockers_and_health_are_allowlisted(self):
        value = document()
        status = value["items"][0]["status"]
        status["waiting"] = {"on": [{"name": "kafka"}, {"name": "never-emit-dependency-details"}]}
        status["composeResourceInfo"] = {"healthStatus": "Unhealthy"}
        state = readiness.resource_states(value, ["receiver-ready"])["receiver-ready"]
        self.assertEqual(state["blocked_by"], ["kafka"])
        self.assertEqual(state["health"], "unhealthy")

    def test_waiting_resource_expires_at_deadline(self):
        current = [0]

        def sleep(duration):
            current[0] += duration

        with self.assertRaisesRegex(readiness.ReadinessFailure, "2 seconds"):
            readiness.wait_ready(["receiver-ready"], 2, reader=lambda: document(), monotonic=lambda: current[0], sleeper=sleep, report=lambda *args, **kwargs: None)
        self.assertEqual(current[0], 2)


if __name__ == "__main__":
    unittest.main()
