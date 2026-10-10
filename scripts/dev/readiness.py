import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

from runtime import ProjectRuntime, RuntimeFailure, SYNTHETIC_LABEL, run_command


RESOURCES = ("kafka", "broker-ready", "receiver", "rust-build", "frontend-install", "fixture-api", "fixture-dashboard", "receiver-ready")
STATES = {"none", "pending", "in_progress", "ok", "error", "not_applicable"}
HEALTH_STATES = {"starting", "healthy", "unhealthy"}
FAILURE_CATEGORIES = {
    "run isolation: another harness is active": "exclusive-run-lock",
    "synthetic local runtime validation:": "runtime-validation",
    "broker protocol metadata:": "broker-metadata",
    "synthetic topic setup:": "topic-setup",
    "readiness: receiver status": "receiver-status",
    "readiness: expected receiver authenticated status": "receiver-status",
    "Connection refused": "receiver-connect",
    "transport: receiver unavailable": "receiver-connect",
    "readiness: authenticated receiver TLS": "receiver-authentication",
    "Owned service health failed": "container-health",
}


class ReadinessFailure(Exception):
    pass


def resource_states(document, names):
    resources = {item["metadata"]["name"]: item.get("status", {}) for item in document.get("items", [])}
    result = {}
    for name in names:
        status = resources.get(name, {})
        history = status.get("buildHistory", [])
        waiting = status.get("waiting", {})
        health = status.get("composeResourceInfo", {}).get("healthStatus", "").lower()
        result[name] = {
            "ready": any(condition.get("type") == "Ready" and condition.get("status") == "True" for condition in status.get("conditions", [])),
            "update": status.get("updateStatus") if status.get("updateStatus") in STATES else "unknown",
            "runtime": status.get("runtimeStatus") if status.get("runtimeStatus") in STATES else "unknown",
            "build_failed": bool(history and history[0].get("error")),
            "health": health if health in HEALTH_STATES else "unknown",
            "blocked_by": sorted({item.get("name") for item in waiting.get("on", []) if item.get("name") in RESOURCES}),
        }
    return result


def failure_categories(output):
    return sorted({category for fragment, category in FAILURE_CATEGORIES.items() if fragment in output}) or ["unclassified"]


def report_failure_categories():
    try:
        completed = subprocess.run(["tilt", "logs", "broker-ready", "receiver-ready", "--source", "all", "--tail", "50"], capture_output=True, text=True, timeout=5)
        categories = failure_categories(completed.stdout + completed.stderr)
    except (OSError, subprocess.TimeoutExpired):
        categories = ["diagnostic-unavailable"]
    print("readiness failure categories: " + json.dumps(categories), flush=True)


def service_healths(runtime, containers, names):
    states = {name: {"running": False, "health": "missing", "exited": False} for name in names}
    observed = set()
    for container in containers:
        labels = container.get("Config", {}).get("Labels") or {}
        name = labels.get("com.docker.compose.service")
        if labels.get("com.docker.compose.project") != runtime.project_name or labels.get(SYNTHETIC_LABEL) != "true" or name not in {"kafka", "receiver"} or name in observed:
            raise ReadinessFailure("Cannot establish exact owned service health.")
        observed.add(name)
        if name in states:
            state = container.get("State", {})
            health = state.get("Health", {}).get("Status")
            states[name] = {
                "running": state.get("Running") is True,
                "health": health if health in HEALTH_STATES else "unknown",
                "exited": state.get("Status") in {"exited", "dead"},
            }
    return states


def read_service_health(runtime, names, command_runner=run_command):
    try:
        identifiers = command_runner(["docker", "ps", "--all", "--filter", "label=com.docker.compose.project=" + runtime.project_name, "--format", "{{.ID}}"], runtime.environment()).split()
        containers = json.loads(command_runner(["docker", "inspect", *identifiers], runtime.environment())) if identifiers else []
        return service_healths(runtime, containers, names)
    except (RuntimeFailure, json.JSONDecodeError):
        raise ReadinessFailure("Owned service health query failed.") from None


def wait_service_health(names, timeout, reader, monotonic=time.monotonic, sleeper=time.sleep, report=print):
    deadline = monotonic() + timeout
    previous = None
    next_report = 0
    while monotonic() < deadline:
        states = reader()
        now = monotonic()
        if states != previous or now >= next_report:
            report("owned service health: " + json.dumps(states, sort_keys=True), flush=True)
            previous = states
            next_report = now + 30
        failed = [name for name, state in states.items() if state["exited"] or state["health"] == "unhealthy"]
        if failed:
            raise ReadinessFailure("Owned service health failed: " + ", ".join(failed))
        if now < deadline and all(states[name]["running"] and states[name]["health"] == "healthy" for name in names):
            return
        sleeper(min(1, max(0, deadline - now)))
    raise ReadinessFailure("Owned service health exceeded " + str(timeout) + " seconds")


def read_resources():
    try:
        completed = subprocess.run(["tilt", "get", "uiresources", "-o", "json"], capture_output=True, text=True, timeout=5)
        if completed.returncode != 0:
            raise ReadinessFailure("Tilt resource query failed")
        return json.loads(completed.stdout)
    except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError):
        raise ReadinessFailure("Tilt resource query unavailable within five seconds") from None


def wait_ready(names, timeout, reader=read_resources, monotonic=time.monotonic, sleeper=time.sleep, report=print):
    deadline = monotonic() + timeout
    previous = None
    next_report = 0
    while monotonic() < deadline:
        states = resource_states(reader(), names)
        now = monotonic()
        if states != previous or now >= next_report:
            report("readiness: " + json.dumps(states, sort_keys=True), flush=True)
            previous = states
            next_report = now + 30
        failed = [name for name, state in states.items() if state["update"] == "error" or state["runtime"] == "error" or state["build_failed"]]
        if failed:
            raise ReadinessFailure("Owned resources failed: " + ", ".join(failed))
        if now < deadline and all(state["ready"] for state in states.values()):
            return
        sleeper(min(1, max(0, deadline - now)))
    raise ReadinessFailure("Owned resource readiness exceeded " + str(timeout) + " seconds")


def main():
    parser = argparse.ArgumentParser(description="Wait for owned Tilt interfaces and report only allowlisted resource state.")
    parser.add_argument("--timeout", type=int, required=True)
    parser.add_argument("--service-health", action="store_true")
    parser.add_argument("resources", nargs="+", choices=RESOURCES)
    options = parser.parse_args()
    if options.timeout <= 0 or options.timeout > 900:
        parser.error("timeout must be between 1 and 900 seconds")
    try:
        if options.service_health:
            if any(name not in {"kafka", "receiver"} for name in options.resources):
                parser.error("service health is restricted to kafka and receiver")
            runtime = ProjectRuntime.at(Path(__file__).resolve().parents[2])
            wait_service_health(options.resources, options.timeout, reader=lambda: read_service_health(runtime, options.resources))
        else:
            wait_ready(options.resources, options.timeout)
    except ReadinessFailure as failure:
        if not options.service_health:
            report_failure_categories()
        print("readiness failed: " + str(failure), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
