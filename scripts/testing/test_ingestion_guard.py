import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SUITES = {"normalization": 20, "persistence": 9, "recovery": 8, "kafka_driver": 10, "cli": 10}
ROOT = Path(__file__).resolve().parents[2]
WRAPPER = ROOT / "scripts/testing/ingestion-suite.sh"


def synthetic_log(overrides=None, omitted=None):
    overrides = overrides or {}
    lines = []
    for suite, count in SUITES.items():
        if suite == omitted:
            continue
        passed, failed, ignored, filtered = overrides.get(suite, (count, 0, 0, 0))
        lines.extend([
            f"Running tests/{suite}.rs (synthetic-test)",
            f"test result: ok. {passed} passed; {failed} failed; {ignored} ignored; 0 measured; {filtered} filtered out; finished in 0.00s",
        ])
    return "\n".join(lines) + "\n"


class IngestionGuardTests(unittest.TestCase):
    def run_fixture(self, content, cargo_exit=0):
        with tempfile.TemporaryDirectory(prefix="chargeshare-synthetic-gate-") as directory:
            root = Path(directory)
            tools = root / "bin"
            tools.mkdir()
            (root / "fixture.txt").write_text(content)
            scripts = {
                "git": '#!/usr/bin/env bash\nprintf "%s\\n" "$CHARGESHARE_SYNTHETIC_TEST_ROOT"\n',
                "cargo": '#!/usr/bin/env bash\ncat "$CHARGESHARE_SYNTHETIC_TEST_ROOT/fixture.txt"\nexit "$CHARGESHARE_SYNTHETIC_CARGO_EXIT"\n',
            }
            for name, source in scripts.items():
                path = tools / name
                path.write_text(source)
                path.chmod(0o700)
            environment = os.environ.copy()
            environment.update({
                "PATH": str(tools) + os.pathsep + environment["PATH"],
                "CHARGESHARE_SYNTHETIC_TEST_ROOT": str(root),
                "CHARGESHARE_SYNTHETIC_CARGO_EXIT": str(cargo_exit),
            })
            environment.pop("GITHUB_STEP_SUMMARY", None)
            result = subprocess.run(["bash", str(WRAPPER)], env=environment, capture_output=True, text=True, timeout=10)
            summary = (root / "target/ingestion-test-results/summary.md").read_text()
            self.assertIn("Actual official receiver/Kafka integration acceptance remains unverified", summary)
            return result.returncode, summary

    def test_complete_unfiltered_fixture_passes(self):
        status, summary = self.run_fixture(synthetic_log())
        self.assertEqual(status, 0)
        self.assertIn("Result: PASS", summary)

    def test_cargo_failure_is_preserved(self):
        status, summary = self.run_fixture(synthetic_log(), 101)
        self.assertEqual(status, 101)
        self.assertIn("Result: FAIL", summary)

    def test_each_missing_or_reduced_suite_fails(self):
        for suite, count in SUITES.items():
            with self.subTest(suite=suite):
                self.assertNotEqual(self.run_fixture(synthetic_log(omitted=suite))[0], 0)
                self.assertNotEqual(self.run_fixture(synthetic_log({suite: (count - 1, 0, 0, 0)}))[0], 0)

    def test_ignored_filtered_or_failed_suites_fail(self):
        for suite, count in SUITES.items():
            for adverse in [(count, 0, 1, 0), (count, 0, 0, 1), (count, 1, 0, 0)]:
                with self.subTest(suite=suite, adverse=adverse):
                    self.assertNotEqual(self.run_fixture(synthetic_log({suite: adverse}))[0], 0)


if __name__ == "__main__":
    unittest.main()
