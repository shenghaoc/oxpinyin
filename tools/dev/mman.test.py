#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the shared POSIX mmap/munmap/mincore binding."""
import ast
import ctypes
import errno
import mmap
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest import mock
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev import darwin, mman
from tools.dev.measure import cold_linux


class MmanTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.file = self.root / 'pages.bin'
        self.file.write_bytes(os.urandom(4 * mman.PAGE_SIZE))

    def test_import_does_not_load_the_c_library(self):
        code = ("import ctypes\n"
                "def fail(*args, **kwargs):\n"
                "    raise AssertionError('libc loaded at import')\n"
                "ctypes.CDLL = fail\n"
                "import tools.dev.mman\n"
                "print('ok')\n")
        result = subprocess.run([sys.executable, '-c', code], cwd=ROOT,
                                capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'ok')

    def test_one_binding_serves_both_hosts(self):
        self.assertEqual(mman.PAGE_SIZE, mmap.PAGESIZE)
        self.assertIs(darwin.map_file, mman.map_file)
        self.assertIs(darwin.unmap, mman.unmap)
        self.assertIs(darwin.file_residency, mman.residency)
        # No second declaration of the shared calls in any of its users.
        for path in (darwin.__file__, cold_linux.__file__, ROOT / 'tools/measure-function-order.py'):
            tree = ast.parse(Path(path).read_text())
            declared = {node.value.attr for node in ast.walk(tree)
                        if isinstance(node, ast.Attribute) and node.attr in ('argtypes', 'restype')
                        and isinstance(node.value, ast.Attribute)}
            self.assertTrue(declared.isdisjoint({'mmap', 'munmap', 'mincore'}), path)

    def test_a_failed_call_raises_oserror_with_its_errno(self):
        with self.file.open('rb') as handle, \
                mock.patch.object(mman, 'map_file', return_value=0x1000), \
                mock.patch.object(mman, 'unmap'), \
                mock.patch.object(mman, '_system') as system, \
                mock.patch.object(mman.ctypes, 'get_errno', return_value=errno.ENOMEM):
            system.return_value.mincore.return_value = -1
            with self.assertRaises(OSError) as raised:
                mman.residency(handle.fileno())
        self.assertEqual(raised.exception.errno, errno.ENOMEM)
        with mock.patch.object(mman, '_system') as system, \
                mock.patch.object(mman.ctypes, 'get_errno', return_value=errno.EBADF):
            system.return_value.mmap.return_value = ctypes.c_void_p(-1).value
            with self.assertRaises(OSError) as raised:
                mman.map_file(-1, 1, mman.PROT_NONE)
        self.assertEqual(raised.exception.errno, errno.EBADF)

    def test_an_empty_file_has_no_pages_and_is_not_mapped(self):
        empty = self.root / 'empty.bin'
        empty.write_bytes(b'')
        with empty.open('rb') as handle, mock.patch.object(mman, 'map_file') as mapping:
            self.assertEqual(mman.residency(handle.fileno()), dict(pages=0, resident=0))
        mapping.assert_not_called()

    def test_a_read_file_has_its_pages_and_a_resident_page(self):
        with self.file.open('rb') as handle:
            handle.read()
            observed = mman.residency(handle.fileno())
        self.assertEqual(observed['pages'], -(-self.file.stat().st_size // mman.PAGE_SIZE))
        self.assertGreaterEqual(observed['resident'], 1)
        self.assertLessEqual(observed['resident'], observed['pages'])

if __name__ == '__main__':
    unittest.main()
