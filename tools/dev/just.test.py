#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Smoke tests of the justfile under the real just: argument transport, exit statuses and the doctor diagnostic."""
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

# cmd.exe reads a batch file in the console code page, so the fake takes the
# interpreter path from the environment instead of holding it as text.
FAKE_UV_PYTHON = 'OXPINYIN_FAKE_UV_PYTHON'


def write_fake_uv(directory: Path, log: Path) -> None:
    """Write a fake uv that records its arguments and exits 2."""
    if sys.platform == 'win32':
        # Windows runs no shebang script: the fake is a .cmd running Python.
        (directory / 'uv.py').write_text(
            'import pathlib, sys\n'
            f'with pathlib.Path({str(log)!r}).open("a", encoding="utf-8") as stream:\n'
            '    stream.write(" ".join(sys.argv[1:]) + "\\n")\n'
            'raise SystemExit(2)\n')
        (directory / 'uv.cmd').write_text(
            f'@"%{FAKE_UV_PYTHON}%" "%~dp0uv.py" %*\n@exit /b %ERRORLEVEL%\n')
        return
    fake = directory / 'uv'
    fake.write_text(f'#!/bin/sh\necho "$@" >> {log}\nexit 2\n')
    fake.chmod(0o755)


class RealJustSmokeTests(unittest.TestCase):
    """Use CI's pinned real just and managed Python; no command mocks."""

    def invoke(self, *args):
        command = ['just', '--justfile', str(ROOT / 'justfile'), *args]
        if sys.platform == 'win32':
            # pwsh is the supported interactive caller; avoid legacy argument conversion.
            command = ['pwsh', '-NoLogo', '-NoProfile', '-Command',
                       'just ' + ' '.join("'" + value.replace("'", "''") + "'" for value in command[1:])
                       + '; exit $LASTEXITCODE']
        return subprocess.run(command,
                              cwd=ROOT, capture_output=True, text=True, timeout=30)

    def test_real_recipe_preserves_arguments_and_nonzero_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'must-not-exist'
            values = ['path with spaces', '', 'literal;$HOME', '*?[abc]',
                      'double" and single\' quotes', r'back\slash', 'line\nbreak',
                      f'$(touch "{marker}")', f'`touch "{marker}"', '-flag', '--flag', '-', '--']
            for status in (0, 1, 2, 77):
                with self.subTest(status=status):
                    result = self.invoke('_test-argv', str(status), *values)
                    self.assertEqual(result.returncode, status, result.stderr)
                    self.assertEqual(json.loads(result.stdout), values)
                    self.assertFalse(marker.exists(), 'shell expanded a literal argument')

    def test_real_recipe_with_zero_or_one_user_arguments(self):
        for values in ([], ['one argument']):
            for status in (0, 1, 2, 77):
                with self.subTest(status=status, values=values):
                    result = self.invoke('_test-argv', str(status), *values)
                    self.assertEqual(result.returncode, status, result.stderr)
                    self.assertEqual(json.loads(result.stdout), values)

    if sys.platform == 'win32':
        def test_cpp_smoke_is_refused_with_its_reason(self):
            # The gate links an ELF .so and reads it with readelf and ldd; the justfile refuses it before any tool runs.
            result = self.invoke('cpp-smoke')
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(result.stdout.strip(),
                             'cpp-smoke is unsupported on Windows: it needs readelf, ldd and an ELF .so.')

    def test_doctor_without_the_interpreter_names_just_python_and_installs_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'uv.log'
            write_fake_uv(Path(directory), log)
            env = {**os.environ, 'PATH': f'{directory}{os.pathsep}{os.environ["PATH"]}',
                   FAKE_UV_PYTHON: sys.executable}
            result = subprocess.run(['just', '--justfile', str(ROOT / 'justfile'), 'doctor'],
                                    cwd=ROOT, capture_output=True, text=True, timeout=30, env=env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('the managed interpreter in .python-version is not installed: run `just python`', result.stderr)
            self.assertEqual(log.read_text().splitlines(),
                             ['python find --managed-python'])


if __name__ == '__main__':
    unittest.main()
