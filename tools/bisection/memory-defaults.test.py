#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
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

RUN_ALL = Path(__file__).with_name('run-all.py')
run_all = runpy.run_path(str(RUN_ALL))
M = run_all


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
        with mock.patch.object(sys, 'platform', 'linux'), mock.patch.object(os, 'sched_getaffinity', return_value=set(range(8))):
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
        self.assertEqual(snapshot['available_mib'], 2416)
        chosen = M['memory_defaults']({'OXPINYIN_COMPAT_CONTAINER': '1'}, None,
                                     cpus=snapshot['cpus'],
                                     available_mib=snapshot['available_mib'],
                                     memory_limit_mib=snapshot['memory_limit_mib'])
        self.assertEqual(chosen['CARGO_BUILD_JOBS'][0], 4)
        self.assertEqual(chosen['--jobs'][0], 2)

    def test_mixed_stat_and_current_levels_take_the_smallest_room(self):
        self.write_cache_stats(self.cg / 'parent/child')
        parent = self.cg / 'parent'
        (parent / 'memory.current').write_text(str(1000 * 1024**2))
        for limit, room in ((4000, 2416), (2048, 1048)):
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
            admission = self.admission()
            self.assertFalse(admission.try_acquire())
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(3000)):
            self.assertTrue(admission.try_acquire())
        result = subprocess.run([sys.executable, '-c', 'import os; print(os.environ["CARGO_BUILD_JOBS"])'],
                                env=admission.env, capture_output=True, text=True, timeout=3)
        self.assertEqual(result.stdout.strip(), '1')
        self.assertIn('defaults changed after admission', self.out.getvalue())

    def test_zero_room_releases_slot_and_expires_without_a_child(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(M['HEADROOM_MIB'])):
            admission = self.admission()
            self.assertFalse(admission.try_acquire())
            with M['HeavySlot']('probe', out=self.out) as slot:
                self.assertLess(slot.waited, 0.01)
            calls = []
            result = M['run_admitted']([dict(result={}, admission=admission)], 2,
                                       lambda job: calls.append(job))
        self.assertEqual(calls, [])
        self.assertEqual(result[0]['status'], 'FAIL')
        self.assertEqual(result[0]['reason'], 'resource-blocked: 600 MiB needed, 0 MiB available')
        self.assertGreaterEqual(result[0]['memory_wait_seconds'], 0.1)
        self.assertIn('no process started', self.out.getvalue())

    def test_unknown_memory_releases_slot_and_fails_closed_with_explicit_settings(self):
        for settings in ({}, {'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '1'},
                         {'CARGO_BUILD_JOBS': 'default'}, {'CARGO_BUILD_JOBS': '-1'}):
            with self.subTest(settings=settings), mock.patch.dict(
                    self.globals, resource_snapshot=lambda: resources(None)):
                admission = self.admission(dict(os.environ, **settings))
                self.assertFalse(admission.try_acquire())
                self.assertIsNone(admission.env)
                with M['HeavySlot']('probe', out=self.out) as slot:
                    self.assertLess(slot.waited, 0.01)
                calls = []
                result = M['run_admitted']([dict(result={}, admission=admission)], 2,
                                           lambda job: calls.append(job))
                self.assertEqual(calls, [])
                self.assertEqual(result[0]['status'], 'FAIL')
                self.assertEqual(result[0]['reason'], 'resource-blocked: available memory unknown')
                self.assertGreaterEqual(result[0]['memory_wait_seconds'], 0.1)
                self.assertIn('no process started', self.out.getvalue())

    def test_unknown_memory_can_recover_after_a_fresh_read(self):
        with mock.patch.dict(self.globals, resource_snapshot=lambda: resources(None)):
            admission = self.admission()
            self.assertFalse(admission.try_acquire())
        admission.retry_at = 0
        self.assertTrue(admission.try_acquire())
        self.assertEqual(admission.env['CARGO_BUILD_JOBS'], '4')
        self.assertIn('defaults changed after admission', self.out.getvalue())

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
        rows = [dict(row, name=name, heavy=heavy, command=[name])
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
        result = subprocess.run([sys.executable, '-c', code], env=os.environ,
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
                   command=child or [sys.executable, '-c', 'pass'])
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
        self.assertNotIn('CARGO_BUILD_JOBS', os.environ)

    def test_blocked_build_and_runner_report_fail_rows(self):
        for build in (False, True):
            with self.subTest(build=build), mock.patch.dict(self.globals, resource_snapshot=lambda: resources(0)):
                execute = mock.Mock(side_effect=AssertionError('A blocked job started.'))
                rc, output = self.aggregate(build=build, executed=execute)
                self.assertEqual(rc, 1)
                self.assertIn('tiny | bdb | FAIL', output)
                self.assertIn(f'resource-blocked: {600 if build else 1024} MiB needed, 0 MiB available', output)
                execute.assert_not_called()

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
                    execute.assert_not_called()


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
                    self.assertIsNone(admission.env)
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
        code = (f"import os,runpy,sys; m=runpy.run_path({str(RUN_ALL.resolve())!r}); "
                f"m['HeavyAdmission'].__init__.__globals__['resource_snapshot']=lambda: {resources(4096, 8)!r}; "
                "sys.exit(m['run_in_heavy_slot']([sys.executable,'-c',"
                "'import time; print(12345,flush=True); time.sleep(1.2)']))")
        env = dict(os.environ, OXPINYIN_HEAVY_SLOTS='2', CARGO_BUILD_JOBS='3')
        parent = subprocess.Popen([sys.executable, '-c', code], env=env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertTrue(select.select([parent.stdout], [], [], 3)[0])
            self.assertEqual(parent.stdout.readline().strip(), '12345')
            parent.kill()
            parent.wait(timeout=3)
            with mock.patch.dict(os.environ, OXPINYIN_HEAVY_SLOTS='2'), mock.patch.dict(
                    self.globals, resource_snapshot=lambda: resources(4096, 8)):
                admitted = self.admission(dict(os.environ, CARGO_BUILD_JOBS='3'))
                self.assertFalse(admitted.try_acquire())
                self.assertIsNone(admitted.env)
                self.assertIn('other slot reservations=1800 MiB', self.out.getvalue())
        finally:
            if parent.poll() is None:
                parent.kill()
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
        text = RUN_ALL.with_name('run-residue-a-tail-diff.sh').read_text()
        condition = next(line for line in text.splitlines() if 'if [[ "${RESIDUE_A_PROBE:-}"' in line)
        # Select only the conditional build command. Do not run the driver.
        lines = text.splitlines()
        start = next(i for i, line in enumerate(lines) if '--heavy-slot -- cargo build ' in line)
        build = []
        for line in lines[start:]:
            build.append(line)
            if not line.rstrip().endswith('\\'):
                break
        script = ('record_wrapper() { printf "%s\\n" "$@"; }; HEAVY_RUNNER=record_wrapper; '
                  'CAPI_FEATURE=bdb; REPO_ROOT=/synthetic;\n' + condition + '\n' + '\n'.join(build) + '\nfi')
        for enabled in ('0', '1'):
            with self.subTest(enabled=enabled):
                result = subprocess.run(['bash', '-c', script], env=dict(os.environ, RESIDUE_A_PROBE=enabled),
                                        capture_output=True, text=True, timeout=3)
                self.assertEqual(result.returncode, 0, result.stderr)
                if enabled == '1':
                    self.assertEqual(result.stdout.splitlines()[:4], ['--heavy-slot', '--', 'cargo', 'build'])
                    self.assertIn('nbest_tail_probe', result.stdout)
                else:
                    self.assertEqual(result.stdout, '')

    def test_runner_cargo_builds_all_use_the_heavy_wrapper(self):
        scripts = sorted(RUN_ALL.parent.glob('run-*.sh')) + [RUN_ALL.with_name('oracle-cell.sh')]
        routed = []
        for path in scripts:
            text = path.read_text()
            self.assertIsNone(re.search(r'^\s*cargo\s+(?:build|test|clippy|run|cinstall|capi)\b', text, re.M), path.name)
            if re.search(r'--heavy-slot -- cargo (?:build|cinstall) ', text):
                routed.append(path.name)
                checked = subprocess.run(['bash', '-n', str(path)], capture_output=True, text=True, timeout=3)
                self.assertEqual(checked.returncode, 0, checked.stderr)
        self.assertEqual(sorted(routed), ['oracle-cell.sh', 'run-perf-baseline.sh', 'run-perf-same-data.sh',
                                           'run-residue-a-tail-diff.sh', 'run-shipped-check.sh',
                                           'run-tree-recipe-control.sh'])


if __name__ == '__main__':
    unittest.main()
