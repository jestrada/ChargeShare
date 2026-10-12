import argparse
import json
import signal
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

from ingestion_runner import RUN_LABEL, owned_runner, runner_command
from runtime import ProjectRuntime, RuntimeFailure, check_local_docker, exclusive_synthetic_run_lock, run_command, validate_runtime_directory


PRICING_GOLDEN = json.loads((Path(__file__).resolve().parents[2] / "dev/fixtures/pricing.json").read_bytes())


SCOPES = (("owner-a", "vehicle-a", "0.500000"), ("owner-b", "vehicle-b", "0.250000"))


def http_document(path, expected_status=200):
    request = urllib.request.Request("http://127.0.0.1:8788" + path, headers={"Cache-Control": "no-cache"})
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            status, body = response.status, response.read(131073)
            cache = response.headers.get("Cache-Control")
    except urllib.error.HTTPError as response:
        status, body = response.code, response.read(131073)
        cache = response.headers.get("Cache-Control")
    except (OSError, urllib.error.URLError):
        raise RuntimeFailure("Persisted API unavailable within five seconds.") from None
    if status != expected_status or len(body) > 131072 or cache != "no-store":
        raise RuntimeFailure("Scoped HTTP status, size or cache contract failed.")
    try:
        return json.loads(body)
    except (ValueError, UnicodeError):
        raise RuntimeFailure("Scoped HTTP contract is not valid JSON.") from None


def scoped_results(owner, vehicle):
    return http_document("/api/poc/owners/" + owner + "/vehicles/" + vehicle + "/results")


def assert_scoped_result(result, owner, vehicle, energy):
    shared = True
    eligible = energy if shared else "0.000000"
    allowed = {"owner", "vehicle", "state", "evidence_boundary", "observed_ac_kwh", "eligible_shared_charger_kwh", "sessions", "provenance", "revision", "pricing"}
    if set(result) != allowed or result["owner"] != owner or result["vehicle"] != vehicle or result["state"] != ("eligible" if shared else "held") or result["observed_ac_kwh"] != energy or result["eligible_shared_charger_kwh"] != eligible or result["evidence_boundary"] != "Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated":
        raise RuntimeFailure("Scoped persisted energy assertion failed.")
    expected = {"vehicle": vehicle, "connection": "connection-1", "classification": "SharedCharger" if shared else "OtherCharger", "classification_reason": "InsideGeofence" if shared else "OutsideGeofence", "evidence_state": "complete", "observed_ac_kwh": energy, "eligible_shared_charger_kwh": eligible, "quality_flags": [], "exclusion_reasons": [] if shared else ["OtherCharger"]}
    if len(result["sessions"]) != 1 or {key: value for key, value in result["sessions"][0].items() if key != "pricing"} != expected:
        raise RuntimeFailure("Scoped session identity, quality or hold assertion failed.")
    provenance = result["provenance"]
    if set(provenance) != {"source", "source_epoch", "input_contract", "mapping_version", "manifest_version", "normalizer_version", "calculation_version", "geofence_version"} or provenance["source"] != "synthetic-receiver-kafka" or provenance["calculation_version"] != "scoped-core-replay-v2" or not re.fullmatch(r"results-v1-[a-f0-9]{16}", result["revision"]) or any(not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", value) for value in provenance.values()):
        raise RuntimeFailure("Scoped semantic revision or safe provenance assertion failed.")

    assert_golden_price(result, 0 if owner == "owner-a" else 1)


def assert_golden_price(result, index, wrong_expectation=False):
    golden = PRICING_GOLDEN
    expected_cost = "0.9990000000000000 USD" if wrong_expectation else golden["exact_costs"][index]
    pricing = result["pricing"]
    expected = {"state": "quoted", "charger_id": "fictional-shared-charger", "calculation_version": "tou-linear-micro-kwh-v1", "estimate_boundary": "Synthetic PG&E bundled energy-only estimate; counter energy allocated by elapsed UTC time; excludes CCA/PCIA, baseline credit, daily Base Services Charge and taxes; not a bill", "priced_ac_kwh": result["observed_ac_kwh"], "exact_subtotal": expected_cost, "display_subtotal": golden["display_costs"][index], "quoted_session_count": 1, "rate_held_session_count": 0, "ineligible_session_count": 0}
    if {key: value for key, value in pricing.items() if key != "rate_models"} != expected:
        raise RuntimeFailure("Scoped exact EV2-A pricing assertion failed.")
    models = pricing["rate_models"]
    if len(models) != 1 or models[0]["id"] != golden["model_version"] or models[0]["plan"] != "PGE-EV2-A" or models[0]["timezone"] != "America/Los_Angeles" or models[0]["effective_from"] != "2026-03-01":
        raise RuntimeFailure("Scoped EV2-A model provenance assertion failed.")
    session_price = result["sessions"][0]["pricing"]
    quote = session_price["quote"]
    if set(session_price) != {"state", "hold_reason", "quote"} or session_price["state"] != "quoted" or session_price["hold_reason"] is not None or set(quote) != {"exact_cost", "display_cost", "slices"} or quote["exact_cost"] != expected_cost or quote["display_cost"] != golden["display_costs"][index] or len(quote["slices"]) != 2:
        raise RuntimeFailure("Scoped exact session quote assertion failed.")
    slice_energy = ("0.250000", "0.125000")[index]
    slice_cost = ("0.0563950000000000 USD", "0.0281975000000000 USD")[index]
    for slice_index, line in enumerate(quote["slices"]):
        expected_line = {"start_utc_seconds": str(1788264000 + slice_index * 300), "end_utc_seconds": str(1788264300 + slice_index * 300), "local_date": golden["local_date"], "local_start": "2026-09-01T05:" + ("00", "05")[slice_index] + ":00-07:00", "local_end": "2026-09-01T05:" + ("05", "10")[slice_index] + ":00-07:00", "utc_offset_seconds": "-25200", "season": golden["season"], "day_type": "Weekday", "period": golden["period"], "model_version": golden["model_version"], "allocated_ac_kwh": slice_energy, "usd_per_kwh": golden["usd_per_kwh"], "exact_cost": slice_cost}
        if line != expected_line:
            raise RuntimeFailure("Scoped exact interval quote assertion failed.")


def assert_http_scopes(wrong_expectation=False):
    results = []
    for index, (owner, vehicle, energy) in enumerate(SCOPES):
        result = scoped_results(owner, vehicle)
        assert_scoped_result(result, owner, vehicle, energy)
        assert_golden_price(result, index, wrong_expectation and index == 0)
        if result != scoped_results(owner, vehicle):
            raise RuntimeFailure("Unchanged scoped HTTP results changed semantic revision.")
        results.append(result)
    if http_document("/api/poc/owners/owner-a/vehicles/vehicle-b/results", 404) != {"error": "scope_refused"}:
        raise RuntimeFailure("Mismatched owner/vehicle scope was not refused safely.")
    return results


def wait_http_scopes(timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            return assert_http_scopes()
        except RuntimeFailure:
            time.sleep(0.1)
    raise RuntimeFailure("Scoped committed results did not arrive within the viewing deadline.")


def wait_committed_progress(runtime, starting_offset, count, timeout=30):
    import contextlib
    import sqlite3
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with contextlib.closing(sqlite3.connect("file:" + str(runtime.directory / "ingestion.sqlite3") + "?mode=ro", uri=True, timeout=0.25)) as database:
            progress = database.execute("SELECT next_offset FROM partition_progress WHERE partition=0").fetchone()
            accepted = database.execute("SELECT count(*) FROM deliveries WHERE partition=0 AND offset>=? AND offset<? AND rejection_reason IS NULL", (starting_offset, starting_offset + count)).fetchone()[0]
        if progress is not None and progress[0] >= starting_offset + count and accepted == count:
            return
        time.sleep(0.1)
    raise RuntimeFailure("Fresh receiver records did not reach committed SQLite progress.")


def runtime_name(runtime):
    return runtime.project_name + "-persisted-runtime"


def owning_runtime_command(runtime, run_id, command_runner=run_command):
    command = runner_command(runtime, runtime_name(runtime), run_id,
        ["serve", "/run/.local-runtime", "kafka:9092", "0.0.0.0:8788"], command_runner=command_runner)
    command[command.index("--rm"):command.index("--rm")] = ["--publish", "127.0.0.1:8788:8788", "--label", "dev.chargeshare.persisted-project=" + runtime.project_name]
    return command


def stop_owner(runtime):
    check_local_docker(runtime)
    name = runtime_name(runtime)
    identifiers = run_command(["docker", "ps", "--all", "--filter", "name=^/" + name + "$", "--format", "{{.ID}}"], runtime.environment()).split()
    if not identifiers:
        return
    containers = json.loads(run_command(["docker", "inspect", *identifiers], runtime.environment()))
    labels = containers[0].get("Config", {}).get("Labels") or {}
    if labels.get("dev.chargeshare.persisted-project") != runtime.project_name or labels.get(RUN_LABEL) != runtime.project_name:
        raise RuntimeFailure("Refusing to stop an unrelated persisted runtime.")
    identifier = owned_runner(containers, name, runtime.project_name)
    run_command(["docker", "stop", "--time", "5", identifier], runtime.environment())


def serve(runtime):
    check_local_docker(runtime)
    validate_runtime_directory(runtime)
    child = None
    previous = {}

    def shutdown(signum, frame):
        raise KeyboardInterrupt

    try:
        for signum in (signal.SIGTERM, signal.SIGINT):
            previous[signum] = signal.signal(signum, shutdown)
        with exclusive_synthetic_run_lock(runtime):
            existing = run_command(["docker", "ps", "--all", "--filter", "name=^/" + runtime_name(runtime) + "$", "--format", "{{.ID}}"], runtime.environment()).split()
            if existing:
                raise RuntimeFailure("Owning persisted runtime already exists; no process was stopped.")
            command = owning_runtime_command(runtime, runtime.project_name)
            child = subprocess.Popen(command, cwd=runtime.checkout, env=runtime.environment())
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if child.poll() is not None:
                    raise RuntimeFailure("Owning persisted runtime exited before readiness.")
                try:
                    http_document("/api/poc/ready")
                    break
                except RuntimeFailure:
                    time.sleep(0.1)
            else:
                raise RuntimeFailure("Owning persisted runtime readiness exceeded 30 seconds.")
        return child.wait()
    finally:
        try:
            if child is not None:
                stop_owner(runtime)
                child.wait(timeout=10)
        finally:
            for signum, handler in previous.items():
                signal.signal(signum, handler)


def check(runtime):
    http_document("/api/poc/ready")
    completed = subprocess.run(["bash", "scripts/dev/harness.sh", "complete"], cwd=runtime.checkout, env=runtime.environment(), timeout=180)
    if completed.returncode != 0:
        raise RuntimeFailure("Fresh receiver ACK and matching Kafka check failed.")
    run = json.loads((runtime.directory / "last-run.json").read_bytes())
    wait_committed_progress(runtime, run["starting_offsets"]["0"], 6)
    results = wait_http_scopes()
    with urllib.request.urlopen("http://127.0.0.1:5174", timeout=5) as response:
        if response.status != 200 or b"/src/persisted-main.tsx" not in response.read(131072):
            raise RuntimeFailure("Persisted dashboard entry is unavailable or incorrect.")
    print("PASS: fresh official receiver ACK + matching Kafka + committed SQLite + scoped HTTP + persisted dashboard")
    print("PASS: vehicle-a=0.500000 kWh vehicle-b=0.250000 kWh observed AC; automatic SharedCharger; eligible A=0.500000/B=0.250000 kWh; EV2-A exact prices 0.11279/0.056395 USD; mismatched scope refused; revision stable")
    return results


def main():
    parser = argparse.ArgumentParser(description="Owned persisted runtime and finite scoped HTTP verification.")
    parser.add_argument("action", choices=("serve", "stop-owner", "check"))
    options = parser.parse_args()
    runtime = ProjectRuntime.at(Path(__file__).resolve().parents[2])
    try:
        if options.action == "serve":
            return serve(runtime)
        if options.action == "stop-owner":
            stop_owner(runtime)
        else:
            check(runtime)
    except KeyboardInterrupt:
        print("Persisted viewing: owning runtime interrupted.", file=sys.stderr)
        return 130
    except (RuntimeFailure, OSError, ValueError, KeyError, subprocess.TimeoutExpired):
        print("Persisted viewing: bounded runtime or viewing check failed.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
