import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from live import live_api_port, live_commands, main, run


class LiveRuntimeTests(unittest.TestCase):
    def test_live_commands_keep_private_config_outside_checkout_and_bind_dashboard_to_loopback(self):
        checkout = Path('/fictional/checkout')
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / 'private.json'
            commands = live_commands(checkout, config)
        self.assertEqual(commands[0][-2:], ['live', str(config)])
        self.assertEqual(commands[1][-4:], ['--host', '127.0.0.1', '--port', '5174'])
        for unsafe in ('relative.json', '/fictional/checkout/private.json'):
            with self.assertRaises(ValueError):
                live_commands(checkout, unsafe)

    def test_web_port_override_is_forwarded_and_invalid_ports_are_refused(self):
        commands = live_commands(Path('/fictional/checkout'), '/private/live.json', 5175)
        self.assertEqual(commands[1][-4:], ['--host', '127.0.0.1', '--port', '5175'])
        for port in (0, -1, 65536, 5174.5, '5174', True, None):
            with self.subTest(port=port), self.assertRaises(ValueError):
                live_commands(Path('/fictional/checkout'), '/private/live.json', port)

    def test_api_port_defaults_and_accepts_nonzero_u16_overrides(self):
        for configuration, expected in (({}, 8788), ({'api_port': 1}, 1),
                                        ({'api_port': 8789}, 8789), ({'api_port': 65535}, 65535)):
            with self.subTest(configuration=configuration), patch('live.Path.read_text', return_value=json.dumps(configuration)):
                self.assertEqual(live_api_port('/private/live.json'), expected)

    def test_invalid_api_ports_and_malformed_configs_refuse_startup(self):
        configurations = [{'api_port': port} for port in (0, -1, 65536, 8788.5, '8788', True, None)]
        configurations.extend(([], None, 'invalid'))
        for contents in [json.dumps(configuration) for configuration in configurations] + ['{']:
            with self.subTest(contents=contents), patch('live.Path.read_text', return_value=contents), patch('live.subprocess.Popen') as start:
                with self.assertRaises(ValueError):
                    run(Path('/fictional/checkout'), '/private/live.json')
                start.assert_not_called()

    def test_npm_child_receives_config_api_port_and_inherits_the_environment(self):
        for configuration, expected in (({}, '8788'), ({'api_port': 8789}, '8789')):
            with self.subTest(configuration=configuration), patch('live.Path.read_text', return_value=json.dumps(configuration)), patch.dict('live.os.environ', {'CHARGESHARE_API_PORT': '9999', 'PATH': '/fictional/bin'}, clear=True), patch('live.subprocess.Popen') as start:
                start.return_value.wait.return_value = 0
                start.return_value.poll.return_value = 0
                self.assertEqual(run(Path('/fictional/checkout'), '/private/live.json', 5175), 0)
                cargo_call, npm_call = start.call_args_list
                self.assertIsNone(cargo_call.kwargs['env'])
                self.assertEqual(npm_call.kwargs['env']['CHARGESHARE_API_PORT'], expected)
                self.assertEqual(npm_call.kwargs['env']['PATH'], '/fictional/bin')
                self.assertEqual(npm_call.args[0][-2:], ['--port', '5175'])

    def test_cli_defaults_and_web_port_override_reach_the_launcher(self):
        for arguments, expected in (([], 5174), (['--web-port', '5175'], 5175)):
            with self.subTest(arguments=arguments), patch('live.sys.argv', ['live.py', '--config', '/private/live.json'] + arguments), patch('live.run', return_value=0) as launch:
                self.assertEqual(main(), 0)
                self.assertEqual(launch.call_args.args[1:], ('/private/live.json', expected))

    def test_partial_startup_failure_stops_the_child_process_group(self):
        class Child:
            pid = 98765

            def poll(self):
                return None

            def wait(self, timeout):
                return 0

        with tempfile.TemporaryDirectory() as directory, patch('live.Path.read_text', return_value='{}'), patch('live.subprocess.Popen', side_effect=[Child(), OSError()]), patch('live.os.killpg') as kill:
            with self.assertRaises(OSError):
                run(Path('/fictional/checkout'), Path(directory) / 'private.json')
        kill.assert_called_once()


if __name__ == '__main__':
    unittest.main()
