#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Subprocess tests for the `python -m tools.dev` import-mode guard."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

# Runs tools.dev as `-m` would and reports, on SystemExit, whether the CLI
# module was loaded. argv[1] is 'keep', 'drop' (remove sys.get_lazy_imports)
# or 'stub=<mode>' (report <mode>, which this interpreter cannot select).
WRAPPER = '''\
import json, runpy, sys
action = sys.argv.pop(1)
if action == 'drop':
    del sys.get_lazy_imports
elif action.startswith('stub='):
    sys.get_lazy_imports = lambda: action.removeprefix('stub=')
status = 0
try:
    runpy.run_module('tools.dev', run_name='__main__', alter_sys=True)
except SystemExit as exc:
    status = exc.code
print(json.dumps({'status': status, 'cli_loaded': 'tools.dev.cli' in sys.modules}), file=sys.stderr)
raise SystemExit(status)
'''

# CPython 3.15.0 accepts only 'normal' and 'all' at startup; 'none' stops the
# interpreter before tools.dev runs, so the guard's 'none' case is stubbed.
STARTUP_INVALID = 'none'


class EntryGuardTests(unittest.TestCase):
    def setUp(self):
        temp = self.enterContext(tempfile.TemporaryDirectory())
        self.lock_dir = Path(temp, 'locks')
        self.lock_dir.mkdir()
        self.marker = Path(temp, 'marker')
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(('PYTHON', 'OXPINYIN_'))}
        self.env.update(OXPINYIN_LOCK_DIR=str(self.lock_dir), OXPINYIN_HEAVY_SLOTS='1')

    def run_wrapper(self, args, *, flag=None, env_mode=None, action='keep'):
        env = dict(self.env)
        if env_mode is not None:
            env['PYTHON_LAZY_IMPORTS'] = env_mode
        options = [] if flag is None else ['-X', f'lazy_imports={flag}']
        command = [sys.executable, *options, '-c', WRAPPER, action, *args]
        return subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, timeout=120)

    def run_reported(self, args, **kwargs):
        result = self.run_wrapper(args, **kwargs)
        *diagnostic, report = result.stderr.splitlines()
        return result, diagnostic, json.loads(report)

    def rejected_runs(self, args):
        yield '-X lazy_imports=all', self.run_reported(args, flag='all'), 'all'
        yield 'PYTHON_LAZY_IMPORTS=all', self.run_reported(args, env_mode='all'), 'all'
        yield 'stubbed none', self.run_reported(args, flag='normal', action='stub=none'), 'none'

    def startup_refusals(self, args):
        yield f'-X lazy_imports={STARTUP_INVALID}', self.run_wrapper(args, flag=STARTUP_INVALID)
        yield f'PYTHON_LAZY_IMPORTS={STARTUP_INVALID}', self.run_wrapper(args, env_mode=STARTUP_INVALID)

    def assert_startup_refused(self, result):
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, '')
        self.assertIn('Fatal Python error: config_init_lazy_imports', result.stderr)
        self.assertNotIn('cli_loaded', result.stderr)
        self.assertEqual(list(self.lock_dir.iterdir()), [])

    def assert_rejected(self, outcome, mode):
        result, diagnostic, report = outcome
        self.assertEqual(result.returncode, 2)
        self.assertEqual(report, {'status': 2, 'cli_loaded': False})
        self.assertEqual(result.stdout, '')
        self.assertEqual(len(diagnostic), 1)
        self.assertIn(repr(mode), diagnostic[0])
        self.assertIn('-X lazy_imports=normal', diagnostic[0])
        self.assertIn('PYTHON_LAZY_IMPORTS=normal', diagnostic[0])
        self.assertEqual(list(self.lock_dir.iterdir()), [])

    def test_normal_mode_reaches_the_cli_listing(self):
        expected = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c',
                                   'from tools.dev import cli; raise SystemExit(cli.main(["list"]))'],
                                  cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=120)
        self.assertEqual(expected.returncode, 0, expected.stderr)
        self.assertNotEqual(expected.stdout, '')
        for label, kwargs in (('-X', {'flag': 'normal'}), ('environment', {'env_mode': 'normal'})):
            with self.subTest(label):
                result, diagnostic, report = self.run_reported(['list'], **kwargs)
                self.assertEqual(result.returncode, 0, diagnostic)
                self.assertEqual(report, {'status': 0, 'cli_loaded': True})
                self.assertEqual(result.stdout, expected.stdout)
                self.assertEqual(diagnostic, [])

    def test_other_modes_exit_2_before_the_cli_loads(self):
        for label, outcome, mode in self.rejected_runs(['list']):
            with self.subTest(label):
                self.assert_rejected(outcome, mode)
        for label, result in self.startup_refusals(['list']):
            with self.subTest(label):
                self.assert_startup_refused(result)

    def test_rejected_slot_run_starts_no_child_and_takes_no_slot(self):
        child = [sys.executable, '-c', f'open({str(self.marker)!r}, "w").close()']
        for label, outcome, mode in self.rejected_runs(['slot', 'run', '--', *child]):
            with self.subTest(label):
                self.assert_rejected(outcome, mode)
                self.assertFalse(self.marker.exists())
        for label, result in self.startup_refusals(['slot', 'run', '--', *child]):
            with self.subTest(label):
                self.assert_startup_refused(result)
                self.assertFalse(self.marker.exists())
        # Control: the same command in normal mode does start the child.
        result, diagnostic, report = self.run_reported(['slot', 'run', '--', *child], flag='normal')
        self.assertEqual(result.returncode, 0, diagnostic)
        self.assertEqual(report, {'status': 0, 'cli_loaded': True})
        self.assertTrue(self.marker.exists())

    def test_missing_get_lazy_imports_names_the_required_interpreter(self):
        result, diagnostic, report = self.run_reported(['list'], flag='normal', action='drop')
        self.assertEqual(result.returncode, 2)
        self.assertEqual(report, {'status': 2, 'cli_loaded': False})
        self.assertEqual(result.stdout, '')
        self.assertEqual(len(diagnostic), 1)
        self.assertIn('sys.get_lazy_imports is missing', diagnostic[0])
        self.assertIn('cpython@3.15.0+gil', diagnostic[0])
        self.assertEqual(list(self.lock_dir.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
