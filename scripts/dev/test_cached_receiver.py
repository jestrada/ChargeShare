import copy
import unittest

import cached_receiver
from runtime import RuntimeFailure


class CachedReceiverTests(unittest.TestCase):
    def setUp(self):
        self.identity = {"revision": "a" * 40, "inputs_sha": "b" * 64}
        self.image_id = "sha256:" + "c" * 64
        self.image = {"Id": self.image_id, "Os": "linux", "Architecture": "amd64", "Config": {"Labels": {
            cached_receiver.BUILD_LABEL: self.identity["inputs_sha"],
            cached_receiver.REVISION_LABEL: self.identity["revision"],
        }}}

    def test_current_job_image_with_exact_inputs_is_accepted(self):
        cached_receiver.validate_loaded_image(self.identity, [self.image], self.image_id)

    def test_mismatched_image_inputs_platform_or_job_identity_are_rejected(self):
        for field, value in (("Os", "darwin"), ("Architecture", "arm64"), ("Id", "sha256:" + "d" * 64)):
            image = copy.deepcopy(self.image)
            image[field] = value
            with self.subTest(field=field), self.assertRaises(RuntimeFailure):
                cached_receiver.validate_loaded_image(self.identity, [image], self.image_id)
        for label in (cached_receiver.BUILD_LABEL, cached_receiver.REVISION_LABEL):
            image = copy.deepcopy(self.image)
            image["Config"]["Labels"][label] = "never-emit-rejected-details"
            with self.subTest(label=label), self.assertRaisesRegex(RuntimeFailure, "reviewed Linux build inputs"):
                cached_receiver.validate_loaded_image(self.identity, [image], self.image_id)

    def test_absent_job_identity_and_missing_image_are_rejected(self):
        for images, expected_id in (([], self.image_id), ([self.image], None), ([self.image], "invalid")):
            with self.subTest(images=bool(images), expected_id=bool(expected_id)), self.assertRaises(RuntimeFailure):
                cached_receiver.validate_loaded_image(self.identity, images, expected_id)

    def test_loaded_compose_changes_only_receiver_build(self):
        configuration = {"services": {"receiver": {"image": "synthetic-receiver", "build": {"context": "dev"}, "ports": ["127.0.0.1:8443:4443"]}, "kafka": {"image": "pinned-kafka"}}, "volumes": {"kafka-data": {"name": "synthetic-volume"}}}
        expected = copy.deepcopy(configuration)
        del expected["services"]["receiver"]["build"]
        self.assertEqual(cached_receiver.compose_with_loaded_receiver(configuration), expected)
        self.assertIn("build", configuration["services"]["receiver"])


if __name__ == "__main__":
    unittest.main()
