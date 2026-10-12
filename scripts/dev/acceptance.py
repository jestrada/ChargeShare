import argparse
import contextlib
import fcntl
import json
import os
import shutil
import signal
import sqlite3
import subprocess
import sys
import time
import uuid
from pathlib import Path

from ingestion_runner import owned_runner, runner_command, runner_name
from runtime import ProjectRuntime, RuntimeFailure, check_ports_available, compose_command, exclusive_synthetic_run_lock, preflight, project_broker_volume, run_command
from source_epoch import TOPIC, read_marker, verify_source_epoch, write_private_json
from viewing import SCOPES, assert_http_scopes, http_document, owning_runtime_command, runtime_name, scoped_results, wait_http_scopes


class AcceptanceFailure(Exception):
    pass


def fail_at_deadline(signum, frame):
    raise AcceptanceFailure("timeout")


def assert_complete(document, wrong_expectation=False):
    expected = ["0.600000 kWh" if wrong_expectation else "0.500000 kWh", "0.250000 kWh"]
    if set(document) != {"scopes", "positions", "accepted", "rejected", "variants"} or document["accepted"] != 6 or document["rejected"] != 0 or document["variants"] != 6 or document["positions"] != [{"partition": 0, "next_offset": 6}]:
        raise AcceptanceFailure("assertion")
    scopes = document["scopes"]
    if len(scopes) != 2:
        raise AcceptanceFailure("assertion")
    for index, scope in enumerate(scopes):
        if scope != {"vehicle": ("vehicle-a", "vehicle-b")[index], "observed_ac": expected[index], "eligible": "0.000000 kWh", "sessions": 1, "unconfirmed": True, "quality_flags": 0}:
            raise AcceptanceFailure("assertion")


def assert_progress(document, offset):
    if document["positions"] != [{"partition": 0, "next_offset": offset}] or document["accepted"] != offset or document["rejected"] != 0 or document["variants"] != offset:
        raise AcceptanceFailure("atomic-recovery")


class Acceptance:
    def __init__(self, runtime, prove_failure="none"):
        self.runtime = runtime
        self.prove_failure = prove_failure
        self.run_id = uuid.uuid4().hex
        self.output = runtime.checkout / "target" / "ingestion-acceptance-results"
        self.private = runtime.checkout / ".cache" / "ingestion-acceptance"
        self.started = False
        self.tilt = None
        self.runners = set()
        self.viewing_process = None
        self.stage = "setup"
        self.summary = []

    def record(self, line):
        self.summary.append(line)
        print(line, flush=True)
        self.output.mkdir(parents=True, exist_ok=True)
        (self.output / "summary.md").write_text("\n".join(self.summary) + "\n", encoding="utf-8")

    def checked(self, command, timeout=30, expected_reason=None):
        try:
            result = subprocess.run(command, cwd=self.runtime.checkout, env=self.runtime.environment(), capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            raise AcceptanceFailure("refusal" if expected_reason is not None else "timeout") from None
        except OSError:
            raise AcceptanceFailure("setup") from None
        self.private.mkdir(mode=0o700, parents=True, exist_ok=True)
        with (self.private / "commands-local.log").open("a", encoding="utf-8") as output:
            output.write(result.stdout + result.stderr)
        if expected_reason is not None:
            if result.returncode != 2 or result.stdout or result.stderr != expected_reason + "\n":
                raise AcceptanceFailure("refusal")
        elif result.returncode != 0:
            raise AcceptanceFailure(self.stage)
        return result.stdout

    def start(self):
        self.stage = "startup"
        self.started = True
        self.private.mkdir(mode=0o700, parents=True, exist_ok=True)
        environment = self.runtime.environment()
        environment["CHARGESHARE_PERSISTED_RUNTIME"] = "0"
        with (self.private / "tilt-local.log").open("a", encoding="utf-8") as output:
            self.tilt = subprocess.Popen(["tilt", "up", "--stream"], cwd=self.runtime.checkout, env=environment, stdout=output, stderr=output)
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            if self.tilt.poll() is not None:
                raise AcceptanceFailure("startup")
            try:
                run_command(["tilt", "get", "uiresource", "(Tiltfile)", "-o", "name"], self.runtime.environment())
                break
            except RuntimeFailure:
                time.sleep(1)
        else:
            raise AcceptanceFailure("timeout")
        self.checked(["python3", "scripts/dev/readiness.py", "--timeout", "900", "kafka", "broker-ready", "receiver", "rust-build", "receiver-ready"], 910)
        self.record("PASS: pinned receiver and private Kafka ready through Tilt")

    def stop(self):
        self.stage = "shutdown"
        self.checked(["tilt", "down"], 130)
        if self.tilt is not None:
            try:
                self.tilt.wait(timeout=10)
            except subprocess.TimeoutExpired:
                raise AcceptanceFailure("shutdown") from None
        self.started = False
        check_ports_available()

    def ingest(self, arguments, directory=None, expected_reason=None):
        name = runner_name(self.runtime)
        with exclusive_synthetic_run_lock(self.runtime):
            command = runner_command(self.runtime, name, self.run_id, arguments, directory)
            self.runners.add(name)
            try:
                return self.checked(command, 45, expected_reason)
            finally:
                self.remove_runner(name)

    def remove_runner(self, name):
        identifiers = run_command(["docker", "ps", "--all", "--filter", "name=^/" + name + "$", "--format", "{{.ID}}"], self.runtime.environment()).split()
        if identifiers:
            containers = json.loads(run_command(["docker", "inspect", *identifiers], self.runtime.environment()))
            identifier = owned_runner(containers, name, self.run_id)
            run_command(["docker", "rm", "--force", identifier], self.runtime.environment())
        self.runners.discard(name)

    def status(self, directory=None):
        try:
            return json.loads(self.ingest(["acceptance-status", "/run/.local-runtime"], directory))
        except (ValueError, TypeError):
            raise AcceptanceFailure("assertion") from None

    def crash(self, boundary):
        self.stage = "crash-" + boundary
        marker = self.runtime.directory / "commit-boundary.json"
        if marker.exists():
            marker.unlink()
        name = runner_name(self.runtime)
        with exclusive_synthetic_run_lock(self.runtime):
            command = runner_command(self.runtime, name, self.run_id, ["consume-probe", "/run/.local-runtime", "kafka:9092", boundary])
            self.runners.add(name)
            with (self.private / "crash-local.log").open("a", encoding="utf-8") as output:
                child = subprocess.Popen(command, cwd=self.runtime.checkout, env=self.runtime.environment(), stdout=output, stderr=output)
            try:
                deadline = time.monotonic() + 20
                while not marker.exists():
                    if child.poll() is not None:
                        raise AcceptanceFailure(self.stage)
                    if time.monotonic() >= deadline:
                        raise AcceptanceFailure("timeout")
                    time.sleep(0.05)
                if json.loads(marker.read_bytes()) != boundary:
                    raise AcceptanceFailure(self.stage)
                containers = json.loads(run_command(["docker", "inspect", name], self.runtime.environment()))
                identifier = owned_runner(containers, name, self.run_id)
                run_command(["docker", "kill", "--signal", "KILL", identifier], self.runtime.environment())
                if child.wait(timeout=10) != 137:
                    raise AcceptanceFailure(self.stage)
            finally:
                self.remove_runner(name)
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)

    def backup_gap(self):
        directory = self.runtime.directory / "acceptance-gap" / ".local-runtime"
        directory.mkdir(mode=0o700, parents=True)
        directory.parent.chmod(0o700)
        for name in ("source-epoch.json", "synthetic-runtime.json", "manifest.json"):
            shutil.copyfile(self.runtime.directory / name, directory / name)
            (directory / name).chmod(0o600)
        database = directory / "ingestion.sqlite3"
        with contextlib.closing(sqlite3.connect("file:" + str(self.runtime.directory / "ingestion.sqlite3") + "?mode=ro", uri=True)) as source:
            with contextlib.closing(sqlite3.connect(database)) as destination:
                source.backup(destination)
        database.chmod(0o600)
        return directory

    def retention_gap(self, directory, before, retained_start):
        self.stage = "retention-gap"
        request = self.runtime.directory / "delete-records.json"
        write_private_json(request, {"partitions": [{"topic": TOPIC, "partition": 0, "offset": retained_start}], "version": 1})
        self.checked(compose_command(self.runtime, "cp", str(request), "kafka:/tmp/chargeshare-delete-records.json"))
        self.checked(compose_command(self.runtime, "exec", "--no-TTY", "--user", "0", "kafka", "/opt/kafka/bin/kafka-delete-records.sh", "--bootstrap-server", "kafka:9092", "--offset-json-file", "/tmp/chargeshare-delete-records.json"), 30)
        low = self.checked(compose_command(self.runtime, "exec", "--no-TTY", "kafka", "/opt/kafka/bin/kafka-get-offsets.sh", "--bootstrap-server", "kafka:9092", "--topic", TOPIC, "--time", "-2"), 30)
        if low.strip() != TOPIC + ":0:" + str(retained_start):
            raise AcceptanceFailure("retention-gap")
        self.ingest(["consume", "/run/.local-runtime", "kafka:9092", "1", "10"], directory, "source: durable position outside retained input")
        if self.status(directory) != before:
            raise AcceptanceFailure("retention-gap")
        self.record("PASS: actual broker retention gap refused; SQLite evidence and progress preserved")

    def mismatch(self, before):
        self.stage = "source-mismatch"
        marker = self.runtime.directory / "source-epoch.json"
        source = read_marker(self.runtime)
        changed = dict(source, epoch="synthetic-" + uuid.uuid4().hex)
        write_private_json(marker, changed)
        try:
            try:
                verify_source_epoch(self.runtime)
            except RuntimeFailure:
                pass
            else:
                raise AcceptanceFailure("source-mismatch")
            name = runner_name(self.runtime)
            write_private_json(marker, source)
            command = runner_command(self.runtime, name, self.run_id, ["consume", "/run/.local-runtime", "kafka:9092", "1", "10"])
            write_private_json(marker, changed)
            self.runners.add(name)
            self.checked(command, 15, "source: broker runtime identity mismatch")
        finally:
            write_private_json(marker, source)
            for name in list(self.runners):
                self.remove_runner(name)
        if self.status() != before:
            raise AcceptanceFailure("source-mismatch")
        self.record("PASS: changed source epoch refused; SQLite evidence and progress preserved")
        saved = marker.with_name("source-epoch.saved.json")
        name = runner_name(self.runtime)
        command = runner_command(self.runtime, name, self.run_id, ["consume", "/run/.local-runtime", "kafka:9092", "1", "10"])
        marker.rename(saved)
        try:
            try:
                verify_source_epoch(self.runtime)
            except RuntimeFailure:
                pass
            else:
                raise AcceptanceFailure("source-mismatch")
            self.runners.add(name)
            self.checked(command, 15, "source: broker runtime identity mismatch")
        finally:
            saved.rename(marker)
            self.remove_runner(name)
        if self.status() != before:
            raise AcceptanceFailure("source-mismatch")
        self.record("PASS: missing source marker refused; SQLite evidence and progress preserved")

    def frozen_configuration(self, before):
        self.stage = "frozen-configuration"
        path = self.runtime.directory / "manifest.json"
        manifest = json.loads(path.read_bytes())
        changed = dict(manifest, manifest_version=manifest["manifest_version"] + "-incompatible")
        name = runner_name(self.runtime)
        command = runner_command(self.runtime, name, self.run_id, ["consume", "/run/.local-runtime", "kafka:9092", "1", "10"])
        try:
            write_private_json(path, changed)
            self.runners.add(name)
            self.checked(command, 15, "storage: frozen configuration mismatch")
        finally:
            write_private_json(path, manifest)
            self.remove_runner(name)
        if self.status() != before:
            raise AcceptanceFailure("frozen-configuration")
        self.record("PASS: incompatible frozen manifest refused; SQLite evidence and progress preserved")

    def prove_timeout(self, before):
        self.stage = "timeout"
        self.ingest(["consume", "/run/.local-runtime", "kafka:9092", "1", "1"], expected_reason="ingestion: bounded deadline exceeded")
        if self.status() != before:
            raise AcceptanceFailure("timeout-progress")
        raise AcceptanceFailure("timeout")

    def start_viewing(self):
        self.stage = "viewing-startup"
        name = runtime_name(self.runtime)
        command = owning_runtime_command(self.runtime, self.run_id)
        self.runners.add(name)
        with (self.private / "viewing-local.log").open("a", encoding="utf-8") as output:
            self.viewing_process = subprocess.Popen(command, cwd=self.runtime.checkout, env=self.runtime.environment(), stdout=output, stderr=output)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if self.viewing_process.poll() is not None:
                raise AcceptanceFailure("viewing-startup")
            try:
                http_document("/api/poc/ready")
                return
            except RuntimeFailure:
                time.sleep(0.1)
        raise AcceptanceFailure("timeout")

    def stop_viewing(self):
        if self.viewing_process is not None:
            self.remove_runner(runtime_name(self.runtime))
            try:
                self.viewing_process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                raise AcceptanceFailure("owned-teardown") from None
            self.viewing_process = None

    def viewing(self):
        self.start_viewing()
        empty = [scoped_results(owner, vehicle) for owner, vehicle, energy in SCOPES]
        if any(result["state"] != "empty" or result["sessions"] for result in empty):
            raise AcceptanceFailure("viewing-empty")
        self.stage = "viewing-receiver-fixture"
        self.checked(["bash", "scripts/dev/harness.sh", "complete"], 180)
        if json.loads((self.runtime.directory / "last-run.json").read_bytes())["starting_offsets"] != {"0": 0}:
            raise AcceptanceFailure("stale-fixture")
        try:
            complete = wait_http_scopes()
            if any(before["revision"] == after["revision"] for before, after in zip(empty, complete)):
                raise AcceptanceFailure("viewing-revision")
            self.record("PASS: owning runtime ingested fresh official receiver output; HTTP scopes A=0.500000/B=0.250000 kWh; Unconfirmed; eligible=0.000000 kWh; mismatched scope refused; semantic revision stable")
            self.stage = "viewing-concurrent-replay"
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(max_workers=4) as readers:
                pending = [readers.submit(assert_http_scopes) for _ in range(8)]
                self.checked(["bash", "scripts/dev/harness.sh", "adverse"], 300)
                if any(request.result(timeout=15) != complete for request in pending):
                    raise AcceptanceFailure("viewing-concurrent-snapshot")
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                with contextlib.closing(sqlite3.connect("file:" + str(self.runtime.directory / "ingestion.sqlite3") + "?mode=ro", uri=True)) as database:
                    progress = database.execute("SELECT next_offset FROM partition_progress WHERE partition=0").fetchone()
                if progress == (20,):
                    break
                time.sleep(0.1)
            else:
                raise AcceptanceFailure("viewing-commit-progress")
            if assert_http_scopes() != complete:
                raise AcceptanceFailure("viewing-replay")
            self.stop_viewing()
            self.start_viewing()
            if assert_http_scopes() != complete:
                raise AcceptanceFailure("viewing-restart")
            self.record("PASS: concurrent scoped HTTP reads, receiver duplicates/out-of-order replay and owning-process restart preserved coherent committed results and revisions")
            html = self.checked(["curl", "--fail", "--max-time", "5", "--silent", "--show-error", "http://127.0.0.1:5174"], 10)
            if "/src/persisted-main.tsx" not in html:
                raise AcceptanceFailure("viewing-dashboard-entry")
            self.record("PASS: separate persisted dashboard listener ready on IPv4 loopback 5174; browser screenshots remain manual")
        except RuntimeFailure:
            raise AcceptanceFailure(self.stage) from None
        finally:
            self.stop_viewing()

    def execute(self):
        preflight(self.runtime)
        if self.runtime.directory.exists() or project_broker_volume(self.runtime, run_command) is not None:
            raise AcceptanceFailure("fresh-owned-runtime-required")
        if self.prove_failure == "setup":
            self.checked(["bash", "-c", "exit 7"])
        self.record("Commit: " + self.checked(["git", "rev-parse", "HEAD"]).strip())
        self.record("Receiver: bd076fe1494841707528449560c4a19d0d426da4; topic=chargeshare_synthetic_V; contract=tesla-v-bd076fe-v1; manifest=fictional-manifest-v1")
        self.record("Trust: synthetic mutual TLS; client trusts generated local CA; unpatched receiver adds local CA to upstream production trust")
        self.record("Boundary: observed fictional AC counters; local SQLite WAL/FULL recovery; physical accuracy unvalidated; fixture dashboard independent")
        self.start()
        first_epoch = verify_source_epoch(self.runtime)["epoch"]
        self.record("PASS: owned source epoch initialized; partition=0 initial_offset=0 epoch=" + first_epoch)
        self.stage = "receiver-fixture"
        self.checked(["bash", "scripts/dev/harness.sh", "complete"], 180)
        run = json.loads((self.runtime.directory / "last-run.json").read_bytes())
        if run["starting_offsets"] != {"0": 0}:
            raise AcceptanceFailure("stale-fixture")
        self.ingest(["verify-decoded", "/run/.local-runtime"])
        shutil.copyfile(self.runtime.directory / "decoded-capture.json", self.private / "decoded-capture.json")
        (self.private / "decoded-capture.json").chmod(0o600)
        self.record("PASS: actual decoded records match allowlist, UTC source times and manifest associations; private capture retained for golden review")
        self.stage = "consume-restart"
        self.ingest(["consume", "/run/.local-runtime", "kafka:9092", "2", "30"])
        assert_progress(self.status(), 2)
        self.ingest(["consume", "/run/.local-runtime", "kafka:9092", "1", "30"])
        assert_progress(self.status(), 3)
        self.record("PASS: independent consumer restart resumed SQLite next position")
        self.crash("before")
        assert_progress(self.status(), 3)
        gap_directory = self.backup_gap()
        gap_before = self.status(gap_directory)
        self.crash("after")
        assert_progress(self.status(), 4)
        self.ingest(["consume", "/run/.local-runtime", "kafka:9092", "2", "30"])
        complete = self.status()
        assert_complete(complete, self.prove_failure == "assertion")
        self.record("PASS: real consumer kills before/after commit recovered atomic old/new state without duplicated energy")
        self.record("PASS: vehicle-a=0.500000 kWh vehicle-b=0.250000 kWh observed AC; both Unconfirmed; eligible=0.000000 kWh")
        if self.prove_failure == "timeout":
            self.prove_timeout(complete)
        self.stage = "duplicate-out-of-order"
        self.checked(["bash", "scripts/dev/harness.sh", "adverse"], 300)
        self.ingest(["consume", "/run/.local-runtime", "kafka:9092", "14", "30"])
        repeated = self.status()
        if repeated != dict(complete, accepted=20, positions=[{"partition": 0, "next_offset": 20}]):
            raise AcceptanceFailure("duplicate-out-of-order")
        complete = repeated
        self.record("PASS: actual duplicate and out-of-order receiver records retained without duplicating energy")
        self.stage = "reread"
        for _ in range(2):
            self.ingest(["reread", "/run/.local-runtime", "kafka:9092", "0", "0", "20", "30"])
            if self.status() != complete:
                raise AcceptanceFailure("reread")
        self.record("PASS: bounded committed-offset reread left results, evidence and SQLite next position unchanged")
        self.mismatch(complete)
        self.frozen_configuration(complete)
        self.stop()
        self.start()
        if verify_source_epoch(self.runtime)["epoch"] != first_epoch or self.status() != complete:
            raise AcceptanceFailure("restart-preservation")
        self.ingest(["reread", "/run/.local-runtime", "kafka:9092", "0", "0", "20", "30"])
        self.record("PASS: ordinary Tilt stop/start preserved epoch, retained input, SQLite progress and energy")
        self.retention_gap(gap_directory, gap_before, 20)
        self.ingest(["reread", "/run/.local-runtime", "kafka:9092", "0", "0", "20", "10"], expected_reason="source: durable position outside retained input")
        if self.status() != complete:
            raise AcceptanceFailure("retention-gap")
        self.stop()
        self.stage = "owned-reset"
        self.checked(["python3", "scripts/dev/runtime.py", "reset", "--confirm-synthetic-reset"], 60)
        self.start()
        second_epoch = verify_source_epoch(self.runtime)["epoch"]
        if first_epoch == second_epoch or (self.runtime.directory / "ingestion.sqlite3").exists():
            raise AcceptanceFailure("owned-reset")
        self.record("PASS: explicit stopped owned reset replaced source epoch=" + second_epoch)
        self.viewing()

    def cleanup(self):
        self.stop_viewing()
        for name in list(self.runners):
            self.remove_runner(name)
        if self.started:
            self.stop()
        if self.runtime.directory.exists() and self.tilt is not None:
            self.checked(["python3", "scripts/dev/runtime.py", "reset", "--confirm-synthetic-reset"], 60)


def main(arguments=None):
    parser = argparse.ArgumentParser(description="Bounded receiver-to-SQLite acceptance using owned Tilt resources.")
    parser.add_argument("mode", choices=("all",))
    parser.add_argument("--prove-failure", choices=("none", "setup", "assertion", "timeout"), default="none")
    options = parser.parse_args(arguments)
    runtime = ProjectRuntime.at(Path(__file__).resolve().parents[2])
    acceptance = Acceptance(runtime, options.prove_failure)
    lock_path = acceptance.private / "acceptance.lock"
    acceptance.private.mkdir(mode=0o700, parents=True, exist_ok=True)
    descriptor = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    result = 1
    locked = False
    owned = False
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            locked = True
        except BlockingIOError:
            raise AcceptanceFailure("concurrent-acceptance") from None
        acceptance.record("Synthetic receiver-to-SQLite acceptance")
        signal.signal(signal.SIGTERM, fail_at_deadline)
        owned = not runtime.directory.exists()
        acceptance.execute()
        result = 0
    except (AcceptanceFailure, RuntimeFailure, OSError, ValueError, KeyError, subprocess.TimeoutExpired, KeyboardInterrupt) as failure:
        acceptance.record("FAIL: " + (str(failure) if isinstance(failure, AcceptanceFailure) else "runtime-or-prerequisite"))
    finally:
        if locked and owned:
            try:
                acceptance.cleanup()
            except (AcceptanceFailure, RuntimeFailure, OSError, ValueError, subprocess.TimeoutExpired):
                acceptance.record("FAIL: owned-teardown")
                result = 1
        if locked:
            acceptance.record("Result: PASS" if result == 0 else "Result: FAIL")
            if os.environ.get("GITHUB_STEP_SUMMARY"):
                with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
                    output.write("\n".join(acceptance.summary) + "\n")
        os.close(descriptor)
    return result


if __name__ == "__main__":
    sys.exit(main())
