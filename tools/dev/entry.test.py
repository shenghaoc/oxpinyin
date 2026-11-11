#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Subprocess tests for the managed tooling import-mode guard."""
import os
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]

# argv[1] selects the interpreter API presented to the shared entry check.
WRAPPER = '''\
import runpy, sys
action = sys.argv.pop(1)
if action == 'drop':
    del sys.get_lazy_imports
elif action.startswith('stub='):
    sys.get_lazy_imports = lambda: action.removeprefix('stub=')
runpy.run_module('tools.dev.check', run_name='__main__', alter_sys=True)
'''

# CPython 3.15.0 accepts only 'normal' and 'all' at startup; 'none' stops the
# interpreter before tools.dev runs, so the guard's 'none' case is stubbed.
STARTUP_INVALID = 'none'


class EntryGuardTests(unittest.TestCase):
    def setUp(self):
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(('PYTHON', 'OXPINYIN_'))}

    def run_wrapper(self, args, *, flag=None, env_mode=None, action='keep'):
        env = dict(self.env)
        if env_mode is not None:
            env['PYTHON_LAZY_IMPORTS'] = env_mode
        options = [] if flag is None else ['-X', f'lazy_imports={flag}']
        command = [sys.executable, *options, '-c', WRAPPER, action, *args]
        return subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, timeout=120)

    def rejected_runs(self, args):
        yield '-X lazy_imports=all', self.run_wrapper(args, flag='all'), 'all'
        yield 'PYTHON_LAZY_IMPORTS=all', self.run_wrapper(args, env_mode='all'), 'all'
        yield 'stubbed none', self.run_wrapper(args, flag='normal', action='stub=none'), 'none'

    def startup_refusals(self, args):
        yield f'-X lazy_imports={STARTUP_INVALID}', self.run_wrapper(args, flag=STARTUP_INVALID)
        yield f'PYTHON_LAZY_IMPORTS={STARTUP_INVALID}', self.run_wrapper(args, env_mode=STARTUP_INVALID)

    def assert_startup_refused(self, result):
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, '')
        self.assertIn('Fatal Python error: config_init_lazy_imports', result.stderr)

    def assert_rejected(self, result, mode):
        diagnostic = result.stderr.splitlines()
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, '')
        self.assertEqual(len(diagnostic), 1)
        self.assertIn(repr(mode), diagnostic[0])
        self.assertIn('-X lazy_imports=normal', diagnostic[0])
        self.assertIn('PYTHON_LAZY_IMPORTS=normal', diagnostic[0])

    def test_normal_mode_reaches_the_operation(self):
        for kwargs in ({'flag': 'normal'}, {'env_mode': 'normal'}):
            with self.subTest(**kwargs):
                result = self.run_wrapper(['--help'], **kwargs)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('Run native setup checks', result.stdout)
                self.assertEqual(result.stderr, '')

    def test_other_modes_exit_2_before_running_the_operation(self):
        for label, outcome, mode in self.rejected_runs(['--help']):
            with self.subTest(label):
                self.assert_rejected(outcome, mode)
        for label, result in self.startup_refusals(['--help']):
            with self.subTest(label):
                self.assert_startup_refused(result)

    def test_missing_get_lazy_imports_names_the_required_interpreter(self):
        result = self.run_wrapper(['--help'], flag='normal', action='drop')
        diagnostic = result.stderr.splitlines()
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, '')
        self.assertEqual(len(diagnostic), 1)
        self.assertIn('sys.get_lazy_imports is missing', diagnostic[0])
        self.assertIn('.python-version', diagnostic[0])


if __name__ == '__main__':
    unittest.main()
