#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the valgrind recipes: the callgrind differential's eight stages and system-import-profile.

The differential runs in the fake world (fake_world.py): its stages, banners, files and stopping
rules are pinned without valgrind, binutils or a PMU, so the file runs on macOS and Linux. The
neutrality comparison it calls is replaced by a fake module (it has its own tests).
"""
import contextlib
import io
from pathlib import Path
import signal
import sys
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import pool, slot
from tools.dev.measure import bench, valgrind
from tools.dev import runners
from tools.dev.measure.fake_world import Stream, World

NM = ('0000000000001130 t bisect_perf_cold_unit\n0000000000001140 t bisect_perf_steady_unit\n'
      '0000000000001150 T main\n')


class ControlTests(World):
    def setUp(self):
        super().setUp()
        self.profile, self.shipping = self.root / 'profile', self.root / 'shipping'
        for stage in (self.profile, self.shipping):
            stage.mkdir()
            (stage / 'libpinyin.so').write_bytes(b'library')

    def control(self):
        return self.execute(valgrind.callgrind_control, [str(self.profile), str(self.shipping)], {})

    def test_dwarf_is_checked_and_the_neutrality_failure_propagates(self):
        with mock.patch.object(valgrind.bench, 'capture', return_value=pool.Completed(
                0, b'  Compilation Unit @ offset 0:\n', b'')) as readelf, mock.patch(
                'tools.dev.measure.neutrality.compare', return_value=(1, 'FAIL (c)\n')) as compare:
            status, out, err = self.control()
        self.assertEqual((status, out, err), (1, 'profiled DWARF compilation units: 1\nFAIL (c)\n', ''))
        readelf.assert_called_once_with(['readelf', '--debug-dump=info', self.profile / 'libpinyin.so'], env={})
        compare.assert_called_once_with(self.profile / 'libpinyin.so', self.shipping / 'libpinyin.so')
        for stage in (self.profile, self.shipping):
            self.assertEqual((stage / 'libpinyin.so').read_bytes(), b'library')

    def test_missing_dwarf_and_failed_readelf_stop_before_comparison(self):
        for code, error in ((0, 'callgrind-control: profiled library has no DWARF compilation units\n'),
                            (7, 'readelf failed\n')):
            with self.subTest(code=code), mock.patch.object(valgrind.bench, 'capture', return_value=pool.Completed(
                    code, b'', b'readelf failed\n' if code else b'')), mock.patch(
                    'tools.dev.measure.neutrality.compare') as compare:
                self.assertEqual(self.control(), (code or 1, '', error))
                compare.assert_not_called()

    def test_bad_arguments_and_either_missing_library_fail(self):
        self.assertEqual(self.execute(valgrind.callgrind_control, [], {})[0], 2)
        for index in range(2):
            stages = [str(self.profile), str(self.shipping)]
            stages[index] = str(self.root / 'missing')
            self.assertEqual(self.execute(valgrind.callgrind_control, stages, {})[0], 1)


class TableTests(World):
    def nm(self, text):
        return lambda command: pool.Completed(0, text.encode(), b'')

    def test_distinct_anchors_pass_and_name_the_toggle(self):
        code, text = valgrind.anchors_table(Path('bisect'), run=self.nm(NM))
        self.assertEqual(code, 0)
        self.assertEqual(text, '=== collection anchors ===\n0000000000001130 t bisect_perf_cold_unit\n'
                               '0000000000001140 t bisect_perf_steady_unit\n'
                               'PASS: distinct addresses, so --toggle-collect=bisect_perf_steady_unit\n'
                               '      excludes process init and the cold cycle\n')

    def test_no_anchor_symbol_at_all_ends_the_group_after_its_header(self):
        self.assertEqual(valgrind.anchors_table(Path('bisect'), run=self.nm('0 T main\n')),
                         (1, '=== collection anchors ===\n'))


class LibrariesTests(World):
    def roots(self, present):
        roots = {}
        for key in ('lp', 'ox', 'nodebug', 'alloc'):
            root = self.root / key
            if present:
                (root / 'lib').mkdir(parents=True)
                if present is True:
                    (root / 'lib/libpinyin.so').write_bytes(b'')
            roots[key] = root
        self.enterContext(mock.patch.object(valgrind, 'ROOTS', roots))
        self.enterContext(mock.patch.object(valgrind, 'DATA', self.root / 'data'))
        (self.root / 'data').mkdir()
        return roots

    def libraries(self):
        err = Stream()
        with contextlib.redirect_stderr(err):
            try:
                return valgrind.libraries(), None, err.text()
            except runners.Finished as stop:
                return None, stop.status, err.text()

    def test_all_four_libraries_are_found_below_their_roots(self):
        roots = self.roots(True)
        found, status, _ = self.libraries()
        self.assertEqual((status, {key: path for key, path in found.items()}),
                         (None, {key: root / 'lib/libpinyin.so' for key, root in roots.items()}))

    def test_a_root_without_the_library_is_a_missing_artifact_with_nothing_to_name(self):
        self.roots('empty')
        _, status, err = self.libraries()
        self.assertEqual((status, err), (1, "fatal: missing artifact: ''\n"))


class AnnotateTests(World):
    """annotate runs its units in the pool."""

    def setUp(self):
        super().setUp()
        self.enterContext(mock.patch.object(slot.CANCELLED, 'is_set', lambda: False))
        self.enterContext(contextlib.redirect_stderr(io.StringIO()))

    def test_each_profile_is_annotated_twice_and_the_redirection_probe_is_skipped(self):
        for name in ('callgrind.libpinyin-tkrzw.r1.out', 'callgrind.redir-probe.out', 'callgrind.oxpinyin-tkrzw.r1.out', 'other.out'):
            (self.root / name).write_bytes(b'')
        calls = []

        def capture(command, merge_stderr=False, **kw):
            calls.append((command, merge_stderr))
            return pool.Completed(0, b'annotated\n', b'')
        with mock.patch.object(bench, 'capture', capture):
            valgrind.annotate(self.root)
        profile = self.root / 'callgrind.libpinyin-tkrzw.r1.out'
        # Independent post-processing on a pool: no order between the units.
        self.assertIn(['callgrind_annotate', '--threshold=99.5', str(profile)], [command for command, _ in calls])
        self.assertIn(['callgrind_annotate', '--threshold=99.5', '--inclusive=yes', str(profile)],
                      [command for command, _ in calls])
        self.assertEqual(len(calls), 4)
        self.assertEqual({merged for _, merged in calls}, {True})
        self.assertEqual((self.root / 'callgrind.libpinyin-tkrzw.r1.out.annotated.txt').read_bytes(), b'annotated\n')
        self.assertTrue((self.root / 'callgrind.oxpinyin-tkrzw.r1.out.inclusive.txt').exists())
        self.assertFalse(list(self.root.glob('*redir-probe*.txt')))

    def test_a_missing_annotate_tool_is_noted_in_the_file_and_is_not_a_failure(self):
        (self.root / 'callgrind.a.r1.out').write_bytes(b'')

        def missing(command, **kw):
            raise FileNotFoundError(2, 'No such file or directory', 'callgrind_annotate')
        with mock.patch.object(bench, 'capture', missing):
            valgrind.annotate(self.root)
        self.assertEqual((self.root / 'callgrind.a.r1.out.annotated.txt').read_text(), 'callgrind_annotate: No such file or directory\n')


class InterceptionTests(World):
    def setUp(self):
        super().setUp()
        self.arm = bench.Arm('oxpinyin-tkrzw', Path('/ox/libpinyin.so'), Path('/data'), Path('/ox'))
        self.worker = bench.Bench('just measure-callgrind-differential', {}, Path('/h/bisect'), '0', cell='tkrzw')

    def probe(self, stderr=b'', profile='', status=0, refuse=False):
        def launch(arm, mode, cycles, **options):
            self.launched = (arm.label, mode, cycles, options)
            if refuse:
                bench.fail('REFUSED: x: subject: libtkrzw resolved to /usr/lib')
            (self.root / 'callgrind.redir-probe.out').write_text(profile)
            return pool.Completed(status, b'row\n', stderr)
        with mock.patch.object(self.worker, 'launch', launch, create=True):
            return valgrind.malloc_interception(self.worker, self.arm, self.root)

    def test_a_failing_probe_does_not_stop_the_capture(self):
        text = self.probe(b'valgrind: error\n', '', status=1)
        self.assertIn('--- objects credited in the profile ---', text)

    def test_a_refusal_inside_the_probe_goes_to_its_stderr_file(self):
        text = self.probe(refuse=True)
        self.assertEqual((self.root / 'redir-probe.stdout').read_bytes(), b'')
        self.assertEqual((self.root / 'redir-probe.stderr').read_text(), 'REFUSED: x: subject: libtkrzw resolved to /usr/lib\n')
        self.assertTrue(text.startswith('=== valgrind malloc interception ===\n'))


class DifferentialTests(World):
    def setUp(self):
        super().setUp()
        self.out = self.root / 'out'
        self.libs = {}
        for key, base in (('lp', 'pin'), ('ox', 'ox'), ('nodebug', 'nodebug'), ('alloc', 'alloc')):
            library = self.root / base / 'lib/libpinyin.so'
            library.parent.mkdir(parents=True)
            library.write_bytes(base.encode())
            self.libs[key] = library
        (self.root / 'data').mkdir()
        self.enterContext(mock.patch.object(valgrind, 'libraries', lambda: dict(self.libs)))
        self.enterContext(mock.patch.object(valgrind, 'DATA', self.root / 'data'))
        self.nm_output = NM
        self.neutral = (0, 'PASS (a): identical .text\n')
        fake = types.SimpleNamespace(compare=lambda first, second: self.neutral)
        self.enterContext(mock.patch.dict(sys.modules, {'tools.dev.measure.neutrality': fake}))
        self.enterContext(mock.patch('tools.dev.measure.neutrality', fake, create=True))
        original = self.fake_capture

        def capture(command, *, env=None, cwd=None, merge_stderr=False):
            if list(command)[:1] == ['nm']:
                return pool.Completed(0, self.nm_output.encode(), b'')
            if '--perf' in [str(part) for part in command] and env.get('PERF_HW') == 'instructions':
                self.log.append(('hardware',))
            return original(command, env=env, cwd=cwd, merge_stderr=merge_stderr)
        self.enterContext(mock.patch.object(bench, 'capture', capture))
        self.enterContext(mock.patch.object(valgrind, 'annotate', lambda out: self.log.append(('annotate', out.name))))
        self.env = {'CG_OUT': str(self.out), 'CG_ROUNDS': '2', 'T_RUNS': '2', 'HW_RUNS': '2', 'PERF_CPU': '1'}
        self.worker_row = {}

    def test_the_eight_stages_run_in_order_and_leave_their_files(self):
        status, out, err = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 0, err)
        stages = [line for line in out.splitlines() if line.startswith(('=== ', '--- '))]
        self.assertEqual(stages, [
            '=== environment ===', '=== collection anchors ===', '=== debug-info neutrality ===',
            '=== valgrind malloc interception ===', '--- redirections naming malloc/free (from --trace-redir) ---',
            '--- objects credited in the profile ---',
            '--- callgrind: libpinyin-tkrzw round 1 ---', '--- callgrind: oxpinyin-tkrzw round 1 ---',
            '--- callgrind: libpinyin-tkrzw round 2 ---', '--- callgrind: oxpinyin-tkrzw round 2 ---',
            '=== allocations per steady cycle (oxpinyin, alloc-count build) ===',
            '--- timing: 2 alternating runs x 8 cycles x 1 passes ---', f'=== captures written to {self.out} ==='])
        for name in ('environment.txt', 'anchors.txt', 'debuginfo-neutrality.txt', 'malloc-interception.txt',
                     'cg.libpinyin-tkrzw.r1.stdout', 'cg.oxpinyin-tkrzw.r2.stderr', 'hw-instructions.jsonl', 'hw-instructions.md',
                     'hw.err', 'alloc-count.txt', 'speed.jsonl', 'speed.err', 'timing.md'):
            self.assertTrue((self.out / name).exists(), name)
        self.assertEqual((self.out / 'debuginfo-neutrality.txt').read_text(), '=== debug-info neutrality ===\nPASS (a): identical .text\n')
        self.assertEqual((self.out / 'timing.md').read_text(), 'REPORT perf-ci.py\n')
        self.assertTrue(out.rstrip().endswith('timing.md'))

    def test_workers_are_pinned_to_perf_cpu_and_profiled_with_the_callgrind_options(self):
        self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual({event[1] for event in self.kinds('taskset')}, {'1'})
        profiled = [w for w in self.workers_of() if 'valgrind' in w[6]]
        # the redirection probe, then two rounds of both cells
        self.assertEqual(len(profiled), 5)
        cell = profiled[1][6]
        self.assertEqual(cell[cell.index('valgrind'):cell.index('valgrind') + 7],
                         ['valgrind', '--tool=callgrind', '--collect-atstart=no', '--toggle-collect=bisect_perf_steady_unit',
                          '--cache-sim=yes', '--branch-sim=yes', '--separate-threads=no'])
        self.assertIn(f'--callgrind-out-file={self.out / "callgrind.libpinyin-tkrzw.r1.out"}', cell)
        self.assertEqual((profiled[1][1], profiled[1][3], profiled[1][4]), ('libpinyin-tkrzw', '4', '1'))
        native = [w for w in self.workers_of() if 'valgrind' not in w[6]]
        # hardware: 2 runs x 2 cells with PERF_HW, the allocation run, then the timing runs
        self.assertEqual([w[1] for w in native], ['libpinyin-tkrzw', 'oxpinyin-tkrzw'] * 2 + ['oxpinyin-alloccount']
                         + ['libpinyin-tkrzw', 'oxpinyin-nodebug'] * 2)
        self.assertEqual({w[3] for w in native}, {'8'})
        self.assertEqual(len(self.kinds('hardware')), 4)
        self.assertEqual([check[2] for check in self.kinds('check')[:4]], ['pin', 'subject', 'subject', 'subject'])

    def test_a_bad_count_is_named_before_anything_is_made(self):
        for name in ('CG_ROUNDS', 'T_RUNS', 'HW_RUNS'):
            with self.subTest(name=name):
                status, out, err = self.execute(valgrind.callgrind_differential, [], {**self.env, name: '1.5'})
                self.assertEqual((status, out, self.out.exists()), (2, '', False))
                self.assertEqual(err, f"just measure-callgrind-differential: {name} must be a decimal count (got '1.5')\n")

    def test_folded_anchors_stop_after_their_table_is_kept(self):
        self.nm_output = NM.replace('1140 t', '1130 t')
        status, out, _ = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 1)
        self.assertIn('FAIL: anchors folded', (self.out / 'anchors.txt').read_text())
        self.assertFalse((self.out / 'debuginfo-neutrality.txt').exists())
        self.assertEqual(self.workers_of(), [])

    def test_different_code_stops_the_run_before_any_profile(self):
        self.neutral = (1, 'FAIL (c): instruction multiset differs\n')
        status, out, _ = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 1)
        self.assertIn('FAIL (c)', out)
        self.assertFalse((self.out / 'malloc-interception.txt').exists())
        self.assertEqual(self.workers_of(), [])

    def test_a_failing_callgrind_run_stops_after_keeping_its_streams(self):
        self.worker_status = {2: 5}
        status, _, _ = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 5)
        self.assertTrue((self.out / 'cg.libpinyin-tkrzw.r1.stderr').exists())
        self.assertFalse((self.out / 'hw-instructions.jsonl').exists())
        self.assertNotIn(('annotate', 'out'), self.log)


class SystemImportProfileTests(World):
    def setUp(self):
        super().setUp()
        self.pin, self.ox, self.data = (self.root / name for name in ('pin.so', 'ox.so', 'data'))
        for path in (self.pin, self.ox):
            path.write_bytes(b'')
        self.data.mkdir()
        self.out = self.root / 'out'
        self.calls = []
        profile = types.SimpleNamespace(main=lambda argv: self.calls.append(argv) or 0)
        self.enterContext(mock.patch.object(bench, 'script', lambda relative: profile))

    def run_recipe(self, *arguments):
        return self.execute(valgrind.system_import_profile, [str(self.pin), str(self.ox), str(self.data), str(self.out), *arguments])

    def test_the_default_workload_is_a_thousand_iterations_and_the_driver_is_compiled_first(self):
        status, out, err = self.run_recipe()
        self.assertEqual(status, 0, err)
        self.assertEqual(self.calls, [['--', str(self.pin.resolve()), str(self.ox.resolve()), str(self.data.resolve()),
                                       str(self.out.resolve()), '1000']])
        compile_command = self.kinds('run')[0][1]
        self.assertEqual(compile_command[:6], ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2'])
        self.assertEqual(compile_command[-3:], ['-ldl', '-o', str(self.out.resolve() / 'driver')])

    def test_missing_operands_are_named_with_the_runner(self):
        for given, name in ((0, 'pin-so'), (1, 'ox-so'), (2, 'system-data-dir')):
            status, _, err = self.execute(valgrind.system_import_profile, [str(self.pin)] * given, {})
            self.assertEqual((status, err), (1, f'system-import-profile: {given + 1}: {name}\n'))

    def test_a_cancelled_collection_ends_with_the_status_of_the_signal(self):
        def cancelled(argv):
            slot.CANCELLED.set()
            slot.CANCELLED_SIGNAL = signal.SIGTERM
            raise SystemExit('cancelled')
        self.addCleanup(slot.CANCELLED.clear)
        self.addCleanup(setattr, slot, 'CANCELLED_SIGNAL', None)
        self.enterContext(mock.patch.object(bench, 'script', lambda relative: types.SimpleNamespace(main=cancelled)))
        status, _, err = self.run_recipe()
        self.assertEqual((status, err), (128 + signal.SIGTERM, 'cancelled\n'))

    def test_any_other_exit_of_the_collections_is_left_alone(self):
        def refused(argv):
            raise SystemExit('pin: failed workload or user-store open')
        self.enterContext(mock.patch.object(bench, 'script', lambda relative: types.SimpleNamespace(main=refused)))
        with self.assertRaises(SystemExit) as stop:
            self.run_recipe()
        self.assertEqual(stop.exception.code, 'pin: failed workload or user-store open')

    def test_an_existing_output_directory_is_refused_before_the_driver_is_built(self):
        self.out.mkdir()
        with self.assertRaises(FileExistsError) as exists:
            self.out.mkdir(parents=True)
        status, _, err = self.run_recipe()
        self.assertEqual(status, 1)
        self.assertEqual(err, f"mkdir: cannot create directory '{self.out}': {exists.exception.strerror}\n")
        self.assertEqual(self.kinds('run'), [])
        self.assertEqual(self.calls, [])

    def test_an_argument_error_comes_before_the_directory_is_made(self):
        status, _, err = self.run_recipe('30', '--bogus')
        self.assertEqual((status, err), (2, 'unknown argument: --bogus\n'))
        self.assertFalse(self.out.exists())


if __name__ == '__main__':
    unittest.main()
