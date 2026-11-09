#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Unit tests for tools.dev CLI diagnostics and exit statuses."""
import contextlib
import io
import json
import os
from pathlib import Path
import re
import sys
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import cli, slot, test_support

IMPLEMENTATION = 'legacy:tools/oracle/build-oracle.sh'
COMMAND = ['bash', '{root}/tools/oracle/build-oracle.sh']


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

    def test_real_justfile_lists_its_public_recipes(self):
        result = self.invoke('--list')
        self.assertEqual(result.returncode, 0, result.stderr)
        if sys.platform != 'win32':
            self.assertIn('test-tools', result.stdout)
        self.assertIn('audit-glib-call-sites', result.stdout)
        self.assertNotIn('_test-argv', result.stdout)

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

    def test_bootstrapping_uv_recipes_depend_on_python(self):
        result = self.invoke('--dump', '--dump-format', 'json')
        self.assertEqual(result.returncode, 0, result.stderr)
        recipes = json.loads(result.stdout)['recipes']
        users = {name for name, recipe in recipes.items()
                 if any('uv run' in fragment for line in recipe['body'] for fragment in line)}
        self.assertIn('check', users)
        for name in sorted(users - {'doctor', 'rust-tables'}):
            with self.subTest(recipe=name):
                self.assertIn('python', [dependency['recipe'] for dependency in recipes[name]['dependencies']])
        for name in ('doctor', 'rust-tables'):
            self.assertEqual(recipes[name]['dependencies'], [])

    def test_platform_variants_keep_bootstrap_or_offline_contract(self):
        header = re.compile(r'^(@?[A-Za-z_][\w-]*)(?: [^:]*)?:(.*)$')
        blocks = []
        for line in (ROOT / 'justfile').read_text().splitlines():
            match = header.match(line)
            if match and not line.startswith(('set ', 'default')):
                blocks.append([match.group(1), match.group(2), []])
            elif blocks and line.startswith(' '):
                blocks[-1][2].append(line)
        users = [(name, dependencies) for name, dependencies, body in blocks
                 if name not in ('doctor', 'rust-tables') and any('uv run' in line for line in body)]
        self.assertGreaterEqual(len(users), 20)
        for name, dependencies, body in blocks:
            if name == 'rust-tables':
                self.assertFalse(dependencies.strip())
                self.assertTrue(any('uv run --offline --no-python-downloads' in line for line in body))
        for name, dependencies in users:
            with self.subTest(recipe=name):
                self.assertIn('python', dependencies.split())

    def test_table_recipe_disables_downloads_and_does_not_bootstrap_python(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'uv.log'
            write_fake_uv(Path(directory), log)
            env = {**os.environ, 'PATH': f'{directory}{os.pathsep}{os.environ["PATH"]}',
                   FAKE_UV_PYTHON: sys.executable}
            result = subprocess.run(
                ['just', '--justfile', str(ROOT / 'justfile'), 'rust-tables', '--check'],
                cwd=ROOT, capture_output=True, text=True, timeout=30, env=env)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(log.read_text().splitlines(), [
                'run --offline --no-python-downloads --no-project --managed-python '
                'python -X lazy_imports=normal -m tools.dev rust-tables --check'])

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


class CliTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(mock.patch.dict(os.environ, {'PATH': os.environ['PATH']}, clear=True))
        self.launch = self.enterContext(mock.patch.object(
            slot.subprocess, 'Popen', side_effect=AssertionError('CLI unit test launched a process')))

    def test_tools_dispatches_to_shared_harness_runner(self):
        with mock.patch.object(cli.test_runner, 'run_tools', return_value=1) as run:
            self.assertEqual(cli.main(['test-tools']), 1)
        run.assert_called_once_with([], None, None)

    def test_tools_forwards_selection_options(self):
        cases = ((['tools/dev/test_runner.test.py', '-k', 'HarnessTests'],
                  (['tools/dev/test_runner.test.py'], 'HarnessTests', None)),
                 (['--changed'], ([], None, 'HEAD')),
                 (['--changed', 'origin/main'], ([], None, 'origin/main')))
        for argv, expected in cases:
            with self.subTest(argv=argv), \
                    mock.patch.object(cli.test_runner, 'run_tools', return_value=7) as run:
                self.assertEqual(cli.main(['test-tools', *argv]), 7)
                run.assert_called_once_with(*expected)

if __name__ == '__main__':
    unittest.main()
