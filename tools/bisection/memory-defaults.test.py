#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for run-all.py's memory-aware concurrency defaults."""
import io
import contextlib
import json
import os
import re
import select
import signal
import tempfile
import time
from unittest import mock
from pathlib import Path
import runpy
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import test_support
from tools.dev import host

RUN_ALL = Path(__file__).with_name('run-all.py')
run_all = runpy.run_path(str(RUN_ALL))
M = run_all


def catalog_command(row, command):
    """Append the row's implementation path so a synthetic catalog validates."""
    form, _, target = row['implementation'].partition(':')
    return [*command, *(['-m', target] if form == 'python' else ['{root}/' + target])]


class FitTests(unittest.TestCase):
    def test_fit_leaves_the_headroom_and_clamps(self):
        self.assertEqual(M['fit'](16000, 728, 4), 4)
        self.assertEqual(M['fit'](3000, 728, 4), 1)      # (3000-2048)//728 = 1
        self.assertEqual(M['fit'](2100, 728, 4), 0)      # No process fits.
        self.assertEqual(M['fit'](3500, 64, 4), 4)       # test threads clamp to the CPUs
        self.assertEqual(M['fit'](16000, 1024, 2), 2)    # runner jobs clamp to 2


class DefaultsTests(unittest.TestCase):
    def test_plenty_of_memory_gives_the_cpu_count_and_two_jobs(self):
        chosen = M['memory_defaults']({}, None, cpus=4, available_mib=16000)
        self.assertEqual({k: v[0] for k, v in chosen.items()}, {'CARGO_BUILD_JOBS': 4, 'RUST_TEST_THREADS': 4, '--jobs': 2})
        self.assertFalse(any(v[2] for v in chosen.values()))
        self.assertIn('16000 MiB available - 2048 MiB headroom', chosen['CARGO_BUILD_JOBS'][1])

    def test_little_memory_lowers_the_build_jobs_and_runner_jobs(self):
        chosen = M['memory_defaults']({}, None, cpus=8, available_mib=3000)
        self.assertEqual({k: v[0] for k, v in chosen.items()}, {'CARGO_BUILD_JOBS': 1, 'RUST_TEST_THREADS': 8, '--jobs': 0})

    def test_explicit_settings_always_win(self):
        chosen = M['memory_defaults']({'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '3'}, 2, cpus=4, available_mib=3000)
        self.assertEqual({k: v[0] for k, v in chosen.items()}, {'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '3', '--jobs': 2})
        self.assertTrue(all(v[2] for v in chosen.values()))
        self.assertEqual(chosen['--jobs'][1], 'set by the caller')

    def test_unreadable_memory_has_no_automatic_capacity(self):
        chosen = M['memory_defaults']({}, None, cpus=4, available_mib=None)
        self.assertEqual({k: v[0] for k, v in chosen.items()}, {'CARGO_BUILD_JOBS': 0, 'RUST_TEST_THREADS': 0, '--jobs': 0})
        self.assertIn('available memory unknown; no capacity admitted', chosen['--jobs'][1])
        chosen = M['memory_defaults']({}, None, cpus=1, available_mib=None)
        self.assertEqual(chosen['CARGO_BUILD_JOBS'][0], 0)

    def test_plan_lines(self):
        out = io.StringIO()
        M['plan_defaults'](M['memory_defaults']({'CARGO_BUILD_JOBS': '2'}, None, cpus=4, available_mib=16000), out)
        lines = out.getvalue().splitlines()
        self.assertEqual(lines[0], 'defaults: CARGO_BUILD_JOBS=2 (explicit: set by the caller)')
        self.assertTrue(lines[1].startswith('defaults: RUST_TEST_THREADS=4 (16000 MiB available'))
        self.assertTrue(lines[2].startswith('defaults: --jobs=2 ('))

    def test_this_host_reads_its_memory(self):
        available = M['memory_available_mib']()
        if sys.platform.startswith('linux'):
            self.assertIsInstance(available, int)
            self.assertGreater(available, 0)

    def test_run_all_prints_the_defaults_in_the_plan(self):
        result = subprocess.run([sys.executable, str(RUN_ALL), '--runners', 'pc-version', '--no-build', '--cells', 'bdb'],
                                capture_output=True, text=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([l.split('=')[0] for l in result.stderr.splitlines() if l.startswith('provisional defaults: ')],
                         ['provisional defaults: CARGO_BUILD_JOBS', 'provisional defaults: RUST_TEST_THREADS', 'provisional defaults: --jobs'])


def resources(memory=16000, cpus=4):
    return dict(available_mib=memory, cpus=cpus, cargo_cpus=cpus,
                notes=[f'synthetic memory={memory} MiB; CPUs={cpus}'])


class CgroupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.proc = self.root / 'proc'
        (self.proc / 'self').mkdir(parents=True)
        (self.proc / 'self/cgroup').write_text('0::/parent/child\n')
        (self.proc / 'meminfo').write_text('MemAvailable: 16384000 kB\n')
        self.cg = self.root / 'cgroup'
        (self.cg / 'parent/child').mkdir(parents=True)
        for directory in (self.cg, self.cg / 'parent', self.cg / 'parent/child'):
            (directory / 'memory.max').write_text('max')
            (directory / 'memory.current').write_text('0')
            (directory / 'cpu.max').write_text('max 100000')

    def snapshot(self):
        with mock.patch.object(sys, 'platform', 'linux'), mock.patch.object(host, 'allowed_cpus', return_value=set(range(8))):
            return M['resource_snapshot'](self.proc, self.cg)

    def test_parent_memory_and_cpu_limits_control_the_choice(self):
        parent = self.cg / 'parent'
        (parent / 'memory.max').write_text(str(4000 * 1024**2))
        (parent / 'memory.current').write_text(str(1000 * 1024**2))
        (parent / 'cpu.max').write_text('200000 100000')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 3000)
        self.assertEqual(snapshot['cpus'], 2)
        child, _, _ = M['prepare_heavy_env']({}, snapshot)
        self.assertEqual(child['CARGO_BUILD_JOBS'], '1')
        self.assertEqual(child['RUST_TEST_THREADS'], '2')

    def test_unknown_limits_remain_unknown_in_the_reason(self):
        (self.cg / 'parent/memory.current').unlink()
        (self.cg / 'parent/memory.max').write_text('100000000')
        (self.cg / 'parent/child/cpu.max').write_text('invalid')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 16000)
        self.assertIn('cgroup /parent memory room=unknown', snapshot['notes'])
        self.assertIn('cgroup /parent/child CPU quota=unknown', snapshot['notes'])
        self.assertIn('cgroup / memory.max=max', snapshot['notes'])

    def test_affinity_and_fractional_quota_bound_cpu_defaults(self):
        (self.cg / 'parent/cpu.max').write_text('150000 100000')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['cpus'], 1)
        self.assertEqual(snapshot['cargo_cpus'], 2)
        (self.cg / 'parent/cpu.max').write_text('1600000 100000')
        self.assertEqual(self.snapshot()['cpus'], 8)

    def test_unknown_cgroup_uses_known_host_resources(self):
        (self.proc / 'self/cgroup').unlink()
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 16000)
        self.assertEqual(snapshot['cpus'], 8)
        self.assertIn('cgroup v2 memory and CPU limits=unknown', snapshot['notes'])

    def write_cache_stats(self, directory):
        stats = dict(anon=64, file=2048, kernel=128, sock=16, shmem=0,
                     active_file=2048, inactive_file=0, file_dirty=0,
                     file_writeback=0, slab_reclaimable=64)
        (directory / 'memory.max').write_text(str(2560 * 1024**2))
        (directory / 'memory.current').write_text(str(2256 * 1024**2))
        (directory / 'memory.stat').write_text(''.join(
            f'{key} {value * 1024**2}\n' for key, value in stats.items()))

    def test_active_build_cache_allows_compatibility_defaults(self):
        self.write_cache_stats(self.cg / 'parent/child')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 2352)
        chosen = M['memory_defaults']({'OXPINYIN_COMPAT_CONTAINER': '1'}, None,
                                     cpus=snapshot['cpus'],
                                     available_mib=snapshot['available_mib'],
                                     memory_limit_mib=snapshot['memory_limit_mib'])
        self.assertEqual(chosen['CARGO_BUILD_JOBS'][0], 3)
        self.assertEqual(chosen['--jobs'][0], 2)

    def test_mixed_stat_and_current_levels_take_the_smallest_room(self):
        self.write_cache_stats(self.cg / 'parent/child')
        parent = self.cg / 'parent'
        (parent / 'memory.current').write_text(str(1000 * 1024**2))
        for limit, room in ((4000, 2352), (2048, 1048)):
            with self.subTest(parent_limit=limit):
                (parent / 'memory.max').write_text(str(limit * 1024**2))
                snapshot = self.snapshot()
                self.assertEqual(snapshot['available_mib'], room)
                self.assertTrue(any('cgroup /parent memory.stat unusable; fallback' in note
                                    for note in snapshot['notes']))

    def test_missing_stat_field_keeps_current_room_beside_a_readable_ancestor(self):
        self.write_cache_stats(self.cg / 'parent')
        child = self.cg / 'parent/child'
        self.write_cache_stats(child)
        path = child / 'memory.stat'
        path.write_text(''.join(line + '\n' for line in path.read_text().splitlines()
                                if not line.startswith('shmem ')))
        snapshot = self.snapshot()
        self.assertEqual(snapshot['available_mib'], 304)
        self.assertTrue(any('fallback to memory.current=' in note and 'shmem' in note
                            for note in snapshot['notes']))


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        clean = dict(os.environ, OXPINYIN_LOCK_DIR=self.temp.name, OXPINYIN_HEAVY_SLOTS='1',
                     OXPINYIN_RESOURCE_WAIT_SECONDS='0.12')
        for key in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'OXPINYIN_HEAVY_SLOT_HELD'):
            clean.pop(key, None)
        self.environment = mock.patch.dict(os.environ, clean, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.globals = M['main'].__globals__
        self.readings = mock.patch.dict(self.globals, resource_snapshot=lambda: resources())
        self.readings.start()
        self.addCleanup(self.readings.stop)
        self.out = io.StringIO()

    def admission(self, env=None, poll=0.01):
        admission = M['HeavyAdmission']('tiny child', os.environ if env is None else env,
                                        out=self.out, poll=poll)
        self.addCleanup(admission.close)
        return admission

    def test_explicit_cargo_values_reach_the_child_unchanged(self):
        for value in ('-1', 'default', '3'):
            with self.subTest(value=value):
                with self.admission(dict(os.environ, CARGO_BUILD_JOBS=value)) as admission:
                    result = subprocess.run([sys.executable, '-c',
                                             'import os; print(os.environ["CARGO_BUILD_JOBS"])'],
                                            env=admission.env, capture_output=True, text=True, timeout=3)
                    self.assertEqual(result.returncode, 0)
                    self.assertEqual(result.stdout.strip(), value)

    def test_invalid_explicit_values_stop_with_a_clear_error(self):
        for key, values in [('CARGO_BUILD_JOBS', ('abc', '0', '', '1.5')),
                            ('RUST_TEST_THREADS', ('0', '-1', 'abc', 'default', ''))]:
            for value in values:
                with self.subTest(key=key, value=value), self.assertRaisesRegex(ValueError, 'Invalid ' + key):
                    self.admission(dict(os.environ, **{key: value}))
        result = subprocess.run([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                 sys.executable, '-c', 'print("started")'],
                                env=dict(os.environ, CARGO_BUILD_JOBS='abc'),
                                capture_output=True, text=True, timeout=3)
        self.assertEqual(result.returncode, 1)
        self.assertIn('FAIL: Invalid CARGO_BUILD_JOBS', result.stderr)
        self.assertNotIn('started', result.stdout)

    def test_final_choice_uses_memory_read_after_the_slot_wait(self):
        holder = M['HeavySlot']('holder', out=self.out)
        with holder:
            # The wait limit runs from the first attempt; setUp's 0.12 s would
            # expire on a host that stalls before the second one.
            admission = self.admission(dict(os.environ, OXPINYIN_RESOURCE_WAIT_SECONDS='5'))
            self.assertFalse(admission.try_acquire())
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(3000)):
            self.assertTrue(admission.try_acquire())
        result = subprocess.run([sys.executable, '-c', 'import os; print(os.environ["CARGO_BUILD_JOBS"])'],
                                env=admission.env, capture_output=True, text=True, timeout=3)
        self.assertEqual(result.stdout.strip(), '1')
        self.assertIn('defaults changed after admission', self.out.getvalue())

    def test_zero_room_releases_slot_and_expires_without_a_child(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(M['HEADROOM_MIB'])):
            started = time.monotonic()
            admission = self.admission()
            self.assertFalse(admission.try_acquire())
            with M['HeavySlot']('probe', out=self.out) as slot:
                self.assertLess(slot.waited, 0.01)
            calls = []
            result = M['run_admitted']([dict(result={}, admission=admission)], 2,
                                       lambda job: calls.append(job))
            elapsed = time.monotonic() - started
        self.assertEqual(calls, [])
        self.assertEqual(result[0]['status'], 'FAIL')
        self.assertEqual(result[0]['reason'], 'resource-blocked: 600 MiB needed, 0 MiB available')
        # The admission ran until its limit: elapsed is measured from before
        # the first attempt, which starts the limit, so this bound does not
        # depend on scheduling. The recorded memory wait leaves out each
        # attempt's slot acquisition and resource read, which the limit
        # includes, so no fixed share of the limit is a floor for it: on a
        # stalled host the first attempt alone took 70 ms and the row read
        # 0.05 (CI test on ae5f4d13). What holds is that a memory wait was
        # recorded and fits inside the admission.
        self.assertGreaterEqual(elapsed, admission.limit)
        self.assertGreater(result[0]['memory_wait_seconds'], 0)
        self.assertLessEqual(result[0]['memory_wait_seconds'], elapsed)
        self.assertIn('no process started', self.out.getvalue())

    def test_unknown_memory_releases_slot_and_fails_closed_with_explicit_settings(self):
        for settings in ({}, {'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '1'},
                         {'CARGO_BUILD_JOBS': 'default'}, {'CARGO_BUILD_JOBS': '-1'}):
            clock = [10.0]
            with self.subTest(settings=settings), mock.patch.dict(
                    self.globals, resource_snapshot=lambda: resources(None)), \
                    mock.patch.object(time, 'monotonic', side_effect=lambda: clock[0]):
                started = time.monotonic()
                admission = self.admission(dict(os.environ, **settings))
                self.assertFalse(admission.try_acquire())
                self.assertTrue(admission.env is None, 'the admission kept an environment')
                with M['HeavySlot']('probe', out=self.out) as slot:
                    self.assertLess(slot.waited, 0.01)
                calls = []
                # Establish the unknown-memory decision before advancing past
                # the deadline; scheduler stalls must not choose another obstacle.
                clock[0] += 2 * admission.limit
                result = M['run_admitted']([dict(result={}, admission=admission)], 2,
                                           lambda job: calls.append(job))
                elapsed = time.monotonic() - started
                self.assertEqual(calls, [])
                self.assertEqual(result[0]['status'], 'FAIL')
                self.assertEqual(result[0]['reason'], 'resource-blocked: available memory unknown')
                # Expiry still records a positive memory wait within the elapsed time.
                self.assertGreaterEqual(elapsed, admission.limit)
                self.assertGreater(result[0]['memory_wait_seconds'], 0)
                self.assertLessEqual(result[0]['memory_wait_seconds'], elapsed)
                self.assertIn('no process started', self.out.getvalue())

    def test_unknown_memory_can_recover_after_a_fresh_read(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(None)):
            admission = self.admission()
            self.assertFalse(admission.try_acquire())
        admission.retry_at = 0
        self.assertTrue(admission.try_acquire())
        self.assertEqual(admission.env['CARGO_BUILD_JOBS'], '4')
        self.assertIn('defaults changed after admission', self.out.getvalue())

    def test_an_expired_slot_wait_names_the_slot_and_its_holder_without_memory_figures(self):
        reads = []
        def snapshot():
            reads.append(1)
            return resources()
        with mock.patch.dict(self.globals, resource_snapshot=snapshot):
            with M['HeavySlot']('other lane', out=self.out):
                record = json.loads((Path(self.temp.name) / 'heavy-slot-0').read_text())
                with self.assertRaises(M['ResourceBlocked']) as raised:
                    with self.admission():
                        self.fail('the slot was taken from its holder')
        message = str(raised.exception)
        # The holder kept the slot, so no attempt reached a memory decision: the
        # only reading is the provisional one. No memory figure can be the cause.
        self.assertEqual(len(reads), 1)
        self.assertTrue(message.startswith('slot-wait-expired: '), message)
        self.assertIn(f"slot 0: pid {record['pid']} in {record['worktree']} since {record['since']}: other lane",
                      message)
        self.assertRegex(message, r'waited [0-9.]+s for a heavy slot \(limit 0\.12s\)')
        self.assertNotIn('MiB', message)
        self.assertNotIn('resource-blocked', message)
        self.assertNotIn('memory refused', self.out.getvalue())
        with M['HeavySlot']('probe', out=self.out) as slot:
            self.assertLess(slot.waited, 0.01)

    def test_an_expired_slot_wait_names_every_held_slot(self):
        os.environ['OXPINYIN_HEAVY_SLOTS'] = '2'
        with M['HeavySlot']('lane a', out=self.out), M['HeavySlot']('lane b', out=self.out):
            with self.assertRaises(M['ResourceBlocked']) as raised:
                with self.admission():
                    self.fail('a held slot was taken')
        self.assertRegex(str(raised.exception),
                         r'held by: slot 0: pid [0-9]+ in .+ since [0-9TZ:-]+: lane a; '
                         r'slot 1: pid [0-9]+ in .+ since [0-9TZ:-]+: lane b$')

    def test_the_last_attempt_decides_what_blocked_an_expired_wait(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(M['HEADROOM_MIB'])):
            admission = self.admission(dict(os.environ, OXPINYIN_RESOURCE_WAIT_SECONDS='30'))
            # No room: the first attempt is refused on memory and gives the slot back.
            self.assertFalse(admission.try_acquire())
            with M['HeavySlot']('other lane', out=self.out):
                # Another lane takes it: the next attempt is blocked on the slot.
                admission.retry_at = 0
                self.assertFalse(admission.try_acquire())
                admission.limit = 0
                with self.assertRaises(M['ResourceBlocked']) as raised:
                    admission.try_acquire()
        self.assertTrue(str(raised.exception).startswith('slot-wait-expired: '), raised.exception)
        self.assertNotIn('MiB', str(raised.exception))

    def refused_on_memory(self):
        """Run out the wait limit on a memory decision made beside a neighbour's slot.

        The provisional reading shows room; the decision's does not. 3300 MiB read,
        less the neighbour's 1500 MiB estimate over its 500 MiB use, leaves 2300 MiB;
        the 2048 MiB headroom leaves 252 MiB for a 600 MiB estimate."""
        os.environ['OXPINYIN_HEAVY_SLOTS'] = '2'
        reads = []
        def snapshot():
            reads.append(1)
            return resources(16000 if len(reads) == 1 else 3300)
        with M['HeavySlot']('neighbour', out=self.out) as neighbour:
            record = json.loads(Path(neighbour._file.name).read_text())
            M['write_holder'](neighbour._file, dict(record, estimate_mib=1500))
            with mock.patch.dict(self.globals, resource_snapshot=snapshot), \
                    mock.patch.object(M['_host'], 'process_tree_use_mib', return_value=500):
                admission = self.admission(dict(os.environ, OXPINYIN_RESOURCE_WAIT_SECONDS='0'))
                with self.assertRaises(M['ResourceBlocked']) as raised:
                    admission.try_acquire()
        return raised.exception

    def test_a_memory_refusal_at_expiry_is_resource_blocked_with_its_decisions_figures(self):
        self.assertEqual(str(self.refused_on_memory()),
                         'resource-blocked: 600 MiB needed, 252 MiB available')

    def test_a_memory_refusal_prints_the_available_headroom_reservations_and_estimate_it_used(self):
        self.refused_on_memory()
        self.assertIn('memory refused: available 2300 MiB after 1000 MiB for other slots and 0 MiB for '
                      'containers; headroom 2048 MiB; estimate 600 MiB; no process started: tiny child',
                      self.out.getvalue())

    def test_heavy_rows_behind_a_held_slot_fail_as_slot_waits(self):
        original_read = Path.read_text
        row = next(r for r in json.loads(original_read(RUN_ALL.with_name('runners.json')))
                   if r['name'] == 'pc-version')
        rows = [dict(row, name=name, heavy=heavy, command=catalog_command(row, [name]))
                for name, heavy in [('heavy-a', True), ('heavy-b', True), ('light', False)]]
        finished = []
        def read(path, *args, **kwargs):
            return json.dumps(rows) if path.name == 'runners.json' else original_read(path, *args, **kwargs)
        def execute(command, env, cwd, timeout, output, slot=None):
            finished.append(command[0])
            return 0, 0.0, '', False
        stdout = io.StringIO()
        with M['HeavySlot']('other lane', out=self.out), \
                mock.patch.dict(self.globals, execute=execute), \
                mock.patch.object(Path, 'read_text', read), mock.patch.object(sys, 'argv',
                [str(RUN_ALL), '--runners', 'heavy-a,heavy-b,light', '--cells', 'bdb',
                 '--no-build', '--jobs', '2']), contextlib.redirect_stdout(stdout):
            self.assertEqual(M['main'](), 1)
        table = [line.split(' | ') for line in stdout.getvalue().splitlines() if ' | ' in line][2:]
        self.assertEqual({(fields[0], fields[1], fields[2]) for fields in table},
                         {('heavy-a', 'bdb', 'FAIL'), ('heavy-b', 'bdb', 'FAIL'), ('light', 'bdb', 'PASS')})
        self.assertEqual(finished, ['light'])
        for fields in table:
            if fields[0] != 'light':
                self.assertTrue(fields[-1].startswith('slot-wait-expired: '), fields[-1])
                self.assertNotIn('MiB', fields[-1])

    def test_explicit_parallelism_waits_without_reduction(self):
        for settings, needed in [({'CARGO_BUILD_JOBS': '3'}, 1800),
                                 ({'RUST_TEST_THREADS': '20'}, 1280),
                                 ({'CARGO_BUILD_JOBS': '-1'}, 1800),
                                 ({'CARGO_BUILD_JOBS': 'default'}, 2400)]:
            with self.subTest(settings=settings):
                with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(3000)):
                    admission = self.admission(dict(os.environ, **settings))
                    self.assertFalse(admission.try_acquire())
                    self.assertEqual(admission.estimate, needed)
                with admission:
                    for key, value in settings.items():
                        self.assertEqual(admission.env[key], value)

    def test_minimum_admission_is_one_rustc_plus_headroom(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(M['HEADROOM_MIB'] + M['RUSTC_PEAK_MIB'])):
            with self.admission() as admission:
                self.assertEqual(admission.env['CARGO_BUILD_JOBS'], '1')

    def test_light_row_finishes_while_two_heavy_rows_wait_for_memory(self):
        original_read = Path.read_text
        row = next(r for r in json.loads(original_read(RUN_ALL.with_name('runners.json')))
                   if r['name'] == 'pc-version')
        rows = [dict(row, name=name, heavy=heavy, command=catalog_command(row, [name]))
                for name, heavy in [('heavy-a', True), ('heavy-b', True), ('light', False)]]
        finished = []
        def read(path, *args, **kwargs):
            return json.dumps(rows) if path.name == 'runners.json' else original_read(path, *args, **kwargs)
        def execute(command, env, cwd, timeout, output, slot=None):
            finished.append(command[0])
            return 0, 0.0, '', False
        stdout = io.StringIO()
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(0), execute=execute), \
                mock.patch.object(Path, 'read_text', read), mock.patch.object(sys, 'argv',
                [str(RUN_ALL), '--runners', 'heavy-a,heavy-b,light', '--cells', 'bdb',
                 '--no-build', '--jobs', '2']), contextlib.redirect_stdout(stdout):
            self.assertEqual(M['main'](), 1)
        self.assertEqual(finished, ['light'])
        self.assertIn('light | bdb | PASS', stdout.getvalue())
        self.assertIn('heavy-a | bdb | FAIL', stdout.getvalue())
        self.assertIn('heavy-b | bdb | FAIL', stdout.getvalue())

    def test_wrapper_resource_block_is_nonzero(self):
        code = (f"import runpy; m=runpy.run_path({str(RUN_ALL.resolve())!r}); "
                f"m['run_in_heavy_slot'].__globals__['resource_snapshot']=lambda: {resources(0)!r}; "
                "m['run_in_heavy_slot'](['echo','child-started'])")
        # The child gets its own wait limit rather than setUp's 0.12 s, which a stall inside its first attempt
        # outlasts: it then fails with the mutex expiry error and not the refusal. The refusal comes when the
        # limit runs out, so five seconds would outlast the timeout below; zero is one immediate attempt,
        # which no stall can turn into an expiry.
        result = subprocess.run([sys.executable, '-c', code], env=dict(os.environ, OXPINYIN_RESOURCE_WAIT_SECONDS='0'),
                                capture_output=True, text=True, timeout=3)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('resource-blocked: 600 MiB needed, 0 MiB available', result.stderr)
        self.assertNotIn('child-started', result.stdout)

    def test_invalid_wait_limits_fail(self):
        for value in ('-1', 'nan', 'inf', 'abc'):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, 'Invalid OXPINYIN_RESOURCE_WAIT_SECONDS'):
                self.admission(dict(os.environ, OXPINYIN_RESOURCE_WAIT_SECONDS=value))

    def test_cancellation_releases_the_slot(self):
        admission = self.admission()
        with self.assertRaises(KeyboardInterrupt):
            with admission:
                raise KeyboardInterrupt()
        with M['HeavySlot']('probe', out=self.out) as slot:
            self.assertLess(slot.waited, 0.01)

    def aggregate(self, *, build=False, child=None, executed=None):
        original_read = Path.read_text
        row = next(r for r in json.loads(original_read(RUN_ALL.with_name('runners.json')))
                   if r['name'] == 'pc-version')
        row = dict(row, name='tiny', heavy=True, artifacts=['pinyin-capi'] if build else [],
                   command=catalog_command(row, child or [sys.executable, '-c', 'pass']))
        def read(path, *args, **kwargs):
            return json.dumps([row]) if path.name == 'runners.json' else original_read(path, *args, **kwargs)
        stdout = io.StringIO()
        with mock.patch.object(Path, 'read_text', read), mock.patch.object(sys, 'argv',
                [str(RUN_ALL), '--runners', 'tiny', '--cells', 'bdb', '--jobs', '2', '--allow-skipped', '*']), contextlib.redirect_stdout(stdout):
            if executed is None:
                rc = M['main']()
            else:
                # A stubbed build writes no identity record, so stub #901's identity
                # check as well; cell-identity.test.py covers that check itself.
                with mock.patch.dict(self.globals, execute=executed, identity_of=lambda target: dict(source={}),
                                     check_identity=lambda *args: None):
                    rc = M['main']()
        return rc, stdout.getvalue()

    def test_wrapper_build_and_runner_give_the_same_child_environment(self):
        child = [sys.executable, '-c', 'import json,os; print(json.dumps(dict((k,os.environ[k]) for k in '
                 '["CARGO_BUILD_JOBS","RUST_TEST_THREADS","OXPINYIN_HEAVY_SLOT_HELD"])))']
        seen = []
        original_start = self.globals['start_child']
        def start(command, env, **kwargs):
            seen.append({key: env[key] for key in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS')})
            return original_start([sys.executable, '-c', 'pass'], env, **kwargs)
        with mock.patch.dict(self.globals, start_child=start):
            self.assertEqual(M['run_in_heavy_slot'](child), 0)
        def execute(command, env, cwd, timeout, output, slot=None):
            result = subprocess.run(child, env=env, capture_output=True, text=True, timeout=3)
            self.assertEqual(result.returncode, 0, result.stderr)
            values = json.loads(result.stdout)
            seen.append({key: values[key] for key in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS')})
            return result.returncode, 0.01, '', False
        rc, _ = self.aggregate(build=True, child=child, executed=execute)
        self.assertEqual(rc, 0)
        self.assertEqual(len(seen), 3)
        self.assertEqual(seen, [seen[0]] * 3)
        self.assertIsNone(os.environ.get('CARGO_BUILD_JOBS'))

    def test_blocked_build_and_runner_report_fail_rows(self):
        for build in (False, True):
            with self.subTest(build=build), mock.patch.dict(self.globals, resource_snapshot=lambda: resources(0)):
                execute = mock.Mock(side_effect=AssertionError('A blocked job started.'))
                rc, output = self.aggregate(build=build, executed=execute)
                self.assertEqual(rc, 1)
                self.assertIn('tiny | bdb | FAIL', output)
                self.assertIn(f'resource-blocked: {600 if build else 1024} MiB needed, 0 MiB available', output)
                test_support.assert_not_called(self, execute)

    def failed_build_reason(self):
        """The reason column of the one FAIL row when the cell build's admission refuses."""
        execute = mock.Mock(side_effect=AssertionError('A blocked build started.'))
        rc, output = self.aggregate(build=True, executed=execute)
        rows = [line.split(' | ') for line in output.splitlines() if ' | ' in line][2:]
        self.assertEqual(rc, 1)
        self.assertEqual({(fields[0], fields[1], fields[2]) for fields in rows}, {('tiny', 'bdb', 'FAIL')})
        test_support.assert_not_called(self, execute)
        return rows[0][-1]

    def test_a_cell_build_refused_on_memory_keeps_its_resource_blocked_reason(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(0)):
            reason = self.failed_build_reason()
        self.assertEqual(reason, 'resource-blocked: 600 MiB needed, 0 MiB available')

    def test_a_cell_build_behind_a_held_slot_keeps_its_slot_wait_reason(self):
        with M['HeavySlot']('other lane', out=self.out):
            reason = self.failed_build_reason()
        self.assertTrue(reason.startswith('slot-wait-expired: '), reason)
        self.assertNotIn('cell build:', reason)
        self.assertNotIn('MiB', reason)

    def test_an_oom_killed_child_is_fail_even_when_skips_are_allowed(self):
        child = [sys.executable, '-c', 'import os,signal; os.kill(os.getpid(), signal.SIGKILL)']
        rc, output = self.aggregate(child=child)
        self.assertEqual(rc, 1)
        self.assertIn('tiny | bdb | FAIL', output)
        self.assertIn('exit -9', output)

    def test_unknown_memory_blocks_wrapper_build_and_runner(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(None)):
            with mock.patch.dict(self.globals, start_child=mock.Mock(side_effect=AssertionError('A child started.'))):
                with self.assertRaisesRegex(M['ResourceBlocked'], '^resource-blocked: available memory unknown$'):
                    M['run_in_heavy_slot']([sys.executable, '-c', 'pass'])
            for build in (False, True):
                with self.subTest(build=build):
                    execute = mock.Mock(side_effect=AssertionError('A blocked job started.'))
                    rc, output = self.aggregate(build=build, executed=execute)
                    self.assertEqual(rc, 1)
                    self.assertIn('tiny | bdb | FAIL', output)
                    self.assertIn('resource-blocked: available memory unknown', output)
                    test_support.assert_not_called(self, execute)


class ReviewAdmissionTests(unittest.TestCase):
    setUp = AdmissionTests.setUp
    admission = AdmissionTests.admission
    # Keep these tests in the existing resource harness.
    def test_heavy_runner_requires_its_runner_footprint(self):
        for available in (2648, 2800, 3071):
            with self.subTest(available=available), mock.patch.dict(
                    self.globals, resource_snapshot=lambda: resources(available, 8)):
                out = io.StringIO()
                admission = M['HeavyAdmission']('contract-diff / bdb', os.environ,
                                                 out=out, poll=0.01, runner=True)
                try:
                    self.assertEqual(admission.estimate, M['RUNNER_PEAK_MIB'])
                    self.assertFalse(admission.try_acquire())
                    self.assertTrue(admission.env is None, 'the admission kept an environment')
                    with M['HeavySlot']('probe', out=out):
                        pass
                finally:
                    admission.close()
        child, _, estimate = M['prepare_heavy_env']({}, resources(2648, 8))
        self.assertEqual(estimate, 600)

    def test_two_slots_share_one_atomic_memory_budget(self):
        code = r'''import json,os,runpy,sys
m=runpy.run_path(sys.argv[1])
m['HeavyAdmission'].__init__.__globals__['resource_snapshot']=lambda: dict(
    available_mib=4096,cpus=8,cargo_cpus=8,notes=['synthetic 4096 MiB'])
a=m['HeavyAdmission']('concurrent request',os.environ,poll=0.01)
sys.stdin.readline()
try:
    ready=a.try_acquire()
    print(json.dumps(dict(admitted=ready,estimate=a.estimate)),flush=True)
    sys.stdin.readline()
finally:
    a.close()
'''
        env = dict(os.environ, OXPINYIN_HEAVY_SLOTS='2', CARGO_BUILD_JOBS='3',
                   OXPINYIN_RESOURCE_WAIT_SECONDS='5')
        children = [subprocess.Popen([sys.executable, '-c', code, str(RUN_ALL.resolve())], env=env,
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True) for _ in range(2)]
        try:
            for child in children:
                child.stdin.write('go\n')
                child.stdin.flush()
            records = []
            for child in children:
                self.assertTrue(select.select([child.stdout], [], [], 3)[0])
                records.append(json.loads(child.stdout.readline()))
            self.assertEqual(sum(record['admitted'] for record in records), 1)
            self.assertEqual([record['estimate'] for record in records], [1800, 1800])
            reservations = [json.loads(path.read_text()) for path in Path(self.temp.name).glob('heavy-slot-*')
                            if path.read_text()]
            self.assertEqual(sum(record.get('estimate_mib', 0) for record in reservations), 1800)
        finally:
            for child in children:
                if child.poll() is None:
                    child.stdin.write('stop\n')
                    child.stdin.flush()
                _, stderr = child.communicate(timeout=5)
                self.assertEqual(child.returncode, 0, stderr)

    def test_dead_and_unlocked_holder_records_do_not_reserve_memory(self):
        os.environ['OXPINYIN_HEAVY_SLOTS'] = '2'
        dead = subprocess.Popen([sys.executable, '-c', 'pass'])
        dead.wait()
        with M['HeavySlot']('other slot', out=self.out) as holder:
            record = json.loads(Path(holder._file.name).read_text())
            record.update(pid=dead.pid, estimate_mib=100000)
            M['write_holder'](holder._file, record)
            with self.admission() as admitted:
                self.assertEqual(admitted.env['CARGO_BUILD_JOBS'], '4')
        path = Path(self.temp.name) / 'heavy-slot-1'
        path.write_text(json.dumps(dict(pid=os.getpid(), estimate_mib=100000)))
        with self.admission() as admitted:
            self.assertEqual(admitted.env['CARGO_BUILD_JOBS'], '4')

    def test_reservation_follows_child_after_orchestrator_death(self):
        # The child holds its slot until the test closes its stdin, so a stall
        # on the test side cannot end the reservation being checked.
        code = (f"import os,runpy,sys; m=runpy.run_path({str(RUN_ALL.resolve())!r}); "
                f"m['HeavyAdmission'].__init__.__globals__['resource_snapshot']=lambda: {resources(4096, 8)!r}; "
                "sys.exit(m['run_in_heavy_slot']([sys.executable,'-c',"
                "'import sys; print(12345,flush=True); sys.stdin.read()']))")
        # The parent admits for real, so it gets a wait limit of its own: setUp's 0.12 s, inherited
        # through os.environ, expires on a host that stalls for that long inside its first attempt, and
        # the parent then exits with the admission-wait-expired error before it has started the child.
        env = dict(os.environ, OXPINYIN_HEAVY_SLOTS='2', CARGO_BUILD_JOBS='3', OXPINYIN_RESOURCE_WAIT_SECONDS='5')
        parent = subprocess.Popen([sys.executable, '-c', code], env=env, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertTrue(select.select([parent.stdout], [], [], 3)[0])
            self.assertEqual(parent.stdout.readline().strip(), '12345')
            parent.kill()
            parent.wait(timeout=3)
            with mock.patch.dict(os.environ, OXPINYIN_HEAVY_SLOTS='2'), mock.patch.dict(
                    self.globals, resource_snapshot=lambda: resources(4096, 8)):
                # A refusal raises once the first attempt has outlasted the wait
                # limit; setUp's 0.12 s would expire on a host that stalls here.
                admitted = self.admission(dict(os.environ, CARGO_BUILD_JOBS='3',
                                               OXPINYIN_RESOURCE_WAIT_SECONDS='5'))
                self.assertFalse(admitted.try_acquire())
                self.assertTrue(admitted.env is None, 'the admission kept an environment')
                # The record names the surviving child: its estimate less its use.
                note = re.search(r'slot [0-9]+ pid ([0-9]+): estimate 1800 MiB, '
                                 r'process tree use ([0-9]+) MiB, charge ([0-9]+) MiB', self.out.getvalue())
                self.assertIsNotNone(note, self.out.getvalue())
                pid, use, charge = map(int, note.groups())
                self.assertNotEqual(pid, parent.pid)
                self.assertEqual(charge, max(0, 1800 - use))
                self.assertIn(f'other slot reservations={charge} MiB', self.out.getvalue())
        finally:
            if parent.poll() is None:
                parent.kill()
            parent.stdin.close()  # EOF: the child that outlived the parent exits
            parent.communicate(timeout=5)

    def test_inherited_no_room_fails_without_waiting(self):
        self.inherited_block(0, 'resource-blocked: inherited slot; 600 MiB needed, 0 MiB available')

    def test_inherited_unknown_memory_fails_without_waiting(self):
        self.inherited_block(None, 'resource-blocked: available memory unknown')

    def inherited_block(self, memory, reason):
        code = (f"import os,runpy,sys,time; m=runpy.run_path({str(RUN_ALL.resolve())!r}); "
                f"m['HeavyAdmission'].__init__.__globals__['resource_snapshot']=lambda: {resources(memory)!r}; "
                "start=time.monotonic()\n"
                "try: m['run_in_heavy_slot']([sys.executable,'-c','print(12345)'])\n"
                "finally: print(f'inherited_admission_seconds={time.monotonic()-start}', file=sys.stderr)")
        outer = (f"import os,runpy,sys; m=runpy.run_path({str(RUN_ALL.resolve())!r}); "
                 f"m['HeavyAdmission'].__init__.__globals__['resource_snapshot']=lambda: {resources()!r}; "
                 f"sys.exit(m['run_in_heavy_slot']([sys.executable,'-c',{code!r}]))")
        result = subprocess.run([sys.executable, '-c', outer],
                                env=dict(os.environ, OXPINYIN_RESOURCE_WAIT_SECONDS='20',
                                         CARGO_BUILD_JOBS='1', RUST_TEST_THREADS='1'),
                                capture_output=True, text=True, timeout=6)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(reason, result.stderr)
        self.assertNotIn('waiting for memory', result.stderr)
        self.assertNotIn('12345', result.stdout)
        elapsed = float(re.search(r'inherited_admission_seconds=([0-9.]+)', result.stderr)[1])
        self.assertLess(elapsed, 1)
        with M['HeavySlot']('after outer failure', out=self.out):
            pass


class RunnerBuildRoutingTests(unittest.TestCase):
    def test_residue_probe_build_uses_the_heavy_wrapper(self):
        import ast
        source = ROOT / 'tools/dev/runner_recipes.py'
        tree = ast.parse(source.read_text())
        residue = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'residue')
        commands = [node.args[0] for node in ast.walk(residue)
                    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)
                    and node.func.attr == 'checked' and node.args and isinstance(node.args[0], ast.List)]
        builds = [command for command in commands if any(isinstance(item, ast.Constant) and item.value == 'cargo'
                                                        for item in command.elts)]
        self.assertEqual(len(builds), 1)
        self.assertEqual([item.value for item in builds[0].elts[:5]], ['just', 'heavy', '--', 'cargo', 'build'])
        self.assertIn('nbest_tail_probe', [item.value for item in builds[0].elts if isinstance(item, ast.Constant)])

    def test_runner_cargo_builds_all_use_the_heavy_wrapper(self):
        import ast
        tree = ast.parse((ROOT / 'tools/dev/measure/campaigns.py').read_text())

        def cargo_list(node):
            """A command line that compiles: `cargo build` and its siblings, not `cargo --version`."""
            return (isinstance(node, ast.List) and len(node.elts) > 1
                    and all(isinstance(item, ast.Constant) for item in node.elts[:2])
                    and node.elts[0].value == 'cargo'
                    and node.elts[1].value in ('build', 'test', 'clippy', 'run', 'cinstall', 'capi'))

        def service(call):
            return call.func.attr if isinstance(call.func, ast.Attribute) else None
        routed = {}
        for function in ast.walk(tree):
            if not isinstance(function, ast.FunctionDef):
                continue
            calls = [node for node in ast.walk(function) if isinstance(node, ast.Call)]
            if not any(cargo_list(node) for node in ast.walk(function)):
                continue
            # A cargo command line goes to bench.heavy (one build, holding the slot) or to bench.cargo
            # (a unit of bench.build_all's admitted pool); never straight to bench.run or bench.capture.
            for call in calls:
                if service(call) in ('run', 'capture') and call.args and cargo_list(call.args[0]):
                    self.fail(f'{function.name} runs cargo outside the heavy slot')
            routed[function.name] = sorted({service(call) for call in calls} & {'heavy', 'cargo'})
        self.assertEqual(routed, {'perf_baseline': ['heavy'], 'build_capi': ['heavy'], 'cinstall': ['cargo'],
                                  'cargo_build': ['cargo'], 'build': ['cargo']})


if __name__ == '__main__':
    unittest.main()
