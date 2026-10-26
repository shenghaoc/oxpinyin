#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Lint: the Python version request is written once, in .python-version (#1246).

Every other file lets that file decide, or reads or passes its contents on. A literal request
anywhere else fails this test, including in a pull request that rebases onto this check. The
dated records under docs/findings/ and docs/perf/ keep their wording. The fixtures below are
assembled from pieces so this file states no request itself.
"""
from pathlib import Path
import os
import re
import stat
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ALLOWED = ('.python-version', 'docs/findings/', 'docs/perf/')
# These two generators reproduce dated records: they print the exact command the tracked Axis J
# ledgers record, which names the request of the day. Changing it would make `--check` fail on
# ledgers that must not be regenerated. Only these exact files are exempt, one literal line each.
DATED_RECORD_GENERATORS = ('tools/bisection/delta_ledger.py', 'tools/bisection/render_ledger.py')
# A request in uv's forms: the implementation prefix with a version, a version followed by the
# GIL suffix, or an explicit version given to `uv python install|find|pin` or `--python`. Managed-install directory names
# (cpython-3.x-linux...), prose such as "CPython 3.15" and version tuples are not requests.
REQUEST = re.compile(r'cpython@\d|\d\.\d+(?:\.\d+)?\+gil|uv python (?:install|find|pin)[^\n]*? \d\.\d|--python[ =]["\']?\d\.\d')


def literals(text):
    """Return the lines of TEXT that write a Python version request."""
    return [line for line in text.splitlines() if REQUEST.search(line)]


def tracked_text_files():
    """Yield (path, text) for the tracked text files outside the allowlist."""
    listing = subprocess.run(['git', 'ls-files', '-z'], cwd=ROOT, capture_output=True, check=True).stdout
    for name in listing.decode().split('\0'):
        if not name or name.startswith(ALLOWED[1:]) or name == ALLOWED[0] or name in DATED_RECORD_GENERATORS:
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
                'x = "cpython' + '@' + series + '.0' + '+gil' + '"',
                'managed ' + series + '+gil build',
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
                'uv python install "$install"',
                'uv python find --managed-python',
                'uv run --no-project --managed-python python -X lazy_imports=normal',
                'cpython@ is a request prefix',
                'the +gil suffix selects the GIL build',
                'python' + series,
        ):
            with self.subTest(line=line):
                self.assertFalse(literals(line), line)

    def test_the_allowlist_is_exactly_the_file_and_the_dated_records(self):
        self.assertEqual(ALLOWED, ('.python-version', 'docs/findings/', 'docs/perf/'))
        self.assertTrue((ROOT / '.python-version').read_text().strip().endswith('+gil'))

    def test_only_the_two_ledger_generators_are_exempt_and_each_writes_one_command(self):
        self.assertEqual(DATED_RECORD_GENERATORS,
                         ('tools/bisection/delta_ledger.py', 'tools/bisection/render_ledger.py'))
        for name in DATED_RECORD_GENERATORS:
            with self.subTest(name=name):
                found = literals((ROOT / name).read_text())
                self.assertEqual(len(found), 1, found)
                self.assertTrue(found[0].startswith('uv run --no-project --managed-python --python '), found)
                self.assertNotIn(name, {n for n, _ in tracked_text_files()})

    def test_the_scan_sees_the_files_it_is_meant_to(self):
        names = {name for name, _ in tracked_text_files()}
        for expected in ('justfile', 'tools/install-python.sh', 'tools/cloud/setup.sh', 'docs/runbooks/setup.md',
                         '.github/workflows/ci.yml', 'tools/oracle/Dockerfile.compat', 'AGENTS.md'):
            self.assertIn(expected, names)
        for excluded in ('.python-version',):
            self.assertNotIn(excluded, names)
        self.assertFalse([name for name in names if name.startswith(('docs/findings/', 'docs/perf/'))])


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
            stub = stubs / 'uv'
            stub.write_text('#!/bin/sh\n{ pwd; for a in "$@"; do printf "%s\\n" "$a"; done; } > "$RECORD"\n')
            stub.chmod(stub.stat().st_mode | stat.S_IXUSR)
            env = {key: value for key, value in os.environ.items()
                   if key not in ('UV_PYTHON', 'PYTHONHOME', 'PYTHONPATH')}
            env.update(PATH=f'{stubs}{os.pathsep}{os.environ["PATH"]}', RECORD=str(record))
            result = subprocess.run(['uv', 'run', '--no-project', '--managed-python', '--python', checkout_request, 'python', '-X', 'lazy_imports=normal', str(ROOT / 'tools/dev/__main__.py'), 'runner', 'run-all', '--list', '--runners', 'relative/path'],
                                    cwd=outer, env=env, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = record.read_text().splitlines()
        self.assertEqual(Path(lines[0]).name, 'outer', 'the caller\'s working directory is kept')
        argv = lines[1:]
        self.assertEqual(argv[argv.index('--python') + 1], checkout_request)
        self.assertNotIn(other, argv)
        self.assertEqual(argv[-3:], ['--list', '--runners', 'relative/path'], 'the caller\'s arguments are unchanged')


if __name__ == '__main__':
    unittest.main()
