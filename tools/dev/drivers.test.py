#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Shared launcher preserves parent and driver stream order under redirection."""
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]


class StreamOrderTests(unittest.TestCase):
    def test_redirected_parent_output_precedes_driver_output(self):
        program = '''
import os, sys
from tools.dev.cell import Settings
from tools.dev.drivers import DriverServices
print('before driver')
result = DriverServices(Settings.read(os.environ)).command(
    [sys.executable, '-c', 'print("driver output")'])
print('after driver')
raise SystemExit(result.returncode)
'''
        result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', program],
                                cwd=ROOT, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'before driver\ndriver output\nafter driver\n')


if __name__ == '__main__':
    unittest.main()
