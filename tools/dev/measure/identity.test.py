"""Tests for the host identity readers in tools.dev.measure.identity."""
import os
from pathlib import Path
import platform
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev.measure import identity


class UnameTests(unittest.TestCase):
    def test_fields_follow_the_order_of_the_flags(self):
        names = platform.uname()
        self.assertEqual(identity.uname('s'), names.system)
        self.assertEqual(identity.uname('m'), names.machine)
        self.assertEqual(identity.uname('srm'), f'{names.system} {names.release} {names.machine}')
        self.assertEqual(identity.uname('srvm'), f'{names.system} {names.release} {names.version} {names.machine}')
        self.assertEqual(identity.uname('ms'), f'{names.machine} {names.system}')
        self.assertEqual(identity.uname(), identity.uname('srm'))

    def test_every_field_is_named(self):
        for flag in 'srvm':
            with self.subTest(flag=flag):
                self.assertTrue(identity.uname(flag))

    if hasattr(os, 'uname'):
        # Where the host has uname, the fields are the ones `uname -srvm` prints.
        def test_fields_equal_os_uname(self):
            names = os.uname()
            self.assertEqual(identity.uname('srvm'),
                             f'{names.sysname} {names.release} {names.version} {names.machine}')


class WindowsIdentityTests(unittest.TestCase):
    """The committed-memory input and loaded-module identity of the Windows measurement."""

    def test_input_commit_rounds_each_mapped_file_not_dbms_or_addons(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'table.conf').write_text('default GB_DICTIONARY x a.bin u SYSTEM_FILE\n'
                'default GBK_DICTIONARY x b.bin u SYSTEM_FILE\n'
                'addon 4 x huge.bin u SYSTEM_FILE\n')
            (root/'a.bin').write_bytes(b'x'*4097)
            (root/'b.bin').write_bytes(b'x'*2)
            result = identity.expected_cow_commit(root, page_size=4096)
            self.assertEqual(result['bytes'], 12288)
            self.assertEqual(len(result['files']), 2)

    def test_module_identity_rejects_other_files_and_changed_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory)/'a.dll', Path(directory)/'b.dll'
            a.write_bytes(b'a'); b.write_bytes(b'a')
            expected = identity.module_inventory([str(a)])
            with self.assertRaises(ValueError):
                identity.verify_modules([str(b)], expected)
            a.write_bytes(b'b')
            with self.assertRaises(ValueError):
                identity.verify_modules([str(a)], expected)


if __name__ == '__main__':
    unittest.main()
