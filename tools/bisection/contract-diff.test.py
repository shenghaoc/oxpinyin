#!/usr/bin/env python3
"""Regressions for the contract differential's fatal-log predicate."""

import copy
import json
import runpy
import signal
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

contract = runpy.run_path(str(Path(__file__).with_name('contract-diff.py')))


class FatalLogTests(unittest.TestCase):
    def setUp(self):
        self.spec = contract['CASES']['save-fatal-valid-to-invalid']
        self.pin = {
            'exit': -signal.SIGTRAP,
            'result': None,
            'stderr': 'open SCRATCH/user-X/user.conf failed.\n'
            '** (process:123): CRITICAL **: 16:44:30.526: assertion failed\n',
            'userfiles': {'gb_char.dbin.tmp': [26, 'digest']},
        }
        self.subject = copy.deepcopy(self.pin)
        self.subject['stderr'] = (
            self.pin['stderr'].replace('123', '456').replace('30.526', '31.045')
        )

    def matches(self):
        return contract['matches'](self.spec, self.pin, self.subject)

    def test_same_glib_fatal_signal(self):
        for sig in (signal.SIGABRT, signal.SIGTRAP):
            with self.subTest(signal=sig):
                self.pin['exit'] = self.subject['exit'] = -sig
                self.assertTrue(self.matches())

    def test_different_fatal_signals(self):
        self.subject['exit'] = -signal.SIGABRT
        self.assertFalse(self.matches())

    def test_other_equal_exits_are_not_fatal_logs(self):
        for status in (0, 1, -signal.SIGSEGV, -signal.SIGILL, -signal.SIGTERM):
            with self.subTest(status=status):
                self.pin['exit'] = self.subject['exit'] = status
                self.assertFalse(self.matches())

    def test_message_difference_is_preserved(self):
        self.subject['stderr'] = self.subject['stderr'].replace(
            'assertion failed', 'different assertion'
        )
        self.assertFalse(self.matches())

    def test_severity_difference_is_preserved(self):
        self.subject['stderr'] = self.subject['stderr'].replace('CRITICAL', 'WARNING')
        self.assertFalse(self.matches())

    def test_partial_profile_difference_is_preserved(self):
        self.subject['userfiles']['gb_char.dbin.tmp'][1] = 'different digest'
        self.assertFalse(self.matches())


class DiagnosticPathTests(unittest.TestCase):
    def observations(self, second_message='failed'):
        values = []
        with tempfile.TemporaryDirectory(
            prefix='ox886-home-user-path-sys-root-'
        ) as root:
            for message in ('failed', second_message):
                with tempfile.TemporaryDirectory(prefix='worker-', dir=root) as scratch:
                    text = f'open {scratch}/user-generated/user.conf {message}.'
                    result = {
                        'log_messages': [text],
                        'reopen_stderr': text,
                        'payload': 'literal user-input and sys-input',
                    }
                    process = SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(result).encode(),
                        stderr=text.encode(),
                    )
                    with patch.object(
                        contract['subprocess'], 'run', return_value=process
                    ):
                        value = contract['run_worker'](
                            'pinyin', '/unused', '/unused', 'case', scratch
                        )
                    values.append(
                        contract['axis_c_normalize_diagnostics'](
                            {key: value[key] for key in ('exit', 'result', 'stderr')},
                            scratch,
                        )
                    )
        return values

    def test_generated_worker_paths_normalize_before_user_sys_names(self):
        first, second = self.observations()
        self.assertEqual(first, second)
        expected = 'open SCRATCH/user-X/user.conf failed.'
        self.assertEqual(first['stderr'], expected)
        self.assertEqual(first['result']['reopen_stderr'], expected)
        self.assertEqual(first['result']['log_messages'], [expected])
        self.assertEqual(first['result']['payload'], 'literal user-input and sys-input')

    def test_real_message_difference_survives_path_normalization(self):
        first, second = self.observations('refused')
        self.assertNotEqual(first, second)
        self.assertNotEqual(first['stderr'], second['stderr'])
        for field in ('reopen_stderr', 'log_messages'):
            self.assertNotEqual(first['result'][field], second['result'][field])


if __name__ == '__main__':
    unittest.main()
