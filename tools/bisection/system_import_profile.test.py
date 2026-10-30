"""System-import collection scheduling, retries and runtime checks."""
import importlib.util
import io
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev import pool, test_support
from tools.dev.measure import valgrind
spec = importlib.util.spec_from_file_location('system_import_profile', ROOT / 'tools/bisection/system_import_profile.py')
PROFILE = importlib.util.module_from_spec(spec)
spec.loader.exec_module(PROFILE)


class FakeAdmission:
    """HeavyAdmission stand-in: records its lifecycle and admits a fixed worker count."""
    workers = 2
    events = None

    def __init__(self, label, env, out=None, runner=False, prepare_heavy_env=None):
        self.env = {pool.POOL_WORKERS: str(self.workers)}
        self.slot = type('Slot', (), {'pass_fds': staticmethod(lambda: ()), 'handoff': staticmethod(lambda: None)})()
        self.events.append('admission')

    def try_acquire(self):
        return True

    def close(self):
        self.events.append('close')


class SystemImportProfileTests(unittest.TestCase):
    """The independent collections: order, retries, validation, runtime check; no Valgrind, /proc or CLOCK_BOOTTIME.

    The pool's own rules (admission bound, stop on failure, reaping, cancellation) are tested in
    tools/dev/pool.test.py; here the collections go through that pool with a fake admission.
    """
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='system-import-profile-test-')
        self.work = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        self.events, self.lock = [], threading.Lock()
        FakeAdmission.events, FakeAdmission.workers = self.events, 2
        # The host readers a collection consults: the boot-minus-monotonic clock offset (Linux only)
        # and /proc/loadavg. Neither exists on macOS.
        self.offsets = lambda: 0
        for patcher in (mock.patch('tools.dev.slot.HeavyAdmission', FakeAdmission),
                        mock.patch.object(PROFILE, 'suspend_offset', lambda: self.offsets()),
                        mock.patch.object(PROFILE, 'load_average', lambda: '0.25')):
            patcher.start()
            self.addCleanup(patcher.stop)
        PROFILE.slot.CANCELLED.clear()
        self.addCleanup(PROFILE.slot.CANCELLED.clear)
        self.plan = dict(pin=Path('/pin.so'), ox=Path('/ox.so'), data=Path('/data'), work=self.work, workload='30')

    def emitted(self, jobs):
        lines = []
        PROFILE.run_collections(self.plan, jobs, lambda side, mode, report: lines.append((side, mode, report)))
        return lines

    def test_jobs_two_caps_concurrent_collections_despite_larger_admission(self):
        FakeAdmission.workers = 6
        active, peak, completed = 0, 0, []
        together = threading.Event()

        def collect(side, mode, plan, environment, context):
            nonlocal active, peak
            with self.lock:
                active += 1
                peak = max(peak, active)
                if active == 2:
                    together.set()
            self.assertTrue(together.wait(5), 'two collections did not overlap')
            time.sleep(.02)
            with self.lock:
                active -= 1
                completed.append((side, mode))
            return [f'{side} {mode}']

        with mock.patch.object(PROFILE, 'collect', collect):
            status, output, checks, _ = self.run_main(self.good_outcomes(), '--jobs', '2')
        order = [(side, mode) for side in PROFILE.SIDES for mode in PROFILE.MODES]
        self.assertEqual(status, 0)
        self.assertEqual(peak, 2)
        self.assertCountEqual(completed, order)
        self.assertEqual(output.splitlines(), [f'{side} {mode}' for side, mode in order])
        self.assertEqual([side for _, _, side, _ in checks], ['pin', 'ox'])

    def test_every_collection_gets_the_same_captured_environment_in_both_modes(self):
        runs = {}

        def collect(side, mode, plan, environment, context):
            runs.setdefault(self.mode, []).append(environment)
            return []
        with mock.patch.object(PROFILE, 'collect', collect), mock.patch.dict(os.environ, {'PROBE': 'x'}):
            for self.mode, jobs in (('serial', 1), ('pooled', None)):
                self.emitted(jobs)
            for environments in runs.values():
                self.assertEqual(len(environments), 6)
                for environment in environments:
                    test_support.assert_same_environment(self, environment, dict(os.environ))

    def launcher(self, outcomes):
        """Record each launch and return the next outcome for its side and mode."""
        calls = []

        def launch(context, argv, env=None, **kwargs):
            user = Path(argv[-2])
            side, mode = user.name.split('-user-')[0].split('-')
            calls.append((side, mode, user.name, threading.get_ident()))
            queued = outcomes[(side, mode)]
            status, out, err, create = queued.pop(0) if len(queued) > 1 else queued[0]
            if create:
                user.mkdir(parents=True, exist_ok=True)
                (user / 'user.conf').write_text('')
            return pool.Completed(status, out, err)
        return launch, calls

    GOOD = {'callgrind': (b'summary: 4\nsummary: 5\nsummary: 6\nsummary: 7\n'),
            'heap': b'mem_heap_B=10\nmem_heap_B=30\nmem_heap_B=20\n', 'pages': b'mem_heap_B=7\n'}

    def good_outcomes(self):
        return {(side, mode): [(0, b'', self.GOOD[mode], True)] for side in PROFILE.SIDES for mode in PROFILE.MODES}

    def collect(self, outcomes, side='pin', mode='callgrind'):
        launch, calls = self.launcher(outcomes)
        with mock.patch.object(pool.UnitContext, 'launch', launch):
            return PROFILE.collect(side, mode, self.plan, {}, pool.UnitContext('x', threading.Event())), calls

    def test_users_are_private_per_side_mode_and_attempt_and_retries_follow_their_own_discard(self):
        FakeAdmission.workers = 6
        launch, calls = self.launcher(self.good_outcomes())
        # Only pin heap overlaps a suspend, once: the offset jumps during its first attempt.
        local = threading.local()
        real_collect = PROFILE.collect

        def collect(side, mode, plan, environment, context):
            local.offsets = iter([0, 5_000_000, 5_000_000, 5_000_000]) if (side, mode) == ('pin', 'heap') else None
            return real_collect(side, mode, plan, environment, context)
        self.offsets = lambda: next(local.offsets) if local.offsets else 0
        with mock.patch.object(pool.UnitContext, 'launch', launch), mock.patch.object(PROFILE, 'collect', collect):
            reports = self.emitted(None)
        names = [name for _, _, name, _ in calls]
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual(sorted(names), sorted(
            [f'{side}-{mode}-user-1' for side in PROFILE.SIDES for mode in PROFILE.MODES] + ['pin-heap-user-2']))
        heap = [(name, thread) for side, mode, name, thread in calls if (side, mode) == ('pin', 'heap')]
        self.assertEqual([name for name, _ in heap], ['pin-heap-user-1', 'pin-heap-user-2'])
        self.assertEqual(heap[0][1], heap[1][1])
        self.assertTrue(reports[1][2][0].startswith('DISCARD pin heap: suspend_delta_ns='))
        self.assertTrue(reports[1][2][1].startswith('pin heap: utc='))
        self.assertEqual(reports[1][2][-1], 'pin peak heap: 30 bytes')

    def test_six_suspend_overlapping_attempts_end_the_run(self):
        values = iter([0, 9_000_000] * 6)
        self.offsets = values.__next__
        with self.assertRaises(PROFILE.Failure) as raised:
            self.collect(self.good_outcomes())
        self.assertEqual(raised.exception.message, 'repeated suspend-overlapping runs')
        self.assertEqual(len(raised.exception.lines), 6)

    def test_output_validation_is_strict(self):
        cases = {
            'status': ((1, b'', self.GOOD['callgrind'], True), 'pin callgrind: failed workload or user-store open'),
            'stdout': ((0, b'x', self.GOOD['callgrind'], True), 'pin callgrind: failed workload or user-store open'),
            'marker': ((0, b'', b'no-user-state\n' + self.GOOD['callgrind'], True),
                       'pin callgrind: failed workload or user-store open'),
            'user.conf': ((0, b'', self.GOOD['callgrind'], False), 'pin callgrind: failed workload or user-store open'),
            'dumps': ((0, b'', b'summary: 4\n', True), 'pin: expected 4 Callgrind dumps, got 1'),
        }
        for name, (outcome, message) in cases.items():
            with self.subTest(name):
                shutil.rmtree(self.work / 'pin-callgrind-user-1', ignore_errors=True)
                outcomes = self.good_outcomes()
                outcomes[('pin', 'callgrind')] = [outcome]
                with self.assertRaises(PROFILE.Failure) as raised:
                    self.collect(outcomes)
                self.assertEqual(raised.exception.message, message)
        self.assertEqual(PROFILE.report('pin', 'callgrind', '--cold-open', b'summary: 9\n'),
                         ['pin cold-open: 9 instructions', 'pin total: 9 instructions'])
        with self.assertRaisesRegex(PROFILE.Failure, 'missing Massif heap snapshots'):
            PROFILE.report('ox', 'pages', '30', b'')

    def run_main(self, outcomes, *arguments, runtime_check=None, workload='30'):
        launch, calls = self.launcher(outcomes)
        checks = []
        runtime = runtime_check or (lambda cell, runner, side, obj, **kw: checks.append((cell, runner, side, str(obj))))
        output = io.StringIO()
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': runtime}), \
                mock.patch('sys.stdout', output):
            status = PROFILE.main([*arguments, '--', '/pin.so', '/ox.so', '/data', str(self.work), workload])
        return status, output.getvalue(), checks, calls

    def test_main_prints_every_side_and_mode_in_the_serial_order_and_checks_both_runtimes_first(self):
        status, output, checks, calls = self.run_main(self.good_outcomes(), '--jobs', '1')
        self.assertEqual(status, 0)
        labels = [line.split(':')[0] for line in output.splitlines()]
        self.assertEqual(labels, ['pin callgrind', 'pin open', 'pin import', 'pin save', 'pin close', 'pin total',
                                  'pin heap', 'pin peak heap', 'pin pages', 'pin peak pages',
                                  'ox callgrind', 'ox open', 'ox import', 'ox save', 'ox close', 'ox total',
                                  'ox heap', 'ox peak heap', 'ox pages', 'ox peak pages'])
        self.assertEqual(checks, [('auto', 'run-system-import-profile.sh', 'pin', '/pin.so'),
                                  ('auto', 'run-system-import-profile.sh', 'ox', '/ox.so')])
        self.assertEqual([name for _, _, name, _ in calls], [f'{side}-{mode}-user-1' for side in PROFILE.SIDES
                                                           for mode in PROFILE.MODES])

    def test_cold_open_is_a_workload_not_an_option(self):
        outcomes = self.good_outcomes()
        outcomes[('pin', 'callgrind')] = [(0, b'', b'summary: 9\n', True)]
        outcomes[('ox', 'callgrind')] = [(0, b'', b'summary: 8\n', True)]
        _, output, _, _ = self.run_main(outcomes, '--jobs', '1', workload='--cold-open')
        self.assertIn('pin cold-open: 9 instructions', output)
        self.assertIn('ox total: 8 instructions', output)

    def test_pooled_main_prints_the_same_lines_as_serial_apart_from_the_run_time_fields(self):
        FakeAdmission.workers = 6

        def normal(text):
            return re.sub(r'utc=\S+ load1=\S+ suspend_delta_ns=-?\d+', 'RUN', text)
        serial = self.run_main(self.good_outcomes(), '--jobs', '1')[1]
        pooled = self.run_main(self.good_outcomes())[1]
        self.assertEqual(normal(serial), normal(pooled))
        self.assertEqual(len(pooled.splitlines()), 20)

    def test_an_unsound_runtime_stops_before_any_collection_starts(self):
        def refuse(cell, runner, side, obj, **kwargs):
            if side == 'ox':
                raise RuntimeError('runtime refused')
        launch, calls = self.launcher(self.good_outcomes())
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': refuse}):
            with self.assertRaisesRegex(RuntimeError, 'runtime refused'):
                PROFILE.main(['--', '/pin.so', '/ox.so', '/data', str(self.work), '30'])
        self.assertEqual(calls, [])

    def test_a_failure_prints_the_finished_lines_then_stderr_then_exits_with_its_message(self):
        outcomes = self.good_outcomes()
        outcomes[('pin', 'heap')] = [(1, b'', b'valgrind said no', True)]
        launch, _ = self.launcher(outcomes)
        out, err = io.StringIO(), io.BytesIO()
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': lambda *a, **k: None}), \
                mock.patch('sys.stdout', out), mock.patch.object(PROFILE.sys, 'stderr', io.TextIOWrapper(err)) as text:
            with self.assertRaises(SystemExit) as raised:
                PROFILE.main(['--jobs', '1', '--', '/pin.so', '/ox.so', '/data', str(self.work), '30'])
            text.flush()
        self.assertEqual(raised.exception.code, 'pin heap: failed workload or user-store open')
        self.assertIn('pin total:', out.getvalue())
        self.assertNotIn('ox ', out.getvalue())
        self.assertEqual(err.getvalue(), b'valgrind said no')

    def test_a_pooled_failure_reports_the_earliest_collection_and_prints_those_before_it(self):
        FakeAdmission.workers = 6
        outcomes = self.good_outcomes()
        outcomes[('pin', 'pages')] = [(1, b'', b'pin pages broke', True)]
        outcomes[('ox', 'heap')] = [(1, b'', b'ox heap broke', True)]
        launch, _ = self.launcher(outcomes)
        out = io.StringIO()
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': lambda *a, **k: None}), \
                mock.patch('sys.stdout', out), mock.patch.object(PROFILE.sys, 'stderr', io.TextIOWrapper(io.BytesIO())):
            with self.assertRaises(SystemExit) as raised:
                PROFILE.main(['--', '/pin.so', '/ox.so', '/data', str(self.work), '30'])
        self.assertEqual(raised.exception.code, 'pin pages: failed workload or user-store open')
        self.assertIn('pin peak heap:', out.getvalue())
        self.assertNotIn('ox ', out.getvalue())
        self.assertEqual(self.events[-1], 'close')




if __name__ == '__main__':
    unittest.main()
