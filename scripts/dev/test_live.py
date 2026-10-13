import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from live import live_commands, run


class LiveRuntimeTests(unittest.TestCase):
    def test_live_commands_keep_private_config_outside_checkout_and_bind_dashboard_to_loopback(self):
        checkout = Path('/fictional/checkout')
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / 'private.json'
            commands = live_commands(checkout, config)
        self.assertEqual(commands[0][-2:], ['live', str(config)])
        self.assertEqual(commands[1][-2:], ['--host', '127.0.0.1'])
        for unsafe in ('relative.json', '/fictional/checkout/private.json'):
            with self.assertRaises(ValueError):
                live_commands(checkout, unsafe)

    def test_partial_startup_failure_stops_the_child_process_group(self):
        class Child:
            pid = 98765

            def poll(self):
                return None

            def wait(self, timeout):
                return 0

        with tempfile.TemporaryDirectory() as directory, patch('live.subprocess.Popen', side_effect=[Child(), OSError()]), patch('live.os.killpg') as kill:
            with self.assertRaises(OSError):
                run(Path('/fictional/checkout'), Path(directory) / 'private.json')
        kill.assert_called_once()


if __name__ == '__main__':
    unittest.main()
