#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Lint: the Python version request is written once, in .python-version (#1246).

Every other file lets that file decide, or reads or passes its contents on. A literal request
anywhere else fails this test, including in a pull request that rebases onto this check. The
dated records under docs/findings/ keep their wording. The fixtures below are
assembled from pieces so this file states no request itself.
"""
from pathlib import Path
import re
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

ALLOWED = ('.python-version', 'docs/findings/')
# A request in uv's forms: the implementation prefix with a version, or an explicit version given
# to `uv python install|find|pin` or `--python`. Managed-install directory names
# (cpython-3.x-linux...), prose such as "CPython 3.14" and version tuples are not requests.
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
                'assert (3, 14, 0) <= sys.version_info[:3] < (3, 15, 0)',
                'PYTHON_MINIMUM = "' + series + '.0"',
                'uv python install',
                'uv python find --managed-python',
                'uv run --no-project --managed-python python -X utf8',
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
        for expected in ('justfile', 'docs/runbooks/setup.md',
                         '.github/workflows/ci.yml', 'tools/oracle/Dockerfile.oracle', 'AGENTS.md'):
            self.assertIn(expected, names)
        for excluded in ('.python-version',):
            self.assertNotIn(excluded, names)
        self.assertFalse([name for name in names if name.startswith('docs/findings/')])


if __name__ == '__main__':
    unittest.main()
