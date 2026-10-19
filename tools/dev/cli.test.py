#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Unit tests for tools.dev CLI diagnostics and exit statuses."""
import contextlib
import io
from pathlib import Path
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import cli, host, slot


class CliTests(unittest.TestCase):
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


if __name__ == '__main__':
    unittest.main()
