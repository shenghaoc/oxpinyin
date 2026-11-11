#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Lint: the Python version request is written once, in .python-version (#1246).

Every other file lets that file decide, or reads or passes its contents on. A literal request
anywhere else fails this test, including in a pull request that rebases onto this check. The
dated records under docs/findings/ keep their wording. The fixtures below are
assembled from pieces so this file states no request itself.
"""
from pathlib import Path
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import test_support
ALLOWED = ('.python-version', 'docs/findings/')
# A request in uv's forms: the implementation prefix with a version, or an explicit version given
# to `uv python install|find|pin` or `--python`. Managed-install directory names
# (cpython-3.x-linux...), prose such as "CPython 3.15" and version tuples are not requests.
REQUEST = re.compile(r'cpython@\d|uv python (?:install|find|pin)[^\n]*? \d\.\d|--python[ =]["\']?\d\.\d')


def literals(text):
    """Return the lines of TEXT that write a Python version request."""
    return [line for line in text.splitlines() if REQUEST.search(line)]


def tracked_text_files():
    """Yield (path, text) for the tracked text files outside the allowlist."""
    listing = subprocess.run(['git', 'ls-files', '-z'], cwd=ROOT, capture_output=True, check=True).stdout
    for name in listing.decode().split('\0'):
        if not name or name.startswith(ALLOWED[1:]) or name == ALLOWED[0]:
            continue
        path = ROOT / name
        if not path.is_file() or path.is_symlink():
            continue
        try:
            yield name, path.read_text()
        except UnicodeDecodeError:
            continue


class RequestLiteralTests(unittest.TestCase):
    def test_no_file_outside_the_allowlist_writes_a_request(self):
        found = {name: literals(text) for name, text in tracked_text_files() if literals(text)}
        self.assertEqual(found, {}, 'a Python version request is written outside .python-version; '
                         'let the file decide, or read it (docs/runbooks/setup.md)')

    def test_the_request_forms_are_caught(self):
        series = '3.' + '15'
        for line in (
                'uv run --python cpython' + '@' + series + ' python',
                'x = "cpython' + '@' + series + '.0"',
                'uv python install ' + series,
                'uv python find --managed-python ' + series,
                'uv run --python ' + series + ' python',
                'uv run --python=' + series + ' python',
                "--python '" + series + "'",
        ):
            with self.subTest(line=line):
                self.assertTrue(literals(line), line)

    def test_look_alikes_are_not_flagged(self):
        series = '3.' + '15'
        for line in (
                '/opt/uv/python/cpython-' + series + '.0-linux-x86_64-gnu/bin/python' + series,
                'managed CPython ' + series + ' series',
                'assert (3, 15, 0) <= sys.version_info[:3] < (3, 16, 0)',
                'PYTHON_MINIMUM = "' + series + '.0"',
                'uv python install',
                'uv python find --managed-python',
                'uv run --no-project --managed-python python -X lazy_imports=normal',
                'cpython@ is a request prefix',
                'python' + series,
        ):
            with self.subTest(line=line):
                self.assertFalse(literals(line), line)

    def test_the_allowlist_is_exactly_the_file_and_the_dated_records(self):
        self.assertEqual(ALLOWED, ('.python-version', 'docs/findings/'))
        self.assertRegex((ROOT / '.python-version').read_text().strip(), r'^\d+\.\d+$')

    def test_the_scan_sees_the_files_it_is_meant_to(self):
        names = {name for name, _ in tracked_text_files()}
        for expected in ('justfile', 'tools/cloud/setup.sh', 'docs/runbooks/setup.md',
                         '.github/workflows/ci.yml', 'tools/oracle/Dockerfile.oracle', 'AGENTS.md'):
            self.assertIn(expected, names)
        for excluded in ('.python-version',):
            self.assertNotIn(excluded, names)
        self.assertFalse([name for name in names if name.startswith('docs/findings/')])


# Stands in for uv: it records its working directory and arguments.
RECORD_UV = ('import os, sys\n'
             'with open(os.environ["RECORD"], "w") as stream:\n'
             '    stream.write(os.getcwd() + "\\n" + "\\n".join(sys.argv[1:]) + "\\n")\n')


class CheckoutInterpreterTests(unittest.TestCase):
    """An entry point run from outside the checkout selects the checkout's interpreter request."""

    def test_run_all_passes_the_checkouts_request_not_the_callers(self):
        checkout_request = (ROOT / '.python-version').read_text().strip()
        other = 'cpython@' + '9.99'
        with tempfile.TemporaryDirectory() as scratch:
            outer, stubs = Path(scratch, 'outer'), Path(scratch, 'bin')
            outer.mkdir()
            stubs.mkdir()
            (outer / '.python-version').write_text(other + '\n')
            record = Path(scratch, 'argv')
            test_support.python_tool(stubs, 'uv', RECORD_UV)
            env = {key: value for key, value in os.environ.items()
                   if key not in ('UV_PYTHON', 'PYTHONHOME', 'PYTHONPATH')}
            env.update(PATH=f'{stubs}{os.pathsep}{os.environ["PATH"]}', RECORD=str(record))
            result = subprocess.run([shutil.which('uv', path=env['PATH']), 'run', '--no-project', '--managed-python', '--python', checkout_request, 'python', '-X', 'lazy_imports=normal', '-c', 'import runpy, sys; sys.path.insert(0, sys.argv.pop(1)); runpy.run_module("tools.dev.provision", run_name="__main__", alter_sys=True)', str(ROOT), '--python-only'],
                                    cwd=outer, env=env, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = record.read_text().splitlines()
        self.assertEqual(Path(lines[0]).name, 'outer', 'the caller\'s working directory is kept')
        argv = lines[1:]
        self.assertEqual(argv[argv.index('--python') + 1], checkout_request)
        self.assertNotIn(other, argv)
        self.assertEqual(argv[-1:], ['--python-only'], 'the caller\'s arguments are unchanged')


if __name__ == '__main__':
    unittest.main()
