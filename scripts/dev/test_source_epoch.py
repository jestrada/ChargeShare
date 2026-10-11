import contextlib
import copy
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import runtime
import source_epoch
from test_runtime import FakeDocker


class EpochDocker(FakeDocker):
    def __init__(self, project):
        super().__init__(project)
        self.containers = []
        self.volumes = []

    def __call__(self, command, environment):
        if command[:3] == ["docker", "volume", "create"]:
            self.commands.append(command)
            labels = dict(command[index + 1].split("=", 1) for index, value in enumerate(command) if value == "--label")
            self.volumes.append({"Name": command[-1], "Labels": labels})
            return command[-1]
        return super().__call__(command, environment)


class SourceEpochTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.project = runtime.ProjectRuntime.at(self.temporary.name)
        self.docker = EpochDocker(self.project)
        self.environment = mock.patch.dict(os.environ, {"DOCKER_HOST": "", "DOCKER_CONTEXT": ""})
        self.environment.start()

    def tearDown(self):
        self.environment.stop()
        self.temporary.cleanup()

    def initialize(self):
        return source_epoch.ensure_source_epoch(self.project, self.docker)

    def test_initialization_creates_private_marker_bound_to_owned_volume_and_explicit_start(self):
        source = self.initialize()
        self.assertEqual(source["partitions"], [{"partition": 0, "initial_offset": 0}])
        self.assertEqual(self.docker.volumes[0]["Labels"][source_epoch.EPOCH_LABEL], source["epoch"])
        self.assertEqual((self.project.directory / "source-epoch.json").stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.project.environment()["CHARGESHARE_SOURCE_EPOCH"], source["epoch"])

    def test_ordinary_restart_preserves_marker_bytes_volume_and_epoch(self):
        source = self.initialize()
        before = (self.project.directory / "source-epoch.json").read_bytes()
        self.assertEqual(self.initialize(), source)
        self.assertEqual((self.project.directory / "source-epoch.json").read_bytes(), before)
        self.assertEqual(len([command for command in self.docker.commands if command[:3] == ["docker", "volume", "create"]]), 1)

    def test_explicit_stopped_owned_reset_replaces_epoch_on_initialization(self):
        first = self.initialize()
        with contextlib.redirect_stdout(io.StringIO()):
            runtime.reset_runtime(self.project, True, self.docker, lambda: None)
        self.assertNotEqual(self.initialize()["epoch"], first["epoch"])

    def test_missing_marker_or_volume_and_mismatched_epoch_preserve_sqlite_evidence(self):
        self.initialize()
        evidence = self.project.directory / "ingestion.sqlite3"
        evidence.write_bytes(b"fictional-retained-evidence")
        original = copy.deepcopy(self.docker.volumes)
        marker = self.project.directory / "source-epoch.json"
        marker_bytes = marker.read_bytes()
        for failure in ("missing-marker", "missing-volume", "changed-volume", "missing-label"):
            with self.subTest(failure=failure):
                self.docker.volumes = copy.deepcopy(original)
                marker.write_bytes(marker_bytes)
                if failure == "missing-marker":
                    marker.unlink()
                elif failure == "missing-volume":
                    self.docker.volumes.clear()
                elif failure == "changed-volume":
                    self.docker.volumes[0]["Labels"][source_epoch.EPOCH_LABEL] = "synthetic-" + "f" * 32
                else:
                    del self.docker.volumes[0]["Labels"][source_epoch.EPOCH_LABEL]
                with self.assertRaises(runtime.RuntimeFailure):
                    self.initialize()
                self.assertEqual(evidence.read_bytes(), b"fictional-retained-evidence")
                self.assertFalse(any(command[:3] == ["docker", "volume", "rm"] for command in self.docker.commands))

    def test_database_without_marker_or_volume_never_receives_a_fresh_epoch(self):
        runtime.ensure_runtime_directory(self.project)
        (self.project.directory / "ingestion.sqlite3-wal").write_bytes(b"fictional-evidence")
        with self.assertRaises(runtime.RuntimeFailure):
            self.initialize()
        self.assertEqual(self.docker.volumes, [])

    def test_nonprivate_and_unrelated_volume_state_refuses_initialization(self):
        self.initialize()
        marker = self.project.directory / "source-epoch.json"
        marker.chmod(0o644)
        with self.assertRaises(runtime.RuntimeFailure):
            self.initialize()
        marker.chmod(0o600)
        self.docker.volumes[0]["Labels"]["com.docker.compose.project"] = "unrelated"
        with self.assertRaises(runtime.RuntimeFailure):
            self.initialize()

    def test_frozen_manifest_is_installed_once_and_changes_never_overwrite_evidence(self):
        self.initialize()
        dev = self.project.checkout / "dev"
        dev.mkdir()
        manifest = dev / "ingestion-manifest.json"
        manifest.write_text(json.dumps({"manifest_version": "fictional-v1"}))
        source_epoch.ensure_manifest(self.project)
        manifest.write_text(json.dumps({"manifest_version": "fictional-v2"}))
        with self.assertRaises(runtime.RuntimeFailure):
            source_epoch.ensure_manifest(self.project)
        self.assertEqual(json.loads((self.project.directory / "manifest.json").read_bytes())["manifest_version"], "fictional-v1")


if __name__ == "__main__":
    unittest.main()
