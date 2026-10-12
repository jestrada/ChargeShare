import argparse
import contextlib
import fcntl
import os
import signal
import stat
import subprocess
import sys
import time
from pathlib import Path

from runtime import ProjectRuntime, RuntimeFailure, preflight, read_json_file, reset_runtime, run_command, validate_runtime_directory
from viewing import SCOPES, check, http_document, scoped_results, stop_owner


@contextlib.contextmanager
def exclusive_lifecycle(runtime):
    private = runtime.checkout / ".cache" / "tilt"
    private.mkdir(mode=0o700, parents=True, exist_ok=True)
    if private.is_symlink() or private.resolve() != private:
        raise RuntimeFailure("Refusing unsafe lifecycle state.")
    descriptor = os.open(private / "poc.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or metadata.st_uid != os.getuid():
            raise RuntimeFailure("Refusing unsafe lifecycle lock.")
        os.fchmod(descriptor, 0o600)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeFailure("Another persisted lifecycle command is active.") from None
        yield
    finally:
        os.close(descriptor)


def lifecycle_deadline(signum, frame):
    signal.signal(signum, signal.SIG_DFL)
    raise KeyboardInterrupt


class PersistedLifecycle:
    def __init__(self, runtime):
        self.runtime = runtime
        self.environment = runtime.environment()
        self.environment["CHARGESHARE_PERSISTED_RUNTIME"] = "1"

    def down(self):
        validate_runtime_directory(self.runtime)
        stop_owner(self.runtime)
        result = subprocess.run(["tilt", "down"], cwd=self.runtime.checkout, env=self.environment, timeout=130)
        if result.returncode != 0:
            raise RuntimeFailure("Owned shutdown failed; retained state was preserved.")
        print("PASS: owned services stopped; source epoch and SQLite preserved")

    def up(self):
        preflight(self.runtime)
        private = self.runtime.checkout / ".cache" / "tilt"
        private.mkdir(mode=0o700, parents=True, exist_ok=True)
        with (private / "poc-local.log").open("a", encoding="utf-8") as output:
            child = subprocess.Popen(["tilt", "up", "--stream"], cwd=self.runtime.checkout, env=self.environment, stdout=output, stderr=output, start_new_session=True)
        ready = False
        try:
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                if child.poll() is not None:
                    raise RuntimeFailure("Owned startup exited before Tilt was reachable.")
                try:
                    run_command(["tilt", "get", "uiresource", "(Tiltfile)", "-o", "name"], self.environment)
                    break
                except RuntimeFailure:
                    time.sleep(1)
            else:
                raise RuntimeFailure("Owned Tilt startup exceeded 90 seconds.")
            resources = ["kafka", "broker-ready", "receiver", "receiver-ready", "rust-build", "frontend-install", "fixture-api", "fixture-dashboard", "persisted-runtime", "persisted-dashboard"]
            result = subprocess.run(["python3", "scripts/dev/readiness.py", "--timeout", "900", *resources], cwd=self.runtime.checkout, env=self.environment, timeout=910)
            if result.returncode != 0:
                raise RuntimeFailure("Owned all-service readiness failed.")
            http_document("/api/poc/ready")
            ready = True
            print("PASS: receiver, private Kafka, owning runtime, persisted 5174/8788 and fixture 5173/8787 ready; no fixtures sent")
        finally:
            if not ready:
                try:
                    self.down()
                finally:
                    if child.poll() is None:
                        child.terminate()
                        child.wait(timeout=35)

    def restart(self):
        validate_runtime_directory(self.runtime)
        before = read_json_file(self.runtime.directory / "source-epoch.json", "Source epoch")
        try:
            results_before = [scoped_results(owner, vehicle) for owner, vehicle, energy in SCOPES]
        except RuntimeFailure:
            results_before = None
        self.down()
        self.up()
        results_after = [scoped_results(owner, vehicle) for owner, vehicle, energy in SCOPES]
        if results_before is not None and results_before != results_after:
            raise RuntimeFailure("Ordinary restart changed committed scoped results or semantic revisions.")
        after = read_json_file(self.runtime.directory / "source-epoch.json", "Source epoch")
        if before != after:
            raise RuntimeFailure("Ordinary restart changed the source epoch.")
        print("PASS: ordinary restart preserved the source epoch and retained evidence")


def main(arguments=None):
    parser = argparse.ArgumentParser(description="Bounded owned all-service synthetic POC lifecycle.")
    parser.add_argument("action", choices=("up", "check", "restart", "down", "reset"))
    parser.add_argument("--confirm-synthetic-reset", action="store_true")
    options = parser.parse_args(arguments)
    if options.confirm_synthetic_reset and options.action != "reset":
        parser.error("Confirmation applies only to reset")
    runtime = ProjectRuntime.at(Path(__file__).resolve().parents[2])
    try:
        signal.signal(signal.SIGTERM, lifecycle_deadline)
        signal.signal(signal.SIGINT, lifecycle_deadline)
        with exclusive_lifecycle(runtime):
            lifecycle = PersistedLifecycle(runtime)
            if options.action == "check":
                check(runtime)
            elif options.action == "reset":
                reset_runtime(runtime, options.confirm_synthetic_reset)
                print("PASS: stopped owned synthetic state reset; next up creates a fresh epoch")
            else:
                getattr(lifecycle, options.action)()
    except KeyboardInterrupt:
        print("Persisted lifecycle: bounded operation interrupted.", file=sys.stderr)
        return 130
    except (RuntimeFailure, OSError, ValueError, KeyError, subprocess.TimeoutExpired) as failure:
        print("Persisted lifecycle: " + (str(failure) if isinstance(failure, RuntimeFailure) else "bounded prerequisite or lifecycle failure"), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
