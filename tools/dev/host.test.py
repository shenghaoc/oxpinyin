#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Unit tests for tools.dev.host."""
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host


class HostTests(unittest.TestCase):
    def test_memory_available_is_an_integer_on_macos(self):
        if sys.platform != 'darwin':
            self.skipTest('macOS reader')
        value = host.memory_available_mib()
        self.assertIsInstance(value, int)
        self.assertGreater(value, 0)

    def test_resource_snapshot_reports_available_memory_and_cpus(self):
        snapshot = host.resource_snapshot()
        self.assertIsInstance(snapshot['cpus'], int)
        self.assertGreaterEqual(snapshot['cpus'], 1)
        self.assertIn('available_mib', snapshot)
        self.assertTrue(snapshot['available_mib'] is None
                        or isinstance(snapshot['available_mib'], int))

    def test_fit_leaves_the_headroom_and_clamps(self):
        self.assertEqual(host.fit(16000, 728, 4), 4)
        self.assertEqual(host.fit(3000, 728, 4), 1)
        self.assertEqual(host.fit(2100, 728, 4), 0)

    def test_relative_lock_directory_is_rejected(self):
        old = host.os.environ.get('OXPINYIN_LOCK_DIR')
        host.os.environ['OXPINYIN_LOCK_DIR'] = 'relative'
        try:
            with self.assertRaisesRegex(ValueError, 'must be an absolute path'):
                host.host_lock_dir()
        finally:
            if old is None:
                host.os.environ.pop('OXPINYIN_LOCK_DIR', None)
            else:
                host.os.environ['OXPINYIN_LOCK_DIR'] = old


if __name__ == '__main__':
    unittest.main()
