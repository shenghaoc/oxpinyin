#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Host-backend-probe fixtures with fake host tools."""
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
