#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Backend-matrix and host-backend-probe fixtures with fake host tools."""
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import check


class FakeTools:
    """Write executable fakes into a private PATH and return that environment."""

    def __init__(self, root):
        self.root = Path(root)
        self.bin = self.root / 'bin'
        self.bin.mkdir()

    def add(self, name, text):
        path = self.bin / name
        path.write_text(text)
        path.chmod(0o755)
        return path

    def environment(self, **values):
        # Only the fakes: a removed fake must leave its tool absent, not expose the host's.
        return dict(os.environ, PATH=str(self.bin), **values)


MATRIX_CARGO = '''#!/bin/sh
case "$*" in
  *"kyotocabinet,tkrzw"*|*"kyotocabinet,bdb"*|*"tkrzw,bdb"*|*"kyotocabinet,tkrzw,bdb"*)
    echo "error: more than one store backend selected" >&2; exit 1 ;;
esac
case "$*" in
  *"--no-default-features"*)
    case "$*" in *"--features"*) ;; *) echo "error: no store backend selected" >&2; exit 1 ;; esac ;;
esac
exit 0
'''


class BackendMatrixTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.tools = FakeTools(self.temp.name)
        self.tools.add('cargo', MATRIX_CARGO)

    def matrix(self, **values):
        out, err = io.StringIO(), io.StringIO()
        tools = check.native.Tools(environment=self.tools.environment(**values))
        with mock.patch.object(tools, 'run', wraps=tools.run) as run:
            status = check.backend_matrix(out=out, err=err, tools=tools)
        prefix = ['cargo', 'check', '--locked', '-p', 'oxpinyin-store']
        selections = [[], ['--no-default-features', '--features', 'kyotocabinet'],
                      ['--no-default-features', '--features', 'tkrzw'],
                      ['--no-default-features', '--features', 'bdb'],
                      ['--no-default-features', '--features', 'kyotocabinet,tkrzw'],
                      ['--no-default-features', '--features', 'kyotocabinet,bdb'],
                      ['--no-default-features', '--features', 'tkrzw,bdb'],
                      ['--no-default-features', '--features', 'kyotocabinet,tkrzw,bdb'],
                      ['--no-default-features']]
        self.assertEqual([call.args[0] for call in run.call_args_list],
                         [prefix + args for args in selections])
        return status, out.getvalue(), err.getvalue()

    def test_valid_and_invalid_selections_report_nine_passes(self):
        status, out, err = self.matrix()
        self.assertEqual((status, err), (0, ''))
        self.assertEqual([line.strip() for line in out.splitlines()
                          if line.lstrip().startswith(('PASS', 'FAIL'))],
                         ['PASS'] * 4 + ['PASS (refused by the exactly-one-backend guard)'] * 5)
        self.assertIn('summary: 9 passed, 0 failed', out)

    def test_a_failing_valid_build_prints_the_tail_and_fails(self):
        cargo = '''#!/bin/sh
case "$*" in
  *"--features"*)
    case "$*" in
      *","*) echo "error: more than one store backend selected" >&2; exit 1 ;;
      *) exit 0 ;;
    esac ;;
  *"--no-default-features"*) echo "error: no store backend selected" >&2; exit 1 ;;
  *) printf 'one\\ntwo\\nthree\\n' >&2; exit 5 ;;
esac
'''
        self.tools.add('cargo', cargo)
        status, out, err = self.matrix()
        self.assertEqual((status, err), (1, ''))
        self.assertIn('   FAIL — expected a clean build; tail:\n', out)
        self.assertIn('     three\n', out)
        self.assertIn('summary: 8 passed, 1 failed', out)

    def test_a_guard_that_does_not_fire_and_an_unrelated_refusal_are_failures(self):
        cargo = '''#!/bin/sh
case "$*" in
  *"--features kyotocabinet,tkrzw,bdb"*|*"--features tkrzw,bdb"*)
    echo "error: more than one store backend selected" >&2; exit 1 ;;
  *"--features kyotocabinet,tkrzw"*) echo ok; exit 0 ;;
  *"--features kyotocabinet,bdb"*)
    echo "error: something else" >&2; exit 1 ;;
  *"--features kyotocabinet"*|*"--features tkrzw"*|*"--features bdb"*) exit 0 ;;
  *"--no-default-features"*)
    echo "error: something else" >&2; exit 1 ;;
  *) exit 0 ;;
esac
'''
        self.tools.add('cargo', cargo)
        status, out, err = self.matrix()
        self.assertEqual((status, err), (1, ''))
        self.assertIn('   FAIL — the guard did not fire, build succeeded', out)
        self.assertIn('   FAIL — build refused for another reason:', out)
        self.assertIn('     error: something else', out)
        self.assertIn('summary: 6 passed, 3 failed', out)

    def test_a_missing_cargo_is_named_for_every_valid_selection(self):
        (self.tools.bin / 'cargo').unlink()
        tools = check.native.Tools(environment=self.tools.environment())
        for peer, label in check.VALID_MATRIX:
            with self.subTest(selection=label):
                self.assertEqual(check._matrix_result(tools, peer.split()),
                                 (127, 'cargo: command not found\n'))
        status, out, err = self.matrix()
        self.assertEqual((status, err), (1, ''))
        self.assertEqual([line.strip() for line in out.splitlines()
                          if line.lstrip().startswith(('PASS', 'FAIL'))],
                         ['FAIL — expected a clean build; tail:'] * 4
                         + ['FAIL — build refused for another reason:'] * 5)
        self.assertEqual(out.count('cargo: command not found'), 4)
        self.assertIn('summary: 0 passed, 9 failed', out)

    def test_a_non_executable_cargo_is_status_126_for_every_valid_selection(self):
        self.tools.add('cargo', '#!/bin/sh\nexit 0\n').chmod(0o644)
        tools = check.native.Tools(environment=self.tools.environment())
        for peer, label in check.VALID_MATRIX:
            with self.subTest(selection=label):
                self.assertEqual(check._matrix_result(tools, peer.split())[0], 126)
        status, _, err = self.matrix()
        self.assertEqual((status, err), (1, ''))


PROBE_BASE = '''#!/bin/sh
case "${0##*/}" in
ldconfig) echo 'libpinyin.so.15 (libc6,x86-64) => /usr/lib/libpinyin.so.15' ;;
rpm) echo '1.2.3-4' ;;
dpkg-query|pacman) exit 1 ;;
readelf) printf ' 0x0000000000000001 (NEEDED)             Shared library: [%s]\\n' "$PROBE_NEEDED" ;;
objdump) printf '  NEEDED               %s\\n' "$PROBE_NEEDED" ;;
esac
'''


class BackendProbeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.tools = FakeTools(self.temp.name)
        for name in ('ldconfig', 'rpm', 'dpkg-query', 'pacman', 'readelf', 'objdump'):
            self.tools.add(name, PROBE_BASE)

    def probe(self, needed='libtkrzw.so.1', **values):
        out, err = io.StringIO(), io.StringIO()
        status = check.backend_probe(
            out=out, err=err,
            environment=self.tools.environment(PRETTY_NAME='FixtureOS', PROBE_NEEDED=needed,
                                               **values))
        return status, out.getvalue(), err.getvalue()

    def test_readelf_identifies_the_direct_dbm(self):
        status, out, err = self.probe()
        self.assertEqual((status, err), (0, ''))
        self.assertIn('so      : /usr/lib/libpinyin.so.15\n', out)
        self.assertIn('version : 1.2.3-4\n', out)
        self.assertIn('backend : Tkrzw\n', out)
        self.assertIn('needed  : libtkrzw.so.1\n', out)

    def test_other_backends_and_an_unknown_library(self):
        for needed, label, expected in (
                ('libkyotocabinet.so.16', 'KyotoCabinet', 0),
                ('libdb-5.3.so', 'BerkeleyDB', 0),
                ('libm.so.6', 'unknown (no known DBM library in DT_NEEDED)', 1)):
            with self.subTest(needed=needed):
                status, out, _ = self.probe(needed=needed)
                self.assertEqual(status, expected)
                self.assertIn(f'backend : {label}\n', out)
                self.assertEqual('needed  : ' in out, needed != 'libm.so.6')

    def test_objdump_is_the_fallback_when_readelf_is_absent(self):
        (self.tools.bin / 'readelf').unlink()
        status, out, _ = self.probe()
        self.assertEqual(status, 0)
        self.assertIn('backend : Tkrzw\n', out)

    def test_a_host_readelf_does_not_replace_a_removed_fake(self):
        # Linux CI has binutils on PATH; its readelf cannot read the fixture's library.
        root = Path(self.temp.name) / 'host'
        root.mkdir()
        host = FakeTools(root)
        host.add('readelf', '#!/bin/sh\nexit 1\n')
        (self.tools.bin / 'readelf').unlink()
        with mock.patch.dict(os.environ, PATH=f'{host.bin}{os.pathsep}{os.environ.get("PATH", "")}'):
            status, out, _ = self.probe()
        self.assertEqual(status, 0)
        self.assertIn('backend : Tkrzw\n', out)

    def test_a_failed_inspection_is_status_three(self):
        self.tools.add('readelf', '#!/bin/sh\nexit 1\n')
        status, out, _ = self.probe()
        self.assertEqual(status, 3)
        self.assertIn('backend : unknown (readelf could not read /usr/lib/libpinyin.so.15)\n', out)

    def test_no_inspection_tool_is_status_three(self):
        (self.tools.bin / 'readelf').unlink()
        (self.tools.bin / 'objdump').unlink()
        out, err = io.StringIO(), io.StringIO()
        status = check.backend_probe(
            out=out, err=err,
            environment={'PATH': str(self.tools.bin), 'PRETTY_NAME': 'FixtureOS'})
        out = out.getvalue()
        self.assertEqual(status, 3)
        self.assertIn('need readelf or objdump; install binutils', out)

    def test_a_missing_libpinyin_is_status_two(self):
        self.tools.add('ldconfig', '#!/bin/sh\nexit 0\n')
        self.tools.add('find', '#!/bin/sh\nexit 0\n')
        status, out, _ = self.probe()
        self.assertEqual(status, 2)
        self.assertIn('libpinyin: not found (package not installed)\n', out)


if __name__ == '__main__':
    unittest.main()
