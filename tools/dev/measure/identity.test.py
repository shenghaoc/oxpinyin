#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the host identity readers in tools.dev.measure.identity."""
import os
from pathlib import Path
import platform
import sys
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


if __name__ == '__main__':
    unittest.main()
