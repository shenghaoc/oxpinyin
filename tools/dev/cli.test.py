#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Unit tests for tools.dev CLI diagnostics and exit statuses."""
import contextlib
import io
import os
from pathlib import Path
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import cli, host, slot


class CliTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(mock.patch.dict(os.environ, {'PATH': os.environ['PATH']}, clear=True))
        self.launch = self.enterContext(mock.patch.object(
            slot.subprocess, 'Popen', side_effect=AssertionError('CLI unit test launched a process')))

    def test_expected_slot_failures_print_one_line_without_a_traceback(self):
        failures = (
            (host.ResourceBlocked('resource-blocked: available memory unknown'), 1),
            (slot.Cancelled('cancelled; child process groups terminated and reaped'), 130),
            (ValueError('OXPINYIN_LOCK_DIR must be an absolute path'), 1),
            (ValueError('Invalid OXPINYIN_HEAVY_SLOT_HELD'), 1),
            (OSError('child launch\nfailed'), 1),
        )
        for error, expected in failures:
            with self.subTest(error=error), mock.patch.object(
                    slot, 'run_command', side_effect=error), contextlib.redirect_stderr(io.StringIO()) as err:
                self.assertEqual(cli.main(['slot', 'run', '--', 'child']), expected)
            self.assertEqual(err.getvalue().splitlines(),
                             ['slot run: ' + ' '.join(str(error).splitlines())])

    def test_negative_child_status_uses_the_shell_signal_convention(self):
        for signum in (15, 9):
            with self.subTest(signal=signum), mock.patch.object(
                    slot, 'run_command', return_value=-signum):
                self.assertEqual(cli.main(['slot', 'run', '--', 'child']), 128 + signum)

    def test_nonnegative_child_status_and_arguments_are_preserved(self):
        for status in (0, 7):
            with self.subTest(status=status), mock.patch.object(
                    slot, 'run_command', return_value=status) as run:
                self.assertEqual(cli.main(['slot', 'run', '--', 'child', '--flag']), status)
                run.assert_called_once_with(['child', '--flag'])

    def test_test_commands_forward_raw_arguments_before_argparse(self):
        selections = (['-p', 'oxpinyin-core'], ['--workspace'],
                      ['--test-threads=2'], ['-P', 'ci'], ['--', '-p', 'oxpinyin-core'])
        for command, target in (('test', 'run'), ('test-doc', 'run_doc')):
            for selection in selections:
                with self.subTest(command=command, selection=selection), \
                        mock.patch.object(cli.test_runner, target, return_value=7) as run:
                    self.assertEqual(cli.main([command, *selection]), 7)
                    run.assert_called_once_with(selection)
        self.launch.assert_not_called()

    def test_test_options_reach_the_slot_without_a_separator(self):
        for selection in (['-p', 'oxpinyin-core'], ['--workspace'],
                          ['--test-threads=2'], ['-P', 'ci']):
            with self.subTest(selection=selection), \
                    mock.patch.object(slot, 'run_command', return_value=0) as run:
                self.assertEqual(cli.main(['test', *selection]), 0)
                self.assertEqual(run.call_args.args[0][-len(selection):], selection)
        self.launch.assert_not_called()

    def test_test_command_help_keeps_the_parser_entries(self):
        for command in ('test', 'test-doc'):
            for flag in ('-h', '--help'):
                with self.subTest(command=command, flag=flag), \
                        mock.patch.object(cli.test_runner, 'run') as run, \
                        mock.patch.object(cli.test_runner, 'run_doc') as run_doc, \
                        contextlib.redirect_stdout(io.StringIO()) as output:
                    with self.assertRaises(SystemExit) as exit_status:
                        cli.main([command, flag])
                    self.assertEqual(exit_status.exception.code, 0)
                    self.assertIn('usage:', output.getvalue())
                    run.assert_not_called()
                    run_doc.assert_not_called()
        self.launch.assert_not_called()

    def test_main_reads_raw_arguments_from_sys_argv(self):
        with mock.patch.object(sys, 'argv', ['tools.dev', 'test', '--workspace']), \
                mock.patch.object(cli.test_runner, 'run', return_value=7) as run:
            self.assertEqual(cli.main(), 7)
            run.assert_called_once_with(['--workspace'])
        self.launch.assert_not_called()


if __name__ == '__main__':
    unittest.main()
