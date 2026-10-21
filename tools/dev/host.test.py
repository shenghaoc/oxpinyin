#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Unit tests for tools.dev.host."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

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

    def test_only_a_hard_limited_compatibility_guest_omits_desktop_headroom(self):
        for env, limit, expected in (({}, 4096, host.HEADROOM_MIB),
                                     ({'OXPINYIN_COMPAT_CONTAINER': '0'}, 4096, host.HEADROOM_MIB),
                                     ({'OXPINYIN_COMPAT_CONTAINER': '1'}, None, host.HEADROOM_MIB),
                                     ({'OXPINYIN_COMPAT_CONTAINER': '1'}, 4096, 0)):
            with self.subTest(env=env, limit=limit):
                resources = dict(available_mib=2048, memory_limit_mib=limit, cpus=2, cargo_cpus=2)
                self.assertEqual(host.memory_headroom(env, resources), expected)
                _, chosen, _ = host.prepare_heavy_env(env, resources)
                self.assertEqual(chosen['CARGO_BUILD_JOBS'][0], 2 if expected == 0 else 0)
                self.assertIn(f'- {expected} MiB headroom', chosen['CARGO_BUILD_JOBS'][1])
                with mock.patch.object(host, 'resource_snapshot', return_value=resources):
                    automatic = host.memory_defaults(env, None)
                self.assertEqual(automatic['--jobs'][0], 2 if expected == 0 else 0)

    def test_cgroup_snapshot_requires_a_visible_finite_limit(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            proc, cgroup = root / 'proc', root / 'cgroup'
            (proc / 'self').mkdir(parents=True)
            (proc / 'self/cgroup').write_text('0::/guest\n')
            (proc / 'meminfo').write_text('MemAvailable: 16384000 kB\n')
            (cgroup / 'guest').mkdir(parents=True)
            for directory in (cgroup, cgroup / 'guest'):
                (directory / 'memory.max').write_text('max')
                (directory / 'memory.current').write_text('0')
                (directory / 'cpu.max').write_text('max 100000')
            env = {'OXPINYIN_COMPAT_CONTAINER': '1'}
            for maximum in ('max', 'invalid', None, str(4096 * 1024**2)):
                with self.subTest(maximum=maximum), mock.patch.object(sys, 'platform', 'linux'):
                    path = cgroup / 'memory.max'
                    if maximum is None:
                        path.unlink()
                    else:
                        path.write_text(maximum)
                    snapshot = host.resource_snapshot(proc, cgroup)
                    finite = maximum == str(4096 * 1024**2)
                    self.assertEqual(snapshot['memory_limit_mib'], 4096 if finite else None)
                    self.assertEqual(host.memory_headroom(env, snapshot), 0 if finite else host.HEADROOM_MIB)

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
