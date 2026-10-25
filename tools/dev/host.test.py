#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Unit tests for tools.dev.host."""
import ctypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host


def write_linux_process(proc, pid, parent, rollup):
    """Write /proc/PID/stat, and smaps_rollup from (Rss MiB, Pss MiB) unless ROLLUP is None."""
    (proc / str(pid)).mkdir()
    (proc / str(pid) / 'stat').write_text(f'{pid} (rustc (a) b) S {parent} {pid} {pid} 0 -1\n')
    if rollup is not None:
        rss, pss = rollup
        (proc / str(pid) / 'smaps_rollup').write_text(
            '55c2b9a6a000-7ffd5e1f9000 ---p 00000000 00:00 0  [rollup]\n'
            f'Rss: {rss * 1024} kB\nPss: {pss * 1024} kB\nPss_Dirty: {pss * 512} kB\n'
            f'Shared_Clean: {(rss - pss) * 1024} kB\n')


def slot_setting(value):
    """Set OXPINYIN_HEAVY_SLOTS to VALUE, or leave it unset for None."""
    environ = {key: item for key, item in os.environ.items() if key != 'OXPINYIN_HEAVY_SLOTS'}
    if value is not None:
        environ['OXPINYIN_HEAVY_SLOTS'] = value
    return mock.patch.dict(os.environ, environ, clear=True)


class HostTests(unittest.TestCase):
    def test_memory_available_is_an_integer_on_macos(self):
        if sys.platform != 'darwin':
            self.skipTest('macOS reader')
        value = host.memory_available_mib()
        self.assertIsInstance(value, int)
        self.assertGreater(value, 0)

    def test_macos_available_counts_speculative_pages_once(self):
        libc = mock.Mock()
        libc.mach_host_self.return_value = 1

        def host_statistics64(_host, _flavor, statistics, _count):
            values = ctypes.cast(statistics, ctypes.POINTER(host._VMStatistics64)).contents
            values.free_count = 1024
            values.inactive_count = 512
            values.speculative_count = 256
            return 0

        def host_page_size(_host, size):
            ctypes.cast(size, ctypes.POINTER(ctypes.c_ulong)).contents.value = 4096
            return 0

        libc.host_statistics64.side_effect = host_statistics64
        libc.host_page_size.side_effect = host_page_size
        with mock.patch.object(host.ctypes, 'CDLL', return_value=libc):
            # free_count already includes the 256 speculative pages; adding
            # speculative_count again would report 7 MiB instead of 6 MiB.
            self.assertEqual(host._macos_available_mib(), 6)

        libc.host_statistics64.side_effect = lambda *_args: 1
        with mock.patch.object(host.ctypes, 'CDLL', return_value=libc):
            self.assertIsNone(host._macos_available_mib())

    def test_macos_tree_footprint_uses_the_shared_darwin_binding(self):
        footprints, children = {1: 100, 2: 30}, {1: [2], 2: []}
        with mock.patch.object(host.darwin, 'footprint',
                               side_effect=lambda pid: footprints[pid]) as footprint, \
                mock.patch.object(host.darwin, 'children',
                                  side_effect=lambda pid: children[pid]) as child:
            self.assertEqual(host._macos_tree_footprint(1), 130)
        self.assertEqual([call.args[0] for call in footprint.call_args_list], [1, 2])
        self.assertEqual([call.args[0] for call in child.call_args_list], [1, 2])

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

    def test_headroom_is_ten_percent_of_total_memory_at_most_2048_mib(self):
        for total, expected in ((8192, 819), (16384, 1638), (32768, 2048), (None, 2048)):
            with self.subTest(total=total):
                self.assertEqual(host.desktop_headroom_mib(total), expected)
                resources = dict(available_mib=4096, memory_limit_mib=None, total_mib=total)
                self.assertEqual(host.memory_headroom({}, resources), expected)
                guest = dict(resources, memory_limit_mib=2560)
                self.assertEqual(host.memory_headroom({'OXPINYIN_COMPAT_CONTAINER': '1'}, guest), 0)

    def test_slot_count_follows_total_memory_unless_set(self):
        for setting, total, expected in ((None, 8192, 1), (None, 16384, 2), (None, 15359, 1),
                                         (None, 15360, 2), (None, None, 1), ('3', 8192, 3),
                                         ('1', 32768, 1), ('0', 16384, 1), ('invalid', 16384, 1)):
            with self.subTest(setting=setting, total=total), slot_setting(setting):
                self.assertEqual(host.heavy_slot_count(total), expected)
                with mock.patch.object(host, 'total_memory_mib', return_value=total):
                    self.assertEqual(host.heavy_slot_count(), expected)
        reader = host.total_memory_mib
        with tempfile.TemporaryDirectory() as temporary, slot_setting(None), \
                mock.patch.object(sys, 'platform', 'linux'):
            # 16 GiB of RAM: Linux reports MemTotal after the kernel's own reservations.
            proc = Path(temporary)
            (proc / 'meminfo').write_text('MemTotal: 16064780 kB\nMemAvailable: 12582912 kB\n')
            self.assertEqual(reader(proc), 15688)
            with mock.patch.object(host, 'total_memory_mib', lambda: reader(proc)):
                self.assertEqual(host.heavy_slot_count(), 2)

    def test_snapshot_reports_the_linux_total_memory(self):
        with tempfile.TemporaryDirectory() as temporary:
            proc = Path(temporary) / 'proc'
            (proc / 'self').mkdir(parents=True)
            for meminfo, total in (('MemTotal: 16777216 kB\nMemAvailable: 8388608 kB\n', 16384),
                                   ('MemAvailable: 8388608 kB\n', None)):
                with self.subTest(total=total), mock.patch.object(sys, 'platform', 'linux'):
                    (proc / 'meminfo').write_text(meminfo)
                    snapshot = host.resource_snapshot(proc, Path(temporary) / 'cgroup')
                    self.assertEqual(snapshot['total_mib'], total)
                    self.assertIn(f'MemTotal={total} MiB' if total else 'MemTotal=unknown', snapshot['notes'])

    def test_default_jobs_take_one_slot_share_of_the_room(self):
        # 16 GiB: 1638 MiB headroom; 8000 - 1638 = 6362 MiB of room.
        resources = dict(available_mib=8000, memory_limit_mib=None, total_mib=16384,
                         cpus=10, cargo_cpus=10)
        for setting, jobs, runners in (('1', 10, 2), ('2', 5, 2)):
            with self.subTest(slots=setting), slot_setting(setting):
                chosen = host.memory_defaults({}, None, cpus=10, available_mib=8000, total_mib=16384)
                self.assertEqual(chosen['CARGO_BUILD_JOBS'][0], jobs)
                self.assertEqual(chosen['--jobs'][0], runners)
                self.assertIn(f'8000 MiB available - 1638 MiB headroom, a 1/{setting} share',
                              chosen['CARGO_BUILD_JOBS'][1])
                child, _, estimate = host.prepare_heavy_env({}, resources)
                self.assertEqual(child['CARGO_BUILD_JOBS'], str(jobs))
                self.assertEqual(estimate, jobs * host.RUSTC_PEAK_MIB)
        with slot_setting(None):
            # The total alone selects two slots.
            self.assertEqual(host.prepare_heavy_env({}, resources)[0]['CARGO_BUILD_JOBS'], '5')
            explicit = host.prepare_heavy_env({'CARGO_BUILD_JOBS': '8'}, resources)
            self.assertEqual(explicit[0]['CARGO_BUILD_JOBS'], '8')
            self.assertEqual(explicit[2], 8 * host.RUSTC_PEAK_MIB)

    def test_one_rustc_is_the_minimum_while_it_fits_the_whole_room(self):
        with slot_setting('2'):
            # 1000 MiB of room: a half share fits no rustc, the whole room fits one.
            child, chosen, estimate = host.prepare_heavy_env(
                {}, dict(available_mib=2638, total_mib=16384, cpus=10, cargo_cpus=10))
            self.assertEqual(child['CARGO_BUILD_JOBS'], '1')
            self.assertEqual(estimate, host.RUSTC_PEAK_MIB)
            self.assertIn('one rustc, which fits the whole room', chosen['CARGO_BUILD_JOBS'][1])
            child, _, estimate = host.prepare_heavy_env(
                {}, dict(available_mib=2237, total_mib=16384, cpus=10, cargo_cpus=10))
            self.assertEqual(child['CARGO_BUILD_JOBS'], '0')
            self.assertEqual(estimate, host.RUSTC_PEAK_MIB)

    def test_linux_tree_use_sums_pss_so_shared_pages_count_once(self):
        with tempfile.TemporaryDirectory() as temporary:
            proc = Path(temporary)
            # pid: (parent, (Rss MiB, Pss MiB)). The rustc children 101 and 102
            # share 300 MiB of library pages; 103 is a grandchild, 200 unrelated.
            for pid, (parent, rollup) in {100: (1, (60, 50)), 101: (100, (400, 250)),
                                          102: (100, (400, 250)), 103: (101, (30, 30)),
                                          200: (1, (900, 900))}.items():
                write_linux_process(proc, pid, parent, rollup)
            (proc / '104').mkdir()  # exited during the walk: no stat
            (proc / 'self').mkdir()
            with mock.patch.object(sys, 'platform', 'linux'):
                # The sum of Pss, 50 + 250 + 250 + 30; the sum of Rss is 890.
                self.assertEqual(host.process_tree_use_mib(100, proc), 580)
                self.assertEqual(host.process_tree_use_mib(101, proc), 280)
                self.assertIsNone(host.process_tree_use_mib(104, proc))
                self.assertIsNone(host.process_tree_use_mib(999, proc))

    def test_linux_tree_use_is_unknown_when_a_pss_cannot_be_read(self):
        with tempfile.TemporaryDirectory() as temporary:
            proc = Path(temporary)
            for pid, (parent, rollup) in {100: (1, (60, 50)), 101: (100, (400, 250)),
                                          102: (100, None)}.items():
                write_linux_process(proc, pid, parent, rollup)
            rollup = proc / '102' / 'smaps_rollup'
            with mock.patch.object(sys, 'platform', 'linux'):
                # Missing, without a Pss row, or with an unreadable value.
                for content in (None, 'Rss: 409600 kB\n', 'Pss: many kB\n'):
                    with self.subTest(content=content):
                        if content is not None:
                            rollup.write_text(content)
                        self.assertIsNone(host.process_tree_use_mib(100, proc))
                self.assertEqual(host.process_tree_use_mib(101, proc), 250)

    def test_tree_use_of_a_live_process_includes_its_children(self):
        grandchild = ('import os, sys; data = os.urandom(64 * 1024 * 1024); '
                      'print("ready", flush=True); sys.stdin.readline()')
        child = ('import subprocess, sys; '
                 f'process = subprocess.Popen([sys.executable, "-c", {grandchild!r}], '
                 'stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True); '
                 'print(process.stdout.readline().strip(), flush=True); sys.stdin.readline(); '
                 'process.stdin.close(); process.wait()')
        process = subprocess.Popen([sys.executable, '-c', child], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, text=True)
        try:
            self.assertEqual(process.stdout.readline().strip(), 'ready')
            self.assertGreaterEqual(host.process_tree_use_mib(process.pid), 64)
        finally:
            process.stdin.close()
            process.wait(timeout=10)
            process.stdout.close()
        self.assertIsNone(host.process_tree_use_mib(process.pid))

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


class CgroupV1Tests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.proc = self.root / 'proc'
        self.unified = self.root / 'sys/fs/cgroup/unified'
        # Deliberately outside the default /sys/fs/cgroup/memory location.
        self.memory = self.root / 'controllers/memory mount'
        self.cpu = self.root / 'controllers/cpu,cpuacct'
        (self.proc / 'self').mkdir(parents=True)
        (self.proc / 'self/cgroup').write_text(
            '9:name=systemd:/unrelated\n7:cpuacct,cpu:/session\n3:memory:/session\n')
        (self.proc / 'meminfo').write_text('MemAvailable: 16384000 kB\n')
        self.mounts = [self.mount(self.memory, 'memory'), self.mount(self.cpu, 'cpu,cpuacct')]
        (self.proc / 'self/mountinfo').write_text(''.join(self.mounts))
        for directory in (self.memory, self.memory / 'session'):
            directory.mkdir(parents=True)
            self.write_memory(directory, '9223372036854771712', 0, 0)
        for directory in (self.cpu, self.cpu / 'session'):
            directory.mkdir(parents=True)
            (directory / 'cpu.cfs_quota_us').write_text('-1')
            (directory / 'cpu.cfs_period_us').write_text('100000')
        self.write_memory(self.memory / 'session', str(4096 * 1024**2), 3072, 512)

    def mount(self, directory, controllers, root='/'):
        escaped = str(directory).replace('\\', r'\134').replace(' ', r'\040')
        return f'30 20 0:27 {root} {escaped} rw,nosuid - cgroup cgroup rw,{controllers}\n'

    def write_memory(self, directory, limit, usage, inactive):
        (directory / 'memory.limit_in_bytes').write_text(limit)
        (directory / 'memory.usage_in_bytes').write_text(str(usage * 1024**2))
        (directory / 'memory.stat').write_text(
            self.stat_rows(dict(cache=64, shmem=0, active_file=0, inactive_file=64,
                                dirty=0, writeback=0), '')
            + self.stat_rows(dict(cache=inactive, shmem=0, active_file=0,
                                  inactive_file=inactive, dirty=0, writeback=0)))

    def stat_rows(self, stats, prefix='total_'):
        return ''.join(f'{prefix}{key} {value * 1024**2}\n' for key, value in stats.items())

    def snapshot(self):
        with mock.patch.object(sys, 'platform', 'linux'), \
                mock.patch.object(os, 'sched_getaffinity', return_value={0, 1, 2, 3}):
            return host.resource_snapshot(self.proc, self.unified)

    def test_v1_session_room_below_host_memory_from_nondefault_mount(self):
        snapshot = self.snapshot()
        self.assert_budget(snapshot, 1536, 2)
        self.assertEqual(snapshot['memory_limit_mib'], 4096)
        self.assertEqual(snapshot['cpus'], 4)
        self.assertIn(f'cgroup v1 /session memory.limit_in_bytes={4096 * 1024**2}',
                      snapshot['notes'])
        self.assertTrue(any('total_inactive_file=536870912' in note
                            and 'memory room=1536 MiB' in note for note in snapshot['notes']))

    def test_unlimited_sentinels_do_not_count_as_finite_limits(self):
        for limit in ('9223372036854771712', str((1 << 63) - 1)):
            with self.subTest(limit=limit):
                (self.memory / 'session/memory.limit_in_bytes').write_text(limit)
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], 16000)
                self.assertIsNone(snapshot['memory_limit_mib'])
                self.assertIn(f'cgroup v1 /session memory.limit_in_bytes={limit} (unlimited)',
                              snapshot['notes'])

    def test_tighter_ancestor_constrains_room(self):
        self.write_memory(self.memory, str(2048 * 1024**2), 1536, 0)
        self.assertEqual(self.snapshot()['available_mib'], 512)
        self.assertEqual(self.snapshot()['memory_limit_mib'], 2048)

    def test_local_fields_are_used_without_total_cache(self):
        stats = dict(cache=128, shmem=0, active_file=64, inactive_file=64, dirty=0, writeback=0)
        # Partial hierarchical fields cannot replace any of the local set.
        (self.memory / 'session/memory.stat').write_text(
            self.stat_rows(stats, '') + f'total_inactive_file {512 * 1024**2}\n')
        self.assert_budget(self.snapshot(), 1152, 1)

    def test_pending_dirty_and_writeback_pages_reduce_room(self):
        for prefix in ('total_', ''):
            for dirty, writeback, room in ((128, 0, 1408), (0, 128, 1408),
                                           (128, 128, 1280), (400, 400, 1024)):
                with self.subTest(prefix=prefix, dirty=dirty, writeback=writeback):
                    stats = dict(cache=512, shmem=0, active_file=0, inactive_file=512,
                                 dirty=dirty, writeback=writeback)
                    (self.memory / 'session/memory.stat').write_text(self.stat_rows(stats, prefix))
                    self.assertEqual(self.snapshot()['available_mib'], room)

    def test_active_clean_cache_is_reclaimable(self):
        for prefix in ('total_', ''):
            with self.subTest(prefix=prefix):
                stats = dict(cache=512, shmem=0, active_file=512, inactive_file=0,
                             dirty=0, writeback=0)
                (self.memory / 'session/memory.stat').write_text(self.stat_rows(stats, prefix))
                self.assert_budget(self.snapshot(), 1536, 2)

    def test_cache_candidates_exclude_shmem_and_are_bounded_by_file_lists(self):
        for prefix in ('total_', ''):
            for shmem, active, inactive, room in ((128, 512, 512, 1408),
                                                  (0, 128, 128, 1280),
                                                  (512, 512, 512, 1024)):
                with self.subTest(prefix=prefix, shmem=shmem, active=active, inactive=inactive):
                    stats = dict(cache=512, shmem=shmem, active_file=active,
                                 inactive_file=inactive, dirty=0, writeback=0)
                    (self.memory / 'session/memory.stat').write_text(self.stat_rows(stats, prefix))
                    self.assertEqual(self.snapshot()['available_mib'], room)

    def test_missing_fields_never_mix_hierarchical_and_local_counters(self):
        stats = dict(cache=512, shmem=0, active_file=0, inactive_file=512, dirty=0, writeback=0)
        for prefix in ('total_', ''):
            for missing in stats:
                with self.subTest(prefix=prefix, missing=missing):
                    rows = self.stat_rows({key: value for key, value in stats.items()
                                           if key != missing}, prefix)
                    # Even a complete local set cannot fill a missing total.
                    # Do not introduce total_cache for local cases: that
                    # selects a different set.
                    if missing != 'cache':
                        rows += (self.stat_rows(stats, '') if prefix else
                                 self.stat_rows({missing: stats[missing]}))
                    (self.memory / 'session/memory.stat').write_text(rows)
                    self.assert_conservative_room()

    def assert_budget(self, snapshot, room, jobs):
        self.assertEqual(snapshot['available_mib'], room)
        # A finite compatibility guest uses the cgroup room without desktop
        # headroom. Fix the slot count so host settings cannot hide the budget.
        with slot_setting('1'):
            child, chosen, estimate = host.prepare_heavy_env(
                {'OXPINYIN_COMPAT_CONTAINER': '1'}, snapshot)
        self.assertEqual(child['CARGO_BUILD_JOBS'], str(jobs))
        self.assertEqual(chosen['CARGO_BUILD_JOBS'][0], jobs)
        self.assertIn(f'{room} MiB available - 0 MiB headroom', chosen['CARGO_BUILD_JOBS'][1])
        self.assertEqual(estimate, max(1, jobs) * host.RUSTC_PEAK_MIB)

    def assert_conservative_room(self, room=1024, jobs=1):
        snapshot = self.snapshot()
        self.assertTrue(any('cgroup v1 /session' in note and 'memory.stat unusable' in note
                            and 'reclaimable cache=unknown' in note
                            and f'memory room={room} MiB' in note
                            for note in snapshot['notes']))
        self.assert_budget(snapshot, room, jobs)

    def test_missing_or_unreadable_stats_retain_limit_minus_usage(self):
        (self.proc / 'meminfo').write_text(
            'MemTotal: 33554432 kB\nMemAvailable: 33554432 kB\n')
        for usage, room in ((768, 256), (1024, 0), (1280, 0)):
            for unreadable in (False, True):
                with self.subTest(usage=usage, unreadable=unreadable):
                    self.write_memory(self.memory / 'session', str(1024 * 1024**2), usage, 512)
                    path = self.memory / 'session/memory.stat'
                    path.unlink()
                    if unreadable:
                        # A directory also fails to read when tests run as root.
                        path.mkdir()
                    self.assert_conservative_room(room, 0)
                    if unreadable:
                        path.rmdir()

    def test_room_is_clamped_between_zero_and_limit(self):
        for usage, inactive, room in ((5000, 0, 0), (100, 200, 4096)):
            with self.subTest(usage=usage, inactive=inactive):
                self.write_memory(self.memory / 'session', str(4096 * 1024**2), usage, inactive)
                self.assertEqual(self.snapshot()['available_mib'], room)

    def test_cpu_quota_combines_with_affinity_and_ancestors(self):
        for quota, cpus, cargo_cpus in ((200000, 2, 2), (250000, 2, 3), (50000, 1, 1),
                                       (800000, 4, 4), (-1, 4, 4)):
            with self.subTest(quota=quota):
                (self.cpu / 'session/cpu.cfs_quota_us').write_text(str(quota))
                snapshot = self.snapshot()
                self.assertEqual((snapshot['cpus'], snapshot['cargo_cpus']), (cpus, cargo_cpus))
        (self.cpu / 'cpu.cfs_quota_us').write_text('100000')
        self.assertEqual(self.snapshot()['cpus'], 1)

    def test_hybrid_empty_unified_hierarchy_adds_no_capacity(self):
        self.unified.mkdir(parents=True)
        (self.unified / 'cgroup.controllers').write_text('')
        (self.proc / 'self/cgroup').write_text('0::/\n7:cpuacct,cpu:/session\n3:memory:/session\n')
        with (self.proc / 'self/mountinfo').open('a') as stream:
            stream.write(f'40 20 0:30 / {self.unified} rw - cgroup2 cgroup rw\n')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 1536)
        self.assertEqual(snapshot['cpus'], 4)

    def test_hybrid_combines_v2_cpu_with_v1_memory(self):
        self.unified.mkdir(parents=True)
        (self.proc / 'self/cgroup').write_text('0::/\n3:memory:/session\n')
        (self.unified / 'cpu.max').write_text('200000 100000')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 1536)
        self.assertEqual(snapshot['cpus'], 2)

    def test_mount_root_limits_the_visible_ancestor_walk(self):
        (self.proc / 'self/cgroup').write_text('3:memory:/tenant/session\n')
        (self.proc / 'self/mountinfo').write_text(self.mount(self.memory, 'memory', '/tenant'))
        # A tighter file outside the mount is not a visible ancestor.
        self.write_memory(self.memory.parent, '0', 0, 0)
        directories = host.linux_cgroup_dirs(self.proc, controller='memory')
        self.assertEqual(directories, [self.memory / 'session', self.memory])
        self.assertEqual(self.snapshot()['available_mib'], 1536)
        # A process at the mount root reads exactly that one visible level.
        (self.proc / 'self/cgroup').write_text('3:memory:/tenant\n')
        self.write_memory(self.memory, str(1024 * 1024**2), 256, 0)
        self.assertEqual(host.linux_cgroup_dirs(self.proc, controller='memory'), [self.memory])
        self.assertEqual(self.snapshot()['available_mib'], 768)

    def test_inaccessible_or_invalid_mounts_remain_unknown(self):
        for entry, mountinfo in (('/outside', self.mount(self.memory, 'memory', '/tenant')),
                                  ('/tenant/../session', self.mounts[0]),
                                  ('/session', ''), ('/session', 'invalid')):
            with self.subTest(entry=entry, mountinfo=mountinfo):
                (self.proc / 'self/cgroup').write_text(f'3:memory:{entry}\n')
                (self.proc / 'self/mountinfo').write_text(mountinfo)
                snapshot = self.snapshot()
                self.assertIn('cgroup v1 memory limits=unknown', snapshot['notes'])
                self.assertIsNone(snapshot['memory_limit_mib'])

    def test_unreadable_memory_values_are_unknown_never_host_room(self):
        for filename in ('memory.limit_in_bytes', 'memory.usage_in_bytes'):
            with self.subTest(filename=filename):
                path = self.memory / 'session' / filename
                content = path.read_text()
                path.unlink()
                # A directory produces a read error even when tests run as root.
                path.mkdir()
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], 16000)
                self.assertTrue(any('cgroup v1 /session' in note and 'unknown' in note
                                    for note in snapshot['notes']))
                self.assertFalse(any('memory room=16000' in note for note in snapshot['notes']))
                (self.proc / 'meminfo').write_text('')
                self.assertIsNone(self.snapshot()['available_mib'])
                (self.proc / 'meminfo').write_text('MemAvailable: 16384000 kB\n')
                path.rmdir()
                path.write_text(content)

    def test_bad_stats_do_not_fall_back_to_local_cache_or_host_room(self):
        for stats in ('', 'inactive_file -1', 'total_inactive_file invalid\ninactive_file 0',
                      'total_inactive_file 1\ntotal_inactive_file 2', 'inactive_file 1 extra'):
            with self.subTest(stats=stats):
                (self.memory / 'session/memory.stat').write_text(stats)
                self.assert_conservative_room()

    def test_each_selected_stat_must_be_numeric_nonnegative_and_unique(self):
        stats = dict(cache=512, shmem=0, active_file=0, inactive_file=512, dirty=0, writeback=0)
        for prefix in ('total_', ''):
            for key in stats:
                for invalid in ('-1', 'invalid', '1.5', '0\n' + prefix + key + ' 0'):
                    with self.subTest(prefix=prefix, key=key, invalid=invalid):
                        rows = self.stat_rows({name: value for name, value in stats.items()
                                               if name != key}, prefix)
                        rows += f'{prefix}{key} {invalid}\n'
                        (self.memory / 'session/memory.stat').write_text(rows)
                        self.assert_conservative_room()

    def test_shmem_dirty_and_writeback_cannot_exceed_cache(self):
        for prefix in ('total_', ''):
            for key in ('shmem', 'dirty', 'writeback'):
                with self.subTest(prefix=prefix, key=key):
                    stats = dict(cache=512, shmem=0, active_file=0, inactive_file=512,
                                 dirty=0, writeback=0)
                    stats[key] = 513
                    (self.memory / 'session/memory.stat').write_text(self.stat_rows(stats, prefix))
                    self.assert_conservative_room()

    def test_unreadable_cpu_files_are_unknown(self):
        (self.cpu / 'session/cpu.cfs_quota_us').write_text('200000')
        for filename in ('cpu.cfs_quota_us', 'cpu.cfs_period_us'):
            with self.subTest(filename=filename):
                path = self.cpu / 'session' / filename
                content = path.read_text()
                path.unlink()
                snapshot = self.snapshot()
                self.assertEqual(snapshot['cpus'], 4)
                self.assertIn('cgroup v1 /session CPU quota=unknown', snapshot['notes'])
                path.write_text(content)


class ContractWorkerEstimates(unittest.TestCase):
    def prepare(self, available):
        env = dict(CARGO_BUILD_JOBS='1', RUST_TEST_THREADS='1',
                   OXPINYIN_CONTRACT_JOBS='8')
        resources = dict(available_mib=available, total_mib=16384,
                         cpus=4, cargo_cpus=4)
        return host.prepare_heavy_env(env, resources, runner=True)

    def test_requested_workers_are_charged_individually(self):
        with slot_setting('3'):
            child, chosen, estimate = self.prepare(1638 + 8 * host.RUNNER_PEAK_MIB)
        self.assertEqual(child['OXPINYIN_CONTRACT_JOBS'], '8')
        self.assertEqual(chosen['OXPINYIN_CONTRACT_JOBS'][0], 8)
        self.assertEqual(estimate, 8 * host.RUNNER_PEAK_MIB)

    def test_short_memory_reduces_the_requested_maximum(self):
        child, chosen, estimate = self.prepare(1638 + 2 * host.RUNNER_PEAK_MIB)
        self.assertEqual(child['OXPINYIN_CONTRACT_JOBS'], '2')
        self.assertEqual(chosen['OXPINYIN_CONTRACT_JOBS'][0], 2)
        self.assertEqual(estimate, 2 * host.RUNNER_PEAK_MIB)

    def test_unknown_or_insufficient_room_does_not_claim_capacity(self):
        for available in (None, 1638 + host.RUNNER_PEAK_MIB - 1):
            with self.subTest(available=available):
                _, chosen, estimate = self.prepare(available)
                self.assertEqual(chosen['OXPINYIN_CONTRACT_JOBS'][0], 0)
                self.assertEqual(estimate, host.RUNNER_PEAK_MIB)

    def test_worker_budget_does_not_replace_explicit_build_budget(self):
        child, _, estimate = host.prepare_heavy_env(
            dict(CARGO_BUILD_JOBS='8', RUST_TEST_THREADS='1', OXPINYIN_CONTRACT_JOBS='1'),
            dict(available_mib=16000, total_mib=16384, cpus=4, cargo_cpus=4), runner=True)
        self.assertEqual(child['CARGO_BUILD_JOBS'], '8')
        self.assertEqual(estimate, 8 * host.RUSTC_PEAK_MIB)

    def test_invalid_worker_counts_are_rejected(self):
        for value in ('', '0', '-1', '1.5', 'default', 'nan'):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, 'OXPINYIN_CONTRACT_JOBS'):
                host.validate_settings({'OXPINYIN_CONTRACT_JOBS': value})


if __name__ == '__main__':
    unittest.main()
