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


class CgroupMemoryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.proc, self.cgroup = root / 'proc', root / 'cgroup'
        (self.proc / 'self').mkdir(parents=True)
        (self.proc / 'self/cgroup').write_text('0::/guest\n')
        (self.proc / 'meminfo').write_text('MemAvailable: 16384000 kB\n')
        self.guest = self.cgroup / 'guest'
        self.guest.mkdir(parents=True)
        for directory in (self.cgroup, self.guest):
            (directory / 'memory.max').write_text('max')
            (directory / 'memory.current').write_text('0')
            (directory / 'cpu.max').write_text('max 100000')
        (self.guest / 'memory.max').write_text(str(2560 * 1024**2))
        (self.guest / 'memory.current').write_text(str(2256 * 1024**2))
        self.stats = {key: value * 1024**2 for key, value in dict(
            anon=64, file=2048, kernel=128, sock=16, shmem=0,
            active_file=1024, inactive_file=1024, file_dirty=0,
            file_writeback=0, slab_reclaimable=64).items()}

    def snapshot(self):
        with mock.patch.object(sys, 'platform', 'linux'):
            return host.resource_snapshot(self.proc, self.cgroup)

    def write_stats(self, stats=None, directory=None):
        values = self.stats if stats is None else stats
        path = (self.guest if directory is None else directory) / 'memory.stat'
        path.write_text(''.join(f'{key} {value}\n' for key, value in values.items()))
        return path

    def assert_build_fits(self, snapshot, expected):
        headroom = host.memory_headroom({'OXPINYIN_COMPAT_CONTAINER': '1'}, snapshot)
        self.assertEqual(headroom, 0)
        self.assertEqual(host.fit(snapshot['available_mib'], host.RUSTC_PEAK_MIB, 1,
                                  headroom), int(expected))

    def test_build_cache_heavy_cgroup_admits_the_next_build(self):
        # The recorded Linux fields, completed with explicitly synthetic
        # shmem/sock/slab values: those three were not captured historically.
        self.stats.update(anon=18497536, file=2084945920, kernel=92110848,
                          active_file=285474816, inactive_file=1799475200,
                          file_dirty=4096, file_writeback=0, shmem=0,
                          sock=0, slab_reclaimable=0)
        (self.guest / 'memory.current').write_text('2196336640')
        self.write_stats()
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 2453)
        self.assert_build_fits(snapshot, True)
        self.assertTrue(any('anon=18497536' in note and 'file_dirty=4096' in note
                            for note in snapshot['notes']))

    def test_active_clean_cache_is_available_without_inactive_pages(self):
        self.stats.update(active_file=2048 * 1024**2, inactive_file=0)
        self.write_stats()
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 2352)
        self.assert_build_fits(snapshot, True)

    def test_anonymous_full_cgroup_refuses_the_next_build(self):
        self.stats.update(anon=2400 * 1024**2, file=0, active_file=0, inactive_file=0)
        (self.guest / 'memory.current').write_text(str(2544 * 1024**2))
        self.write_stats()
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 16)
        self.assert_build_fits(snapshot, False)

    def test_dirty_and_writeback_pages_are_both_reserved(self):
        for dirty, writeback in ((2048, 0), (0, 2048), (1024, 1024)):
            with self.subTest(dirty=dirty, writeback=writeback):
                self.stats.update(file_dirty=dirty * 1024**2,
                                  file_writeback=writeback * 1024**2)
                self.write_stats()
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], 304)
                self.assert_build_fits(snapshot, False)

    def test_dirty_writeback_overlap_reservation_is_bounded_by_file_candidates(self):
        self.stats.update(file_dirty=1536 * 1024**2, file_writeback=1536 * 1024**2)
        self.write_stats()
        snapshot = self.snapshot()
        # Their sum exceeds file; reserve all candidates once, without
        # producing negative clean cache or treating max(dirty, writeback)
        # as an exact union. Overlapping pages may be overreserved.
        self.assertEqual(snapshot['available_mib'], 304)
        self.assert_build_fits(snapshot, False)

    def test_shmem_is_excluded_even_when_file_lru_counters_are_larger(self):
        self.stats['shmem'] = self.stats['file']
        self.write_stats()
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 304)
        self.assert_build_fits(snapshot, False)

    def test_file_pages_outside_the_reclaim_lists_are_retained(self):
        self.stats.update(active_file=0, inactive_file=0)
        self.write_stats()
        self.assertEqual(self.snapshot()['available_mib'], 304)

    def test_kernel_components_are_not_added_to_the_aggregate(self):
        self.stats.update({key: value * 1024**2 for key, value in dict(
            kernel_stack=8, pagetables=8, sec_pagetables=4, percpu=4,
            slab_unreclaimable=32, slab=96).items()})
        self.write_stats()
        self.assertEqual(self.snapshot()['available_mib'], 2352)

    def test_large_reclaimable_slab_does_not_gain_room(self):
        self.stats.update(kernel=128 * 1024**2, slab_reclaimable=128 * 1024**2)
        self.write_stats()
        snapshot = self.snapshot()
        # The same room as the 64 MiB slab fixture: slab stays inside kernel.
        self.assertEqual(snapshot['available_mib'], 2352)
        self.assertTrue(any('slab_reclaimable=134217728' in note
                            for note in snapshot['notes']))

    def test_missing_slab_reclaimable_is_not_required(self):
        self.write_stats({key: value for key, value in self.stats.items()
                          if key != 'slab_reclaimable'})
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 2352)
        self.assertFalse(any('fallback to memory.current=' in note
                             for note in snapshot['notes']))

    def test_larger_current_or_stat_total_retains_unclassified_or_new_charges(self):
        self.write_stats()
        for current, room in ((2192, 2352), (2320, 2288)):
            with self.subTest(current=current):
                (self.guest / 'memory.current').write_text(str(current * 1024**2))
                self.assertEqual(self.snapshot()['available_mib'], room)

    def test_each_missing_required_field_falls_back_to_current(self):
        for key in set(self.stats) - {'slab_reclaimable'}:
            with self.subTest(missing=key):
                self.write_stats({name: value for name, value in self.stats.items() if name != key})
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], 304)
                self.assert_build_fits(snapshot, False)
                self.assertTrue(any('fallback to memory.current=' in note and key in note
                                    for note in snapshot['notes']))

    def test_missing_or_unreadable_stat_file_falls_back_to_current(self):
        for unreadable in (False, True):
            with self.subTest(unreadable=unreadable):
                if unreadable:
                    # A directory gives a portable read error, even as root.
                    (self.guest / 'memory.stat').mkdir()
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], 304)
                self.assertTrue(any('fallback to memory.current=' in note
                                    for note in snapshot['notes']))

    def test_unusable_stats_fall_back_instead_of_silently_zeroing_fields(self):
        invalid = ('anon -1', 'anon invalid', 'anon 1 extra', 'anon 1\nanon 2',
                   'shmem 2147483649', 'file_dirty 2147483649',
                   'file_writeback 2147483649', 'slab_reclaimable 134217729')
        for rows in invalid:
            with self.subTest(rows=rows):
                replaced = {line.split()[0] for line in rows.splitlines()}
                path = self.write_stats({key: value for key, value in self.stats.items()
                                         if key not in replaced})
                with path.open('a') as stream:
                    stream.write(rows + '\n')
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], 304)
                self.assertTrue(any('fallback to memory.current=' in note
                                    for note in snapshot['notes']))

    def test_positive_memory_min_falls_back_to_memory_current(self):
        (self.guest / 'memory.min').write_text(str(512 * 1024**2))
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 304)
        self.assertTrue(any(f'memory.min={512 * 1024**2}: protected memory; '
                            f'fallback to memory.current={2256 * 1024**2}' in note
                            for note in snapshot['notes']))

    def test_zero_missing_or_unreadable_memory_min_uses_the_reclaimable_rule(self):
        self.write_stats()
        path = self.guest / 'memory.min'
        for content, note in ((None, 'memory.min=0 (kernel default; missing)'),
                              ('0', 'memory.min=0;'),
                              ('unreadable', 'memory.min=0 (kernel default; unreadable)')):
            with self.subTest(content=content):
                if path.exists():
                    if path.is_dir():
                        path.rmdir()
                    else:
                        path.unlink()
                if content == 'unreadable':
                    path.mkdir()
                elif content is not None:
                    path.write_text(content)
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], 2352)
                self.assertTrue(any(note in line for line in snapshot['notes']))

    def test_memory_min_protects_only_its_own_level(self):
        parent_stats = dict.fromkeys(self.stats, 0)
        parent_stats.update(file=512 * 1024**2, active_file=512 * 1024**2)
        self.write_stats(parent_stats, self.cgroup)
        (self.cgroup / 'memory.max').write_text(str(1024 * 1024**2))
        (self.cgroup / 'memory.current').write_text(str(512 * 1024**2))
        self.write_stats()
        protected = str(512 * 1024**2)
        for root_min, guest_min, room in ((protected, '0', 512),
                                          ('0', protected, 304)):
            with self.subTest(root_min=root_min, guest_min=guest_min):
                (self.cgroup / 'memory.min').write_text(root_min)
                (self.guest / 'memory.min').write_text(guest_min)
                self.assertEqual(self.snapshot()['available_mib'], room)

    def test_nested_ancestors_take_the_smallest_room(self):
        self.write_stats()
        parent_stats = dict(self.stats, anon=256 * 1024**2, file=0, kernel=0,
                            sock=0, active_file=0, inactive_file=0, slab_reclaimable=0)
        self.write_stats(parent_stats, self.cgroup)
        (self.cgroup / 'memory.max').write_text(str(1024 * 1024**2))
        (self.cgroup / 'memory.current').write_text(str(256 * 1024**2))
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 768)
        self.assertEqual(snapshot['memory_limit_mib'], 1024)
        self.assert_build_fits(snapshot, True)

    def test_unknown_finite_ancestor_is_still_noted_and_skipped(self):
        self.write_stats()
        (self.cgroup / 'memory.max').write_text(str(1024 * 1024**2))
        (self.cgroup / 'memory.current').unlink()
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 2352)
        self.assertIn('cgroup / memory room=unknown', snapshot['notes'])

    def test_reclaiming_cache_never_produces_more_room_than_the_limit(self):
        self.stats = dict.fromkeys(self.stats, 0)
        self.stats.update(file=2048 * 1024**2, kernel=128 * 1024**2,
                          active_file=2048 * 1024**2, slab_reclaimable=128 * 1024**2)
        (self.guest / 'memory.current').write_text(str(2176 * 1024**2))
        self.write_stats()
        self.assertEqual(self.snapshot()['available_mib'], 2432)


if __name__ == '__main__':
    unittest.main()
