#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Linux compare and profile contracts; synthetic rows and a stub library, not product measurements."""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import pool
from tools.dev.measure import bench, identity, prepare, recipes, reports
from tools.dev.runners import Finished

LIBRARY = '/stage/A/lib/libpinyin.so.15.0.0'


def perf_row(cycles=4, **changes):
    """One bisect --perf row in rss-diag mode with the hardware counter granted."""
    row = dict(pid=17, backend='A', so=LIBRARY, mode='rss-diag', repeats=1,
               workload_checksum='0123456789abcdef',
               cycles_ns=[9000, *[1000] * (cycles - 1)], cycles_cpu_ns=[8000, *[900] * (cycles - 1)],
               rollup_cycle=dict(rss_kib=9000, anonymous_kib=4000, private_dirty_kib=4100),
               steady_hw_cycles=cycles - 1, steady_hw_instructions=700000)
    row.update(changes)
    return json.dumps(row)


def reduced(kind, block, arm, source='perf_event_open', **changes):
    row = reports.linux_observation(perf_row(), cycles=4, library=LIBRARY, source=source)
    row.update(kind=kind, block=block, arm=arm, instruction_source=source, **changes)
    return row


def session(source='perf_event_open', pairs=6, **by_arm):
    """Balanced rows; BY_ARM maps 'comparison_B' style names to per-block changes."""
    rows = []
    for kind in ('comparison', 'control'):
        for block in range(pairs):
            for arm in 'AB':
                change = by_arm.get(f'{kind}_{arm}', lambda block: {})(block)
                rows.append(reduced(kind, block, arm, source, **change))
    return rows


class LinuxRows(unittest.TestCase):
    def test_the_loop_is_the_steady_cycles_and_every_reading_is_required(self):
        row = reports.linux_observation(perf_row(), cycles=4, library=LIBRARY, source='perf_event_open')
        # Cycle 0 is outside both instruction sources, so it is outside the clocks as well.
        self.assertEqual((row['wall_seconds'], row['cpu_seconds']), (3000 / 1e9, 2700 / 1e9))
        self.assertEqual((row['steady_cycles'], row['loop_hw_instructions']), (3, 700000))
        self.assertEqual((row['anonymous_kib'], row['private_dirty_kib']), (4000, 4100))
        broken = dict(
            so='/stage/B/lib/libpinyin.so.15.0.0', pid=0, repeats=2, workload_checksum='bad',
            cycles_ns=[1000] * 3, cycles_cpu_ns=[0] * 4, steady_hw_cycles=2, steady_hw_instructions=0,
            rollup_cycle=dict(anonymous_kib=-1, private_dirty_kib=4100),
            cycles_cpu_ns_missing=None)
        for key, value in broken.items():
            changed = json.loads(perf_row())
            if key.endswith('_missing'):
                del changed[key.removesuffix('_missing')]
            else:
                changed[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                reports.linux_observation(json.dumps(changed), cycles=4, library=LIBRARY, source='perf_event_open')
        with self.assertRaisesRegex(ValueError, 'fingerprint mismatch'):
            reports.linux_observation(perf_row(), cycles=4, library=LIBRARY, source='callgrind', fingerprint='f' * 16)
        with self.assertRaisesRegex(ValueError, 'two cycles'):
            reports.linux_observation(perf_row(1), cycles=1, library=LIBRARY, source='callgrind')

    def test_a_refused_counter_inside_a_hardware_comparison_fails(self):
        refused = json.loads(perf_row())
        del refused['steady_hw_cycles'], refused['steady_hw_instructions']
        refused['hw_errno'] = 2
        with self.assertRaisesRegex(ValueError, 'never mixed'):
            reports.linux_observation(json.dumps(refused), cycles=4, library=LIBRARY, source='perf_event_open')
        # The same row is complete where Callgrind supplies the count, and carries no hardware figure.
        row = reports.linux_observation(json.dumps(refused), cycles=4, library=LIBRARY, source='callgrind')
        self.assertNotIn('loop_hw_instructions', row)
        # A Callgrind worker's clocks and residency ran under Valgrind and are not read.
        worker = reports.linux_observation(json.dumps(refused), cycles=4, library=LIBRARY)
        self.assertEqual(set(worker), {'pid', 'cycles', 'steady_cycles', 'fingerprint'})


class LinuxReport(unittest.TestCase):
    def test_hardware_report_labels_its_source_and_gates_the_control(self):
        rows = session(comparison_B=lambda block: dict(loop_hw_instructions=770000, private_dirty_kib=4132 + block),
                       control_B=lambda block: dict(wall_seconds=3000 / 1e9 * (1.2 if block % 2 else 1.3)))
        summary = reports.linux_comparison(rows, source='perf_event_open')
        self.assertEqual((summary['platform'], summary['instruction_source']), ('linux', 'perf_event_open'))
        self.assertEqual(list(summary['comparison']),
                         ['loop_hw_instructions', 'wall_seconds', 'cpu_seconds', 'anonymous_kib', 'private_dirty_kib'])
        self.assertEqual(set(summary['units']), set(summary['comparison']))
        self.assertIn('perf_event_open', summary['units']['loop_hw_instructions'])
        self.assertEqual(summary['comparison']['loop_hw_instructions']['B_over_A']['median'], 1.1)
        # Residency is a paired difference in KiB, never a ratio.
        residency = summary['comparison']['private_dirty_kib']
        self.assertNotIn('B_over_A', residency)
        self.assertEqual(residency['B_minus_A_kib']['median'], 34.5)
        control = summary['control']
        self.assertEqual(control['loop_hw_instructions']['gate']['rule'], [0.95, 1.05])
        self.assertEqual(control['anonymous_kib']['gate']['rule'], dict(floor_kib=64, fraction=0.01))
        # An unstable control is DIAGNOSTIC; its estimate stays beside the control limits.
        self.assertFalse(control['wall_seconds']['gate']['stable'])
        self.assertIn('B_over_A', summary['comparison']['wall_seconds'])
        self.assertEqual(summary['verdicts'], dict(loop_hw_instructions='VALID', wall_seconds='DIAGNOSTIC',
                                                    cpu_seconds='VALID', anonymous_kib='VALID',
                                                    private_dirty_kib='VALID'))

    def test_residency_control_outside_its_band_is_diagnostic(self):
        rows = session(control_B=lambda block: dict(anonymous_kib=4065))
        summary = reports.linux_comparison(rows, source='perf_event_open')
        self.assertEqual(summary['control']['anonymous_kib']['gate']['bound_kib'], 64)
        self.assertEqual(summary['verdicts']['anonymous_kib'], 'DIAGNOSTIC')
        self.assertEqual(summary['verdicts']['private_dirty_kib'], 'VALID')

    def test_sources_are_never_mixed_and_the_control_is_required(self):
        rows = session()
        rows[3]['instruction_source'] = 'callgrind'
        with self.assertRaisesRegex(ValueError, 'never mixed'):
            reports.linux_comparison(rows, source='perf_event_open')
        with self.assertRaisesRegex(ValueError, 'same-bytes control'):
            reports.linux_comparison([row for row in session() if row['kind'] == 'comparison'],
                                     source='perf_event_open')

    def callgrind(self, counts, tables):
        """COUNTS and TABLES give Ir and per-function Ir by (kind, arm), each a function of the block."""
        rows, functions = [], {}
        for row in session('callgrind', pairs=2):
            key = row['kind'], row['arm']
            row['loop_callgrind_ir'] = counts.get(key, lambda block: 5000)(row['block'])
            functions[row['kind'], row['block'], row['arm']] = tables.get(key, lambda block: {'steady': 5000})(row['block'])
            rows.append(row)
        return reports.linux_comparison(rows, source='callgrind', functions=functions, creators=['callgrind-3.22.0'])

    def test_callgrind_control_is_exact_then_randomstate_then_invalid(self):
        exact = self.callgrind({('comparison', 'B'): lambda block: 5500}, {})
        self.assertEqual(exact['verdicts']['loop_callgrind_ir'], 'VALID')
        self.assertEqual(exact['comparison']['loop_callgrind_ir']['B_over_A']['median'], 1.1)
        self.assertEqual(exact['control']['loop_callgrind_ir']['gate']['spread'], 0)
        self.assertNotIn('loop_hw_instructions', exact['units'])
        self.assertIn('Callgrind', exact['units']['loop_callgrind_ir'])

        # A' differs from A only inside a RandomState scope: ranges and bounds, no point estimate.
        scoped = 'oxpinyin_engine::session::order_window_tail'
        varying = dict(counts={('control', 'B'): lambda block: 5003 + block,
                               ('comparison', 'B'): lambda block: 5500},
                       tables={('control', 'B'): lambda block: {'steady': 4000, scoped: 1003 + block},
                               ('control', 'A'): lambda block: {'steady': 4000, scoped: 1000},
                               ('comparison', 'A'): lambda block: {'steady': 4000, scoped: 1000}})
        diagnostic = self.callgrind(**varying)
        self.assertEqual(diagnostic['verdicts']['loop_callgrind_ir'], 'DIAGNOSTIC')
        entry = diagnostic['comparison']['loop_callgrind_ir']
        self.assertNotIn('B_over_A', entry)
        self.assertEqual((entry['Ir_ranges'], entry['B_minus_A_bounds']),
                         ({'A': [5000, 5000], 'B': [5500, 5500]}, [500, 500]))
        gate = diagnostic['control']['loop_callgrind_ir']['gate']
        self.assertEqual((gate['rule'], gate['varying_functions'], gate['outside_allowlist']),
                         ('randomstate', [scoped], []))

        # The same variation in any other function is INVALID and withholds the Ir comparison only.
        varying['tables'][('control', 'B')] = lambda block: {'steady': 4003 + block, scoped: 1000}
        invalid = self.callgrind(**varying)
        self.assertEqual(invalid['verdicts']['loop_callgrind_ir'], 'INVALID')
        self.assertEqual(set(invalid['comparison']['loop_callgrind_ir']), {'withheld'})
        self.assertEqual(invalid['control']['loop_callgrind_ir']['gate']['outside_allowlist'],
                         [dict(pair='main', function='steady')])
        self.assertIn('B_over_A', invalid['comparison']['wall_seconds'])


class CampaignFixture:
    """A comparison whose guard and driver processes are replaced; shared with the Linux-only cases."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.arms = {}
        for label, name in recipes.PREFIXES.items():
            library = self.root / name / 'lib/libpinyin.so.15.0.0'
            self.arms[label] = dict(label=name, dylib=library, driver=self.root / name / 'bin/bisect',
                                    data=self.root / 'data', backend='tkrzw')
        self.launches = []
        self.refused = True
        self.ir = lambda label: 5000
        self.enterContext(mock.patch.object(bench, 'guard', side_effect=self.guard))
        self.enterContext(mock.patch.object(bench, 'measured_process', side_effect=self.process))
        self.enterContext(mock.patch.object(os, 'sched_getaffinity', return_value={3, 5}, create=True))
        self.guarded = []

    def guard(self, label, cell, side, obj, command, *, env, cwd=None):
        self.guarded.append((cell, str(obj), env['LD_LIBRARY_PATH']))

    def process(self, command, *, env, timeout, usage=True):
        command = list(map(str, command))
        self.launches.append((command, dict(env), usage))
        library = command[command.index('--perf') + 1]
        row = json.loads(perf_row(int(env['PERF_CYCLES']), so=library, backend=env['PERF_BACKEND'], pid=len(self.launches)))
        if 'PERF_HW' not in env or self.refused:
            del row['steady_hw_cycles'], row['steady_hw_instructions']
            if 'PERF_HW' in env:
                row['hw_errno'] = 2
        for part in command:
            if part.startswith('--callgrind-out-file='):
                total = self.ir(env['PERF_BACKEND'])
                Path(part.split('=', 1)[1]).write_text(
                    f'creator: callgrind-3.22.0\npid: {row["pid"]}\ncmd: {command[command.index("--perf") - 1]} --perf x\n'
                    f'events: Ir\nob=(1) {library}\nfn=(1) steady\n0 {total}\nsummary: {total}\ntotals: {total}\n')
        return json.dumps(row).encode(), None

    def compare(self, **options):
        args = types.SimpleNamespace(pairs=2, control_pairs=2, cycles=4, budget=180, **options)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return recipes.linux_compare(args, self.arms, {'LD_LIBRARY_PATH': '/overlay/lib'}, self.root)


class LinuxCampaign(CampaignFixture, unittest.TestCase):
    """The runner around the driver: what it launches, in which order, and what it keeps."""

    def test_a_granted_counter_needs_no_valgrind_and_a_later_refusal_fails(self):
        self.refused = False
        self.assertEqual(self.compare(), 0)
        self.assertEqual(len(self.launches), 9)
        self.assertTrue(all(env['PERF_HW'] == 'instructions' for _, env, _ in self.launches))
        summary = json.loads((self.root / 'summary.json').read_text())
        self.assertEqual(summary['instruction_source'], 'perf_event_open')
        self.assertNotIn('loop_callgrind_ir', summary['comparison'])
        real = self.process

        def refuse_later(command, **options):
            self.refused = len(self.launches) >= 3
            return real(command, **options)
        self.launches.clear()
        with mock.patch.object(bench, 'measured_process', side_effect=refuse_later), \
             self.assertRaisesRegex(ValueError, 'never mixed'):
            self.compare()
        # The completed runs stay; nothing is retried and no summary is written.
        self.assertEqual(len((self.root / 'runs.jsonl').read_text().splitlines()), 2)
        self.assertEqual(len(self.launches), 4)

    def test_the_budget_covers_both_passes(self):
        # Two readings for each run: the schedule's check, then the launcher's after its preflight.
        with mock.patch('time.monotonic', side_effect=[0, *[1] * 16, 181]), \
             self.assertRaisesRegex(ValueError, 'budget'):
            self.compare()
        self.assertEqual(len((self.root / 'runs.jsonl').read_text().splitlines()), 8)
        self.assertFalse((self.root / 'summary.json').exists())

    def test_a_slow_preflight_shortens_the_worker_bound_and_cannot_launch_past_the_deadline(self):
        clock, timeouts = [0.0], []
        real_guard, real_process = self.guard, self.process

        def slow_guard(*arguments, **options):
            # The probe is check 1. Check 3 takes 30 seconds; check 4 outlasts the 180-second budget.
            clock[0] += {3: 30, 4: 200}.get(len(self.guarded) + 1, 0)
            return real_guard(*arguments, **options)

        def process(command, **options):
            timeouts.append(options['timeout'])
            # A worker that starts in time and succeeds is kept, however long it then runs.
            clock[0] += 5
            return real_process(command, **options)
        with mock.patch('time.monotonic', side_effect=lambda: clock[0]), \
             mock.patch.object(bench, 'guard', side_effect=slow_guard), \
             mock.patch.object(bench, 'measured_process', side_effect=process), \
             self.assertRaisesRegex(ValueError, 'measurement budget exhausted; campaign incomplete'):
            self.compare()
        # The probe and two runs were launched; the third was checked and never started.
        self.assertEqual((len(self.guarded), len(self.launches)), (4, 3))
        # The budget starts after the probe. Run 1 has all of it; run 2 loses run 1's 5 seconds and its own 30-second check.
        self.assertEqual(timeouts, [180, 180, 145])
        self.assertEqual(len((self.root / 'runs.jsonl').read_text().splitlines()), 2)
        self.assertFalse((self.root / 'summary.json').exists())

    def profile(self, template):
        args = types.SimpleNamespace(template=template, cycles=4, budget=60)
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            status = recipes.linux_profile(args, self.arms['A'], {}, self.root)
        return status, json.loads(out.getvalue()), self.launches[-1][0]

    def test_profiles_use_the_existing_callgrind_and_massif_recipes(self):
        from tools.dev.measure import valgrind
        annotated = []
        real = self.process

        def massif(command, **options):
            for part in map(str, command):
                if part.startswith('--massif-out-file='):
                    Path(part.split('=', 1)[1]).write_text('mem_heap_B=10\nmem_heap_B=4096\n')
            return real(command, **options)
        with mock.patch.object(valgrind, 'annotate', side_effect=annotated.append), \
             mock.patch.object(bench, 'measured_process', side_effect=massif):
            status, report, command = self.profile('Callgrind')
            self.assertEqual(status, 0)
            self.assertEqual(command[3:3 + len(valgrind.CALLGRIND_OPTIONS)], list(valgrind.CALLGRIND_OPTIONS))
            self.assertEqual(annotated, [self.root])
            self.assertEqual(report['capture']['subject_Ir'], 5000)
            status, report, command = self.profile('Massif')
            self.assertEqual(command[3:8], ['valgrind', '--tool=massif', '--time-unit=i', '--heap=yes', '--stacks=no'])
            self.assertEqual(command.count('--massif-out-file=' + str(self.root / 'massif.out')), 1)
            self.assertEqual(report['capture']['heap'], ['A peak heap: 4096 bytes'])
        # A profile that credits nothing to the measured library is not an attribution.
        self.ir = lambda label: 0
        with mock.patch.object(valgrind, 'annotate'), self.assertRaises(ValueError):
            self.profile('Callgrind')


class PreparationFixture:
    def native(self, argv, env=None, **patches):
        with mock.patch.object(sys, 'platform', 'linux'), contextlib.redirect_stdout(io.StringIO()), \
             contextlib.redirect_stderr(io.StringIO()):
            return recipes.native(argv, env or {})


class LinuxPreparation(PreparationFixture, unittest.TestCase):

    def test_modes_linux_does_not_have_fail_before_any_build(self):
        for argv in (['neutrality'], ['profile', '--template', 'Allocations'],
                     ['profile', '--template', 'Massif', '--instrument', 'VM Tracker'],
                     ['compare', '--base', 'x', '--head', 'x', '--cycles', '1']):
            with self.subTest(argv=argv), mock.patch.object(prepare, 'native_build') as build, \
                 self.assertRaises(Finished) as stopped:
                self.native(argv)
            self.assertEqual(stopped.exception.status, 2)
            build.assert_not_called()
        # The Valgrind templates belong to Linux alone.
        with mock.patch.object(sys, 'platform', 'darwin'), mock.patch('platform.machine', return_value='arm64'), \
             mock.patch.object(prepare, 'native_build') as build, contextlib.redirect_stderr(io.StringIO()), \
             self.assertRaises(Finished):
            recipes.native(['profile', '--template', 'Callgrind'], {})
        build.assert_not_called()

    def test_the_build_is_the_packaged_default_backend_from_the_overlay(self):
        from tools.dev import cell, vcpkg
        metadata = {'packages': [{'name': 'oxpinyin-capi', 'features': {'default': ['declared']}}]}
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(bench, 'run'), \
             mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=json.dumps(metadata).encode())), \
             mock.patch('tools.dev.packaging.install_library') as install, \
             mock.patch.object(identity, 'sha256', return_value='digest'), \
             mock.patch.object(cell, 'declared_default', return_value='tkrzw'), \
             mock.patch.object(vcpkg, 'triplet', return_value='x64-linux-oxpinyin'), \
             mock.patch.object(identity, 'library_backend', return_value='tkrzw') as backend:
            env = dict(VCPKG_ROOT=directory, RUSTFLAGS='bad', CARGO_PROFILE_RELEASE_DEBUG='2', OXPINYIN_DSYM='1')
            installed = Path(directory).resolve() / 'installed/x64-linux-oxpinyin'
            with self.assertRaisesRegex(ValueError, 'tkrzw:x64-linux-oxpinyin'):
                prepare.native_build('a' * 40, Path(directory) / 'none', env, linux=True, symbols=False)
            install.assert_not_called()
            (installed / 'lib/pkgconfig').mkdir(parents=True)
            (installed / 'lib/pkgconfig/tkrzw.pc').touch()
            arm = prepare.native_build('a' * 40, Path(directory) / 'plain', env, linux=True, symbols=False)
            arguments, tools = install.call_args.args
            self.assertEqual(arguments[2:], ['--', '--locked', '--release', '--features', 'shipped'])
            for name in ('RUSTFLAGS', 'CARGO_PROFILE_RELEASE_DEBUG', 'OXPINYIN_DSYM'):
                self.assertNotIn(name, tools.env)
            self.assertTrue(tools.env['PKG_CONFIG_PATH'].startswith(str(installed / 'lib/pkgconfig')))
            self.assertEqual(tools.env['OXPINYIN_TKRZW_PREFIX'], str(installed))
            self.assertEqual((arm['dylib'].name, arm['uuid'], arm['backend'], arm['linux']),
                             ('libpinyin.so.15.0.0', 'digest', 'tkrzw', True))
            prepare.native_build('a' * 40, Path(directory) / 'profiled', env, linux=True)
            self.assertEqual(install.call_args.args[1].env['RUSTFLAGS'], '-Cdebuginfo=1 -Cstrip=none')
            self.assertNotIn('OXPINYIN_DSYM', install.call_args.args[1].env)
            backend.return_value = 'bdb'
            with self.assertRaises(Finished):
                prepare.native_build('a' * 40, Path(directory) / 'other', env, linux=True)

    def test_launch_timeout_bounds_the_worker(self):
        worker = bench.Bench('just measure', {}, Path('/stage/bisect'), '0', required=True)
        arm = bench.Arm('A', Path(LIBRARY), Path('/data'), Path(LIBRARY).parent)
        with mock.patch.object(bench, 'guard'), \
             mock.patch.object(bench, 'measured_process', return_value=(b'{}', None)) as bounded, \
             mock.patch.object(bench, 'capture', return_value=pool.Completed(0, b'[]', b'')) as unbounded:
            self.assertEqual(worker.launch(arm, 'speed', 2, timeout=7).stdout, b'{}')
            self.assertEqual((bounded.call_args.kwargs['timeout'], bounded.call_args.kwargs['usage']), (7, False))
            unbounded.assert_not_called()
            self.assertEqual(worker.launch(arm, 'speed', 2).stdout, b'[]')
            unbounded.reset_mock()
            # A deadline replaces the bound with the time left after the check; with none left nothing is launched.
            with mock.patch('time.monotonic', return_value=100):
                worker.launch(arm, 'speed', 2, deadline=103)
                self.assertEqual(bounded.call_args.kwargs['timeout'], 3)
                launched = bounded.call_count
                with self.assertRaises(subprocess.TimeoutExpired):
                    worker.launch(arm, 'speed', 2, deadline=100)
            self.assertEqual(bounded.call_count, launched)
            unbounded.assert_not_called()


if sys.platform == 'linux':
    STUB = '''#include <stdbool.h>
#include <stddef.h>
#include <string.h>
static bool failed;
static volatile unsigned long spin;
void *pinyin_init(const char *data, const char *user) { (void)user; failed = !strcmp(data, "fail"); return failed ? NULL : (void *)1; }
void pinyin_fini(void *p) { (void)p; }
void *pinyin_alloc_instance(void *p) { return p; }
void pinyin_free_instance(void *p) { (void)p; }
bool pinyin_set_options(void *p, unsigned flags) { (void)p; (void)flags; return true; }
size_t pinyin_parse_more_full_pinyins(void *p, const char *s) { (void)p; for (int i = 0; i < 4000; i++) spin += i; return strlen(s); }
bool pinyin_guess_candidates(void *p, size_t n, unsigned f) { (void)p; (void)n; (void)f; return true; }
bool pinyin_get_n_candidate(void *p, unsigned *n) { (void)p; *n = 1; return true; }
bool pinyin_reset(void *p) { (void)p; return true; }
'''

    class LinuxDriver(unittest.TestCase):
        """The real driver, taskset and Valgrind against a stub library: no tkrzw, no product code."""

        @classmethod
        def setUpClass(cls):
            cls.scratch = tempfile.TemporaryDirectory()
            cls.root = Path(cls.scratch.name).resolve()
            (cls.root / 'stub.c').write_text(STUB)
            cls.arms = {}
            driver = cls.root / 'bisect'
            digest = prepare.native_driver(dict(linux=True), driver, dict(os.environ))
            built = cls.root / 'build/libpinyin.so.15.0.0'
            built.parent.mkdir()
            subprocess.run(['cc', '-shared', '-fPIC', '-O1', '-g', cls.root / 'stub.c', '-o', built],
                           check=True, capture_output=True)
            for label, name in recipes.PREFIXES.items():
                arm = recipes.stage_arm(cls.root / name, dict(dylib=built, linux=True, backend='tkrzw'), driver, digest)
                cls.arms[label] = dict(arm, label=name, data=cls.root)
            cls.env = dict(os.environ, TMPDIR=str(cls.root))

        @classmethod
        def tearDownClass(cls):
            cls.scratch.cleanup()

        def test_perf_row_carries_pid_cpu_time_and_one_counter_outcome(self):
            arm = self.arms['A']
            done = subprocess.run([arm['driver'], '--perf', arm['dylib'], self.root],
                                  env=dict(self.env, PERF_MODE='rss-diag', PERF_CYCLES='6', PERF_HW='instructions',
                                           PERF_WORKLOAD_CHECKSUM='1'), capture_output=True, timeout=60)
            self.assertEqual(done.returncode, 0, done.stderr)
            row = json.loads(done.stdout)
            self.assertGreater(row['pid'], 1)
            self.assertEqual((len(row['cycles_ns']), len(row['cycles_cpu_ns'])), (6, 6))
            self.assertGreater(sum(row['cycles_cpu_ns'][1:]), 0)
            # The host grants the hardware counter or the driver reports its refusal; never both, never neither.
            self.assertEqual(('steady_hw_instructions' in row) + ('hw_errno' in row), 1)
            source = 'perf_event_open' if 'steady_hw_instructions' in row else 'callgrind'
            reduced = reports.linux_observation(done.stdout, cycles=6, library=arm['dylib'], source=source)
            self.assertGreater(reduced['private_dirty_kib'], 0)
            # A failed workload leaves no row.
            failed = subprocess.run([arm['driver'], '--perf', arm['dylib'], 'fail'], env=self.env,
                                    capture_output=True, timeout=60)
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(failed.stdout, b'')

        def test_comparison_and_profiles_run_end_to_end_on_the_stub(self):
            # Never skipped: a Linux host without these tools has not run the comparison.
            missing = [tool for tool in ('valgrind', 'taskset') if not shutil.which(tool)]
            self.assertFalse(missing, 'the end-to-end comparison needs ' + ' and '.join(missing)
                             + ' on PATH; install Valgrind and util-linux')
            args = types.SimpleNamespace(pairs=2, control_pairs=2, cycles=4, budget=600)
            output = self.root / 'session'
            output.mkdir()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                status = recipes.linux_compare(args, self.arms, self.env, output)
            summary = json.loads((output / 'summary.json').read_text())
            source = summary['instruction_source']
            runs = [json.loads(line) for line in (output / 'runs.jsonl').read_text().splitlines()]
            self.assertEqual(len(runs), 16 if source == 'callgrind' else 8)
            self.assertEqual(len({run['fingerprint'] for run in runs}), 1)
            self.assertEqual({run['instruction_source'] for run in runs}, {source})
            if source == 'callgrind':
                # The stub has no hash containers: identical bytes at same-length paths count identically.
                gate = summary['control']['loop_callgrind_ir']['gate']
                self.assertEqual((status, gate['verdict'], gate['spread']), (0, 'VALID', 0), gate)
                self.assertGreater(gate['counts']['A'][0], 3 * 123 * 4000)
            for template, name in (('Callgrind', 'callgrind.out'), ('Massif', 'massif.out')):
                out = io.StringIO()
                profile = self.root / template
                profile.mkdir()
                with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(recipes.linux_profile(types.SimpleNamespace(template=template, cycles=3, budget=600),
                                                           self.arms['A'], self.env, profile), 0)
                self.assertTrue((profile / name).is_file())
                capture = json.loads(out.getvalue())['capture']
                if template == 'Callgrind':
                    self.assertGreater(capture['subject_Ir'], 0)
                    self.assertTrue((profile / 'callgrind.out.inclusive.txt').is_file())
                else:
                    self.assertRegex(capture['heap'][0], r'^A peak heap: [1-9][0-9]* bytes$')

    # These read Callgrind profiles by their Linux paths and select the host's overlay triplet.
    class LinuxCallgrindCampaign(CampaignFixture, unittest.TestCase):
        """The Callgrind pass of the runner, which parses a profile written for Linux paths."""

        def test_a_refused_counter_selects_callgrind_for_the_whole_comparison(self):
            self.assertEqual(self.compare(), 0)
            commands = [command for command, _, _ in self.launches]
            # One unmeasured probe, the native schedule, then the same schedule under Callgrind.
            self.assertEqual([('valgrind' in command) for command in commands], [False] * 9 + [True] * 8)
            probe, native, profiled = self.launches[0], self.launches[1], self.launches[9]
            self.assertEqual((probe[1]['PERF_HW'], probe[1]['PERF_CYCLES'], probe[1]['PERF_MODE']), ('instructions', '2', 'speed'))
            self.assertNotIn('PERF_HW', native[1])
            self.assertEqual((native[1]['PERF_MODE'], native[1]['PERF_WORKLOAD_CHECKSUM'], native[2]), ('rss-diag', '1', False))
            # The existing launcher: pinned to the first allowed CPU, the arm's directory first on the loader path.
            self.assertEqual(native[0][:3], ['taskset', '-c', '3'])
            self.assertEqual(native[1]['LD_LIBRARY_PATH'], str(self.arms['A']['dylib'].parent) + ':/overlay/lib')
            self.assertIn(('tkrzw', str(self.arms['A']['dylib']), native[1]['LD_LIBRARY_PATH']), self.guarded)
            self.assertEqual(profiled[0][3:10], ['valgrind', '--tool=callgrind', '--collect-atstart=no',
                                                 '--toggle-collect=bisect_perf_steady_unit', '--cache-sim=no',
                                                 '--branch-sim=no', '--separate-threads=no'])
            self.assertEqual(profiled[1]['PERF_MODE'], 'speed')
            # Both passes walk the shared schedule; the control's B label is A' from its own prefix.
            order = [env['PERF_BACKEND'] for _, env, _ in self.launches[1:9]]
            self.assertEqual(order, ['A', 'B', 'A', 'C', 'B', 'A', 'C', 'A'])
            self.assertEqual(order, [env['PERF_BACKEND'] for _, env, _ in self.launches[9:]])
            runs = [json.loads(line) for line in (self.root / 'runs.jsonl').read_text().splitlines()]
            self.assertEqual([run['phase'] for run in runs], ['native'] * 8 + ['callgrind'] * 8)
            summary = json.loads((self.root / 'summary.json').read_text())
            self.assertEqual((summary['instruction_source'], summary['verdicts']['loop_callgrind_ir']), ('callgrind', 'VALID'))
            self.assertNotIn('loop_hw_instructions', summary['comparison'])

        def test_an_invalid_instruction_control_keeps_its_record_and_fails(self):
            self.ir = lambda label: 5000 + (label == 'C')
            self.assertEqual(self.compare(), 3)
            summary = json.loads((self.root / 'summary.json').read_text())
            self.assertEqual(summary['verdicts']['loop_callgrind_ir'], 'INVALID')


    class LinuxStaging(PreparationFixture, unittest.TestCase):
        """The native recipe up to staging, which selects the Linux overlay triplet of this host."""

        def test_one_plain_build_and_three_identical_prefixes_of_one_length(self):
            from tools.dev import contract_data
            for mode, outcome in (('compare', 0), ('compare', ValueError('sample failed')), ('profile', 0)):
                with self.subTest(mode=mode, outcome=outcome), tempfile.TemporaryDirectory() as directory:
                    destinations, staged, environments = [], {}, []

                    def build(sha, destination, env, **options):
                        destinations.append((destination, options))
                        library = destination / 'prefix/lib/libpinyin.so.15.0.0'
                        library.parent.mkdir(parents=True)
                        library.write_bytes(b'product')
                        return dict(revision=sha, dylib=library, uuid='hash', linux=True, backend='tkrzw')

                    def driver(arm, output, env):
                        output.write_bytes(b'driver')
                        return identity.sha256(output)

                    def measure(args, arms, env, output):
                        arms = arms if isinstance(arms, dict) and 'A' in arms else {'A': arms}
                        staged.update({label: (arm['dylib'], arm['dylib'].read_bytes(), arm['driver'], arm['label'])
                                       for label, arm in arms.items()})
                        environments.append(env)
                        if isinstance(outcome, Exception):
                            raise outcome
                        return outcome
                    with mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'a' * 40 + b'\n')), \
                         mock.patch.object(identity, 'first_output_line', return_value='tool 1.0'), \
                         mock.patch.object(prepare, 'native_build', side_effect=build) as builds, \
                         mock.patch.object(prepare, 'native_driver', side_effect=driver), \
                         mock.patch.object(contract_data, 'data_identity', return_value='data'), \
                         mock.patch.object(recipes, 'linux_compare', side_effect=measure), \
                         mock.patch.object(recipes, 'linux_profile', side_effect=measure):
                        argv = ([mode, '--base', 'HEAD', '--head', 'HEAD'] if mode == 'compare'
                                else [mode, '--template', 'Massif']) + ['--data', directory]
                        env = dict(OXPINYIN_CACHE=directory, VCPKG_ROOT=directory + '/vcpkg')
                        if isinstance(outcome, Exception):
                            with self.assertRaises(ValueError):
                                self.native(argv, env)
                        else:
                            self.assertEqual(self.native(argv, env), 0)
                    builds.assert_called_once()
                    # A comparison measures the plain shipping build; only a profile adds debug information.
                    self.assertEqual(destinations[0][1], dict(linux=True, symbols=mode == 'profile'))
                    self.assertEqual(set(staged), {'A', 'B', recipes.CONTROL} if mode == 'compare' else {'A'})
                    self.assertEqual({content for _, content, _, _ in staged.values()}, {b'product'})
                    self.assertEqual(len({path for path, _, _, _ in staged.values()}), len(staged))
                    self.assertEqual({len(str(path)) for path, _, _, _ in staged.values()}, {len(str(staged['A'][0]))})
                    self.assertEqual([label for _, _, _, label in staged.values()],
                                     [recipes.PREFIXES[label] for label in staged])
                    for path, _, executable, _ in staged.values():
                        self.assertEqual(executable, path.parents[1] / 'bin/bisect')
                    # Every launch reads the overlay install; the build tree is removed either way.
                    installed = Path(directory).resolve() / 'vcpkg/installed'
                    self.assertTrue(environments[0]['OXPINYIN_TKRZW_PREFIX'].startswith(str(installed)))
                    self.assertTrue(environments[0]['LD_LIBRARY_PATH'].startswith(str(installed)))
                    self.assertFalse(destinations[0][0].parent.exists())
                    self.assertTrue(destinations[0][0].parent.parent.is_dir(), 'retain session output')


if __name__ == '__main__':
    unittest.main()
