#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Subprocess tests for the managed tooling UTF-8 guard."""
import os
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]


class EntryGuardTests(unittest.TestCase):
    def run_check(self, *options):
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(('PYTHON', 'OXPINYIN_'))}
        return subprocess.run([sys.executable, *options, '-m', 'tools.dev.check', '--help'],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=120)

    def test_utf8_mode_reaches_the_operation(self):
        result = self.run_check('-X', 'utf8')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Run native setup checks', result.stdout)
        self.assertEqual(result.stderr, '')

    def test_disabled_mode_refuses_the_operation(self):
        result = self.run_check('-X', 'utf8=0')
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, '')
        self.assertEqual(result.stderr.splitlines(), [
            'repository Python: UTF-8 mode is 0; run with -X utf8 (or PYTHONUTF8=1)',
        ])

    def test_missing_mode_names_the_required_interpreter(self):
        result = subprocess.run(
            [sys.executable, '-X', 'utf8', '-c',
             'import sys; from types import SimpleNamespace; '
             'from tools.dev.entry import require_utf8_mode; '
             'sys.flags = SimpleNamespace(); require_utf8_mode()'],
            cwd=ROOT, capture_output=True, text=True, timeout=120)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, '')
        self.assertEqual(result.stderr.splitlines(), [
            'repository Python: sys.flags.utf8_mode is missing; '
            'run the interpreter in .python-version with -X utf8',
        ])


if __name__ == '__main__':
    unittest.main()
