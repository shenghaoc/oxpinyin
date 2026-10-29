#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the round-robin campaigns: perf-baseline, perf-matrix, perf-same-data, shipped-check,
tree-recipe-control and rss-diag.

Each recipe runs in a world without tools. The seams bench.py gives them (capture, run, heavy,
the function-order guard, the libtkrzw check, the report scripts and the pool of builds) are
replaced by fakes that write an ordered event log, so a test pins what the shell runners did:
their counts and banners, the order of arms and modes, the files they leave, and the statuses
they stop with. Nothing needs taskset, binutils, cargo or /proc, so the file runs on macOS and Linux.
"""
import contextlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import pool, runners
from tools.dev.measure import bench, campaigns, identity, valgrind
from tools.dev.measure.fake_world import Stream, World

SHA_X = 'bf8d3b5707b5bc1bafc09670185b96770b73f596dd2eb57b8a9cba46bad4aec8'


class SteadyComparisonTests(unittest.TestCase):
    def test_sample_refuses_failed_missing_and_incomplete_windows(self):
        good = dict(cycles_ns=[90, 10, 12], workload_checksum='0' * 16)
        done = lambda row: pool.Completed(0, (json.dumps(row) + '\n').encode(), b'')
        self.assertEqual(campaigns.steady_sample(done(good), 3), good)
        for row in ({}, dict(good, cycles_ns=[90, 10]), dict(good, cycles_ns=[90, 0, 12]),
                    dict(good, workload_checksum='missing')):
            with self.subTest(row=row), self.assertRaises(ValueError):
                campaigns.steady_sample(done(row), 3)
        with self.assertRaises(runners.Finished) as raised:
            campaigns.steady_sample(pool.Completed(7, b'', b''), 3)
        self.assertEqual(raised.exception.args, (7,))

    def test_summary_keeps_control_spread_and_instruction_ranges(self):
        times = dict(base=[10, 12], pr__=[8, 10], pin_=[20, 22], bctl=[11, 13], pctl=[9, 11])
        ir = dict(base=[100, 103], pr__=[90, 94], pin_=[200, 201], bctl=[101, 104], pctl=[91, 95])
        result = campaigns.steady_summary(times, ir, dict(ball=[dict(count=10, bytes=100)],
                                                         pall=[dict(count=9, bytes=80)]))
        self.assertEqual(result['wall_ns_per_cycle']['pr__']['median'], 9)
        self.assertEqual(result['same_bytes_wall_delta_ns'], dict(base=1, pr__=1))
        self.assertEqual(result['ir_per_cycle']['base'], [100, 103])
        self.assertEqual(result['base_minus_pr_ir_bounds'], [6, 13])
        self.assertEqual(result['allocations_per_cycle']['pall'], dict(count=[9, 9], bytes=[80, 80]))

    def campaign(self, *, checksum_change=False, missing_alloc=False):
        from tools.packaging import function_order
        from tools.bisection import teardown
        from tools.dev.measure import native
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths = {name: root / (name + '.so') for name in ('base', 'pr', 'pin', 'base-alloc', 'pr-alloc')}
            for path in paths.values():
                path.write_bytes(b'object')
            events = []
            worker = mock.Mock()

            def launch(arm, mode, cycles, *, repeats, prefix, extra):
                events.append((arm.label, bool(prefix)))
                value = '1' * 16 if checksum_change and arm.label == 'pr__' else '0' * 16
                row = dict(cycles_ns=[999] + [10] * (cycles - 1), workload_checksum=value)
                if arm.label in ('ball', 'pall') and not missing_alloc:
                    row.update(steady_alloc_cycles=cycles - 1, steady_alloc_count=70, steady_alloc_bytes=700)
                if prefix:
                    output = next(item.split('=', 1)[1] for item in prefix
                                  if item.startswith('--callgrind-out-file='))
                    Path(output).write_text('profile')
                return pool.Completed(0, (json.dumps(row) + '\n').encode(), b'')

            worker.launch.side_effect = launch
            argv = ['--cell', 'bdb', '--system-dir', str(root), '--out', str(root / 'out')]
            for name, path in paths.items():
                argv += ['--' + name, str(path)]
            with contextlib.ExitStack() as stack:
                stack.enter_context(mock.patch.object(campaigns.sys, 'platform', 'linux'))
                stack.enter_context(mock.patch.object(campaigns.os, 'sched_getaffinity', return_value={0}, create=True))
                stack.enter_context(mock.patch.object(function_order, 'library_backend', return_value='bdb'))
                stack.enter_context(mock.patch.object(bench, 'check_function_order'))
                stack.enter_context(mock.patch.object(native, 'check_filesystems'))
                stack.enter_context(mock.patch.object(bench, 'run'))
                stack.enter_context(mock.patch.object(campaigns, 'measured', return_value=worker))
                stack.enter_context(mock.patch.object(teardown, 'callgrind_costs', return_value=dict(totals=dict(Ir=700))))
                stack.enter_context(mock.patch.object(campaigns, 'emit'))
                status = campaigns.steady_comparison_inner(argv, {})
            return status, events, json.loads((root / 'out/result.json').read_text())

    def test_campaign_rotates_all_arms_and_excludes_warmup(self):
        status, events, result = self.campaign()
        self.assertEqual(status, 0)
        self.assertEqual([name for name, _ in events[:10]],
                         ['base', 'pr__', 'pin_', 'bctl', 'pctl', 'pr__', 'pin_', 'bctl', 'pctl', 'base'])
        self.assertEqual(len(events), 100 + 6 + 10)
        self.assertEqual(result['wall_ns_per_cycle']['base']['median'], 10)
        self.assertEqual(result['ir_per_cycle']['base'], [100, 100])
        self.assertEqual(result['allocations_per_cycle']['ball']['count'], [10, 10])

    def test_campaign_refuses_changed_answers_and_missing_counters(self):
        with self.assertRaisesRegex(ValueError, 'checksums differ'):
            self.campaign(checksum_change=True)
        with self.assertRaisesRegex(ValueError, 'allocation counters unavailable'):
            self.campaign(missing_alloc=True)

    def test_recipe_holds_admission_for_the_whole_comparison(self):
        admitted = mock.Mock(env={'admitted': 'yes'})
        with mock.patch.object(campaigns.slot, 'HeavyAdmission', return_value=contextlib.nullcontext(admitted)) as heavy:
            with mock.patch.object(campaigns, 'steady_comparison_inner', return_value=0) as inner:
                self.assertEqual(campaigns.steady_comparison(['--cell', 'bdb'], {'source': 'yes'}), 0)
        heavy.assert_called_once_with('steady comparison', {'source': 'yes'}, runner=True)
        inner.assert_called_once_with(['--cell', 'bdb'], {'admitted': 'yes'})


class CountTests(World):
    """Each runner reads its loop counts as plain decimals, before it does any work."""

    VARIABLES = (
        ('perf_baseline', campaigns.perf_baseline, 'run-perf-baseline.sh', ('PERF_RUNS', 'PERF_RAM_RUNS')),
        ('perf_matrix', campaigns.perf_matrix, 'run-perf-matrix.sh', ('PERF_RUNS', 'PERF_RAM_RUNS')),
        ('perf_same_data', campaigns.perf_same_data, 'run-perf-same-data.sh', ('PERF_RUNS', 'PERF_RAM_RUNS')),
        ('shipped_check', campaigns.shipped_check, 'run-shipped-check.sh', ('CTRL_RUNS',)),
        ('tree_recipe_control', campaigns.tree_recipe_control, 'run-tree-recipe-control.sh', ('CTRL_RUNS',)),
        ('callgrind', valgrind.callgrind_differential, 'run-callgrind-differential.sh', ('CG_ROUNDS', 'T_RUNS', 'HW_RUNS')))
    ACCEPTED = ('8', '08', '010', '0')
    REFUSED = ('2+3', '-1')

    def environment(self):
        return {'HOME': str(self.root), 'PINYIN_ORACLE_PREFIX': str(self.root / 'absent'), 'MATRIX_OUT': str(self.root / 'o'),
                'OXPINYIN_PERF_WORK': str(self.root / 'w'), 'CTRL_OUT': str(self.root / 'c'), 'CG_OUT': str(self.root / 'g'),
                'OXPINYIN_KC_SO': str(self.root / 'absent-kc.so'), 'OXPINYIN_TKRZW_SO': str(self.root / 'absent-tkrzw.so')}

    def test_a_count_that_is_not_a_plain_decimal_exits_2_naming_it(self):
        for name, recipe, label, variables in self.VARIABLES:
            for variable in variables:
                for value in self.REFUSED:
                    with self.subTest(runner=label, variable=variable, value=value):
                        self.log.clear()
                        status, out, err = self.execute(recipe, [], {**self.environment(), variable: value})
                        self.assertEqual((status, out), (2, ''))
                        self.assertEqual(err, f"{label}: {variable} must be a decimal count (got '{value}')\n")
                        self.assertEqual(self.log, [])

    def test_a_decimal_count_passes_to_the_next_step(self):
        for name, recipe, label, variables in self.VARIABLES:
            for variable in variables:
                for value in (*self.ACCEPTED, ''):
                    with self.subTest(runner=label, variable=variable, value=value):
                        status, out, err = self.execute(recipe, [], {**self.environment(), variable: value})
                        self.assertNotIn('must be a decimal count', err)
                        self.assertEqual(status, 1, err)


class BaselineTests(World):
    def setUp(self):
        super().setUp()
        self.oracle = self.root / 'oracle'
        (self.oracle / 'lib/libpinyin/data').mkdir(parents=True)
        (self.oracle / 'lib/libpinyin.so').write_bytes(b'')
        (self.oracle / 'oracle-pin.txt').write_text(f'pin_ref={campaigns.ORACLE_PIN}\n')
        self.work = self.root / 'work'
        self.env = {'PINYIN_ORACLE_PREFIX': str(self.oracle), 'OXPINYIN_PERF_WORK': str(self.work),
                    'PERF_RUNS': '2', 'PERF_RAM_RUNS': '1', 'HOME': str(self.root)}

    def stage(self):
        stage = self.work / 'stage/usr'
        (stage / 'lib/pkgconfig').mkdir(parents=True, exist_ok=True)
        (stage / 'lib/pkgconfig/libpinyin.pc').write_text('')
        (stage / 'lib/libpinyin.so').write_bytes(b'')
        (stage / 'share/oxpinyin').mkdir(parents=True, exist_ok=True)
        for name in campaigns.REQUIRED_DATA:
            (stage / 'share/oxpinyin' / name).write_bytes(b'')

    def test_a_full_run_stages_checks_then_alternates_oracle_and_oxpinyin(self):
        status, out, err = self.execute(campaigns.perf_baseline, ['--unordered-arm', 'ox'], self.env)
        self.assertEqual(status, 0, err)
        heavy = self.kinds('heavy')
        self.assertEqual(heavy[0][1][:7], ['cargo', 'cinstall', '--locked', '--release', '-p', 'oxpinyin-capi',
                                           f'--destdir={self.work}/stage'])
        self.assertEqual([event[0] for event in self.log if event[0] in ('run', 'heavy', 'order', 'check')][:4],
                         ['run', 'heavy', 'order', 'check'])
        self.assertEqual([(w[1], w[2]) for w in self.workers_of()],
                         [('oracle', 'speed'), ('oxpinyin', 'speed')] * 2
                         + [('oracle', 'ram-init'), ('oxpinyin', 'ram-init'), ('oracle', 'ram-cycle'), ('oxpinyin', 'ram-cycle')])
        self.assertEqual(self.kinds('order')[0][1:], (['ox/pinyin', 'pin/pinyin'], ['--unordered-arm', 'ox'], ['pin']))
        self.assert_guarded_before_work()
        self.assertEqual(out.splitlines()[0:2], ['--- building bisect harness ---', 'build: ok'])
        self.assertIn(f'--- cargo cinstall oxpinyin-capi -> {self.work}/stage ---', out)
        self.assertIn('pkg-config oxpinyin cflags: -I/stage/usr/include ', out)
        self.assertIn('--- speed: 2 alternating runs x 8 cycles, CPU 0 ---', out)
        self.assertIn('--- RAM: 1 runs per mode per backend, CPU 0 ---', out)
        self.assertTrue(out.rstrip().endswith(f'reports: {self.work}/{{speed,ram,size}}-report.md'))
        for name in ('speed.jsonl', 'ram-init.jsonl', 'ram-cycle.jsonl', 'oracle-speed.err', 'oxpinyin-ram-cycle.err'):
            self.assertTrue((self.work / name).exists(), name)
        self.assertEqual((self.work / 'speed-report.md').read_text(), 'REPORT perf-baseline.py\n')
        self.assertEqual([r[1][0] for r in self.reports], ['summarize-speed', 'summarize-ram', 'size'])

    def test_the_guard_sees_the_exports_the_script_had_made_by_then(self):
        self.execute(campaigns.perf_baseline, ['--unordered-arm', 'ox'], self.env)
        self.assertEqual({name: self.guard_env.get(name) for name in ('PINYIN_ORACLE_PREFIX', 'PKG_CONFIG_SYSROOT_DIR')},
                         {'PINYIN_ORACLE_PREFIX': str(self.oracle), 'PKG_CONFIG_SYSROOT_DIR': f'{self.work}/stage'})
        self.assertTrue(self.guard_env['PKG_CONFIG_PATH'].startswith(f'{self.work}/stage/usr/lib/pkgconfig'))

    def test_workers_get_the_libdir_first_and_no_repeats(self):
        self.execute(campaigns.perf_baseline, ['--unordered-arm', 'ox'], {**self.env, 'LD_LIBRARY_PATH': '/inherited'})
        oracle, oxpinyin = self.workers_of('speed')[:2]
        self.assertEqual(oracle[5], f'{self.oracle}/lib:/inherited')
        self.assertEqual(oxpinyin[5], f'{self.work}/stage/usr/lib:/inherited')
        self.assertIsNone(oracle[4])
        self.assertEqual(oracle[6][-2:], [str(self.oracle / 'lib/libpinyin.so'), str(self.oracle / 'lib/libpinyin/data')])

    def test_loop_counts_are_decimal_and_empty_means_the_default(self):
        for runs, ram_runs, speed, ram in (('08', '010', 8, 10), ('0', '0', 0, 0), ('', '', 20, 10)):
            with self.subTest(runs=runs):
                self.log.clear()
                self.stage()
                status, _, err = self.execute(campaigns.perf_baseline, [], {
                    **self.env, 'PERF_RUNS': runs, 'PERF_RAM_RUNS': ram_runs, 'PERF_REUSE_STAGE': '1'})
                self.assertEqual(status, 0, err)
                self.assertEqual(len(self.workers_of('speed')), 2 * speed)
                self.assertEqual(len(self.workers_of('ram-init')), 2 * ram)
                self.assertEqual(len(self.workers_of('ram-cycle')), 2 * ram)

    def test_a_bad_count_stops_before_anything_exists(self):
        for name in ('PERF_RUNS', 'PERF_RAM_RUNS'):
            with self.subTest(name=name):
                status, out, err = self.execute(campaigns.perf_baseline, [], {**self.env, name: '2+3'})
                self.assertEqual((status, out), (2, ''))
                self.assertEqual(err, f"run-perf-baseline.sh: {name} must be a decimal count (got '2+3')\n")
                self.assertFalse(self.work.exists())
                self.assertEqual(self.log, [])

    def test_the_first_bad_count_wins_and_option_errors_come_before_counts(self):
        status, _, err = self.execute(campaigns.perf_baseline, [], {**self.env, 'PERF_RUNS': 'x', 'PERF_RAM_RUNS': 'y'})
        self.assertIn('PERF_RUNS', err)
        status, _, err = self.execute(campaigns.perf_baseline, ['--nope'], {**self.env, 'PERF_RUNS': 'x'})
        self.assertEqual((status, err), (2, 'unknown function-order argument: --nope\n'))

    def test_a_missing_or_off_pin_oracle_stops_with_the_runbook_message(self):
        (self.oracle / 'oracle-pin.txt').unlink()
        status, out, err = self.execute(campaigns.perf_baseline, [], self.env)
        self.assertEqual((status, out), (1, ''))
        self.assertEqual(err, f'fatal: pin-built oracle not found at {self.oracle}\n  build it with tools/oracle/build-oracle.sh\n')
        self.assertTrue(self.work.is_dir())
        (self.oracle / 'oracle-pin.txt').write_text('pin_ref=libpinyin-9.9.9\n')
        status, _, err = self.execute(campaigns.perf_baseline, [], self.env)
        self.assertEqual((status, err), (1, f'fatal: oracle prefix at {self.oracle} is off-pin\n'))
        self.assertEqual(self.kinds('run'), [])

    def test_reusing_a_stage_skips_the_build_only_when_asked(self):
        self.stage()
        _, out, _ = self.execute(campaigns.perf_baseline, [], {**self.env, 'PERF_REUSE_STAGE': '1'})
        self.assertIn(f'--- reusing stage: {self.work}/stage ---', out)
        self.assertEqual(self.kinds('heavy'), [])
        self.log.clear()
        _, out, _ = self.execute(campaigns.perf_baseline, [], self.env)
        self.assertIn('--- cargo cinstall oxpinyin-capi ->', out)
        self.assertEqual(len(self.kinds('heavy')), 1)

    def test_taskset_is_used_only_when_it_exists(self):
        self.execute(campaigns.perf_baseline, [], {**self.env, 'PERF_RUNS': '1', 'PERF_RAM_RUNS': '0', 'PERF_CPU': '3'})
        self.assertEqual({event[1] for event in self.kinds('taskset')}, {'3'})
        self.taskset, self.log[:] = False, []
        self.execute(campaigns.perf_baseline, [], {**self.env, 'PERF_RUNS': '1', 'PERF_RAM_RUNS': '0'})
        self.assertEqual(self.kinds('taskset'), [])
        self.assertEqual(len(self.workers_of('speed')), 2)

    def test_a_failing_worker_ends_the_run_with_its_status_after_its_output_is_kept(self):
        self.worker_status = {2: 7}
        status, out, _ = self.execute(campaigns.perf_baseline, [], self.env)
        self.assertEqual(status, 7)
        self.assertEqual(len(self.workers_of()), 2)
        self.assertEqual((self.work / 'speed.jsonl').read_text().count('\n'), 2)
        self.assertEqual((self.work / 'oxpinyin-speed.err').read_bytes(), b'worker diagnostics\n')
        self.assertNotIn('captures:', out)

    def test_a_refused_runtime_stops_before_the_first_worker(self):
        def refuse(cell, runner, side, obj, **kwargs):
            raise bench.RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to /usr/lib')
        with mock.patch.object(bench, 'check', refuse):
            status, _, err = self.execute(campaigns.perf_baseline, [], self.env)
        self.assertEqual(status, 1)
        self.assertEqual(err, 'REFUSED: run-perf-baseline.sh: pin: libtkrzw resolved to /usr/lib\n')
        self.assertEqual(self.workers_of(), [])

    def test_the_data_tree_is_populated_from_the_export_and_checked(self):
        export = self.root / 'export'
        export.mkdir()
        self.stage()
        data = self.work / 'stage/usr/share/oxpinyin'
        (data / 'bigram.db').unlink()
        env = {**self.env, 'PERF_REUSE_STAGE': '1', 'PINYIN_EXPORT_DIR': str(export)}
        status, out, err = self.execute(campaigns.perf_baseline, [], env)
        self.assertEqual(status, 1)
        self.assertIn('--- populating oxpinyin data from export + model cache ---', out)
        self.assertEqual(err, f'fatal: exported file(s) pinyin_index.bin phrase_index.bin bigram.db table.conf not found at {export};\n'
                              '  generate with: cargo run --release -p oxpinyin-datagen -- compile --out-dir DIR\n')
        for name in ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'table.conf', 'extra.table'):
            (export / name).write_bytes(name.encode())
        (export / 'link.bin').symlink_to(export / 'bigram.db')
        status, _, err = self.execute(campaigns.perf_baseline, [], env)
        self.assertEqual(status, 0, err)
        self.assertEqual((data / 'bigram.db').read_bytes(), b'bigram.db')
        self.assertEqual((data / 'extra.table').read_bytes(), b'extra.table')
        self.assertEqual((data / 'link.bin').read_bytes(), b'bigram.db')

    def test_a_missing_installed_table_is_named(self):
        self.stage()
        (self.work / 'stage/usr/share/oxpinyin/gb_char.bin').unlink()
        status, _, err = self.execute(campaigns.perf_baseline, [], {**self.env, 'PERF_REUSE_STAGE': '1'})
        self.assertEqual((status, err), (1, f'fatal: missing {self.work}/stage/usr/share/oxpinyin/gb_char.bin\n'))

    def test_a_failing_report_stops_the_run_and_leaves_its_file_empty(self):
        self.report_status = {'perf-baseline.py': 1}
        status, out, _ = self.execute(campaigns.perf_baseline, [], self.env)
        self.assertEqual(status, 1)
        self.assertNotIn('captures:', out)
        self.assertEqual(len(self.workers_of('speed')), 4)


class MatrixTests(World):
    def setUp(self):
        super().setUp()
        self.out = self.root / 'out'
        self.env = {'MATRIX_OUT': str(self.out), 'PERF_RUNS': '2', 'PERF_RAM_RUNS': '1', 'PERF_REPEATS': '3'}
        self.data = {}
        for label, so, data in [('libpinyin-tkrzw', self.root / 'a/libpinyin.so', self.root / 'a/data'),
                                ('libpinyin-kc', self.root / 'b/libpinyin.so', self.root / 'b/data'),
                                ('oxpinyin-tkrzw', self.root / 'c/libpinyin.so', self.root / 'c/data'),
                                ('oxpinyin-kc', self.root / 'd/libpinyin.so', self.root / 'd/data')]:
            so.parent.mkdir(parents=True)
            so.write_bytes(b'')
            data.mkdir()
            self.data[label] = (so, data)
        self.enterContext(mock.patch.object(campaigns, 'matrix_cells', lambda: [
            (label, so, data) for label, (so, data) in self.data.items()]))

    def test_four_cells_are_measured_round_robin_with_the_repeats_everywhere(self):
        status, out, err = self.execute(campaigns.perf_matrix, ['--unordered-arm', 'C'], self.env)
        self.assertEqual(status, 0, err)
        labels = ['libpinyin-tkrzw', 'libpinyin-kc', 'oxpinyin-tkrzw', 'oxpinyin-kc']
        speed = [(w[1], w[2]) for w in self.workers_of('speed')]
        self.assertEqual(speed, [(label, 'speed') for label in labels] * 2)
        ram = [(w[1], w[2]) for w in self.workers_of() if w[2] != 'speed']
        self.assertEqual(ram, [(label, mode) for label in labels for mode in ('ram-init', 'ram-cycle')])
        self.assertEqual({w[4] for w in self.workers_of()}, {'3'})
        self.assertEqual(self.kinds('order')[0][1:], (['C/pinyin', 'D/pinyin', 'A/pinyin', 'B/pinyin'],
                                                       ['--unordered-arm', 'C'], ['A', 'B']))
        self.assert_guarded_before_work()
        standalone = [(c[2], c[1]) for c in self.kinds('check')[:2]]
        self.assertEqual(standalone, [('pin', 'tkrzw'), ('subject', 'tkrzw')])
        self.assertIn('=== Matrix Configuration ===', out)
        self.assertIn('--- speed: 2 alternating runs × 8 cycles × 3 passes, CPU 0 ---', out)
        self.assertIn('--- RAM: 1 runs per mode per cell, CPU 0 ---', out)
        self.assertTrue(out.endswith(f'\ncaptures: {self.out}/{{speed,ram-init,ram-cycle}}.jsonl\n'
                                     f'report:   {self.out}/matrix-report.md\n'))
        script, argv = self.reports[0]
        self.assertEqual(Path(script).name, 'perf-matrix.py')
        self.assertEqual(argv[argv.index('--labels') + 1:argv.index('--size-prefixes')], labels)
        self.assertEqual(argv[argv.index('--size-prefixes') + 1], '/opt/libpinyin-tkrzw')
        self.assertEqual(argv[-4:], [str(self.data[label][1]) for label in labels])

    def test_a_missing_library_or_data_directory_is_named_after_the_cell_is_printed(self):
        self.data['oxpinyin-kc'][1].rmdir()
        status, out, err = self.execute(campaigns.perf_matrix, [], self.env)
        self.assertEqual(status, 1)
        self.assertEqual(err, f'fatal: {self.data["oxpinyin-kc"][1]} not found\n')
        self.assertIn(f'  oxpinyin-kc:\n    so:   {self.data["oxpinyin-kc"][0]}\n', out)
        self.assertEqual(self.workers_of(), [])

    def test_counts_are_checked_before_the_output_directory_is_made(self):
        status, _, err = self.execute(campaigns.perf_matrix, [], {**self.env, 'PERF_RAM_RUNS': '-1'})
        self.assertEqual((status, self.out.exists()), (2, False))
        self.assertIn('run-perf-matrix.sh: PERF_RAM_RUNS must be a decimal count', err)


class SameDataTests(World):
    def setUp(self):
        super().setUp()
        self.out = self.root / 'out'
        self.target = self.root / 'target'
        self.env = {'MATRIX_OUT': str(self.out), 'CARGO_TARGET_DIR': str(self.target), 'PERF_RUNS': '1',
                    'PERF_RAM_RUNS': '1'}
        self.libs = {}
        for name in ('tkrzw', 'kc'):
            lib = self.root / f'pin-{name}/libpinyin.so'
            lib.parent.mkdir(parents=True)
            lib.write_bytes(b'')
            self.libs[name] = lib
        self.enterContext(mock.patch.object(campaigns, 'LIBPINYIN_TKRZW', self.root / 'pin-tkrzw'))
        self.enterContext(mock.patch.object(campaigns, 'LIBPINYIN_KC', self.root / 'pin-kc'))
        self.needed = {'kyotocabinet': ['libkyotocabinet.so.16', 'libc.so.6'], 'tkrzw': ['libtkrzw.so.1']}
        self.enterContext(mock.patch.object(identity, 'needed', lambda library, **k: self.needed[
            'kyotocabinet' if 'kc' in Path(library).name else 'tkrzw']))
        self.enterContext(mock.patch.object(campaigns.reports, 'same_data_summary', lambda *paths: 'SUMMARY\n'))

    def test_prebuilt_libraries_skip_the_builds_and_are_verified_and_measured_on_shared_data(self):
        kc, tkrzw = self.root / 'prebuilt-kc.so', self.root / 'prebuilt-tkrzw.so'
        for path in (kc, tkrzw):
            path.write_bytes(b'')
        status, out, err = self.execute(campaigns.perf_same_data, ['--unordered-arm', 'C', '--unordered-arm', 'D'], {
            **self.env, 'OXPINYIN_KC_SO': str(kc), 'OXPINYIN_TKRZW_SO': str(tkrzw)})
        self.assertEqual(status, 0, err)
        self.assertEqual(self.kinds('heavy'), [])
        self.assertIn('--- verifying capi backend linkage ---\n--- building bisect harness ---\n', out)
        workers = {(w[1], w[2]): w for w in self.workers_of()}
        self.assertEqual(set(label for label, _ in workers), {'libpinyin-tkrzw', 'libpinyin-kc', 'oxpinyin-tkrzw', 'oxpinyin-kc'})
        # the oxpinyin cells open the pin's own data
        self.assertEqual(workers[('oxpinyin-tkrzw', 'speed')][6][-1], str(self.root / 'pin-tkrzw/lib/libpinyin/data'))
        self.assertEqual(workers[('oxpinyin-kc', 'speed')][6][-1], str(self.root / 'pin-kc/lib/libpinyin/data'))
        self.assertTrue(out.endswith('--- summary (medians) ---\nSUMMARY\n'))
        self.assertEqual(self.kinds('order')[0][1:], (['C/pinyin', 'D/pinyin', 'A/pinyin', 'B/pinyin'],
                                                       ['--unordered-arm', 'C', '--unordered-arm', 'D'], ['A', 'B']))
        self.assert_guarded_before_work()
        self.assertEqual(self.reports, [])

    def test_unset_libraries_are_built_in_sequence_stripped_and_copied_out(self):
        status, out, err = self.execute(campaigns.perf_same_data, ['--unordered-arm', 'C', '--unordered-arm', 'D'], self.env)
        self.assertEqual(status, 0, err)
        builds = [event[1] for event in self.kinds('heavy')]
        self.assertEqual([build[build.index('--features') + 1] for build in builds], ['kyotocabinet', 'tkrzw'])
        self.assertEqual(builds[0][:6], ['cargo', 'build', '--locked', '--release', '-p', 'oxpinyin-capi'])
        strips = [event[1] for event in self.kinds('run') if event[1][0] == 'strip']
        self.assertEqual([command[-1] for command in strips],
                         [str(self.out / 'libpinyin_capi-kc.so'), str(self.out / 'libpinyin_capi-tkrzw.so')])
        self.assertEqual((self.out / 'libpinyin_capi-kc.so').read_bytes(), b'\x7fELF kyotocabinet')
        self.assertEqual((self.out / 'libpinyin_capi-tkrzw.so').read_bytes(), b'\x7fELF tkrzw')
        self.assertIn('--- building oxpinyin capi (kyotocabinet) ---\n', out)

    def verify(self, backend, library, needed):
        err = Stream()
        with contextlib.redirect_stderr(err), mock.patch.object(identity, 'needed', lambda path, **k: needed):
            try:
                campaigns.verify_backend(backend, library)
            except runners.Finished as stop:
                return stop.status, err.text()
        return None, err.text()

    def test_an_artifact_must_link_its_backend_and_only_that_one(self):
        library = self.root / 'x.so'
        library.write_bytes(b'')
        for backend, needed, message in (
                ('tkrzw', ['libkyotocabinet.so.16'],
                 f'fatal: {library} does not link a tkrzw library (NEEDED: libkyotocabinet.so.16)\n'),
                ('kyotocabinet', ['libkyotocabinet.so.16', 'libtkrzw.so.1'],
                 f"fatal: {library} links a second backend's library (NEEDED: libkyotocabinet.so.16 libtkrzw.so.1)\n"),
                ('tkrzw', [], f'fatal: {library} does not link a tkrzw library (NEEDED: )\n'),
                ('bogus', ['libtkrzw.so.1'], "fatal: verify_backend: unknown backend 'bogus'\n")):
            with self.subTest(backend=backend, needed=needed):
                self.assertEqual(self.verify(backend, library, needed), (1, message))
        self.assertEqual(self.verify('tkrzw', library, ['libtkrzw.so.1', 'libc.so.6']), (None, ''))
        missing = self.root / 'missing.so'
        self.assertEqual(self.verify('tkrzw', missing, []), (1, f'fatal: {missing} not found\n'))


class ControlTests(World):
    """shipped-check and tree-recipe-control: pooled builds, then round-robin over CTRL_SIZES."""

    def setUp(self):
        super().setUp()
        self.out = self.root / 'out'
        self.pin = self.root / 'opt-pin'
        (self.pin / 'lib').mkdir(parents=True)
        (self.pin / 'lib/libpinyin.so').write_bytes(b'pin')
        self.enterContext(mock.patch.object(campaigns, 'LIBPINYIN_TKRZW', self.pin))
        tools = self.root / 'ctltools'
        (tools / 'bisection').mkdir(parents=True)
        self.env = {'CTRL_OUT': str(self.out), 'CTRL_RUNS': '2', 'CTRL_SIZES': '1 8', 'CTLTOOLS': str(tools),
                    'SRC_CURR': str(self.root / 'curr'), 'SRC_OLD': str(self.root / 'old'), 'CTRL_CPU': '1'}
        self.enterContext(mock.patch.object(identity, 'fixture_hooks', lambda library, **k: 2 if 'Z' in str(library) else 0))
        self.enterContext(mock.patch.object(identity, 'needed', lambda library, **k: ['libtkrzw.so.1', 'libc.so.6']))
        self.enterContext(mock.patch.object(campaigns, 'tempfile_directory', lambda: tempfile.TemporaryDirectory(dir=self.root)))
        # The runners' fixed /tmp artifact names stay out of the real /tmp.
        (self.root / 'scratch').mkdir()
        self.enterContext(mock.patch.object(campaigns, 'SCRATCH', self.root / 'scratch'))

    def test_shipped_check_builds_two_artifacts_then_samples_l_z_s_for_each_size(self):
        status, out, err = self.execute(campaigns.shipped_check, ['--unordered-arm', 'Z', '--unordered-arm', 'S'], self.env)
        self.assertEqual(status, 0, err)
        self.assertEqual(self.kinds('builds'), [('builds', 'shipped-check builds', ['Z', 'S'])])
        self.assert_guarded_before_work()
        features = [command[command.index('--features') + 1] for command in self.builds]
        self.assertEqual(features, ['tkrzw', 'tkrzw,shipped'])
        self.assertEqual(self.kinds('order')[0][1:], (['Z/pinyin', 'S/pinyin', 'L/pinyin'], ['--unordered-arm', 'Z', '--unordered-arm', 'S'], ['L']))
        order = [(w[1], w[4]) for w in self.workers_of()]
        cells = ['libpinyin-tkrzw', 'oxpinyin-cur-cinstall', 'oxpinyin-shipped']
        self.assertEqual(order, [(cell, '1') for cell in cells] * 2 + [(cell, '8') for cell in cells] * 2)
        self.assertEqual({event[1] for event in self.kinds('taskset')}, {'1'})
        self.assertIn('--- n=1: 2 runs, round-robin L Z S ---', out)
        identity_text = (self.out / 'shipped-identity.txt').read_text()
        self.assertTrue(identity_text.startswith('=== identity (shipped check) ===\ncaptured_utc: 2026-10-27T00:00:00Z\nL: '))
        self.assertIn('Z fixture hooks: 2\nS fixture hooks: 0\n', identity_text)
        self.assertIn('=== identity (shipped check) ===', out)
        self.assertEqual(sorted(path.name for path in self.out.glob('shipped-ratios-n*.md')),
                         ['shipped-ratios-n1.md', 'shipped-ratios-n8.md'])
        self.assertTrue((self.out / 'shipped-ratios-n1.md').read_text().startswith('## n=1\n'))
        self.assertEqual(out.splitlines()[-1], 'done: 12 rows')
        self.assertEqual(len((self.out / 'speed-shipped.jsonl').read_text().splitlines()), 12)

    def test_the_control_runners_use_the_image_harness_source_and_do_not_move_directory(self):
        before = os.getcwd()
        self.execute(campaigns.shipped_check, ['--unordered-arm', 'Z', '--unordered-arm', 'S'], self.env)
        harness = [event[1] for event in self.kinds('run')][0]
        self.assertEqual(harness[harness.index('-o') + 2], str(campaigns.IMAGE_HARNESS_SOURCE))
        self.assertEqual(os.getcwd(), before)

    def test_only_the_first_bad_count_is_named_and_the_directory_is_not_made(self):
        status, _, err = self.execute(campaigns.shipped_check, [], {**self.env, 'CTRL_RUNS': 'x'})
        self.assertEqual((status, self.out.exists()), (2, False))
        self.assertEqual(err, "run-shipped-check.sh: CTRL_RUNS must be a decimal count (got 'x')\n")

    def test_tree_recipe_control_gates_on_the_record_artifact_before_any_sample(self):
        with mock.patch.object(identity, 'sha256', lambda path: 'f' * 64):
            status, out, err = self.execute(campaigns.tree_recipe_control, ['--unordered-arm', 'X'], self.env)
        self.assertEqual(status, 1)
        self.assertEqual(err, "FATAL: cell X is not the record's artifact.\n  built:  " + 'f' * 64 + f"\n  record: {SHA_X}\n"
                              "The control cannot proceed: X must reproduce the record's build before\n"
                              "any ratio is read. Stop and report.\n")
        self.assertEqual(self.workers_of(), [])
        self.assertEqual(self.kinds('builds')[0][2], ['X', 'Y', 'Z', 'S'])
        self.assertTrue((self.out / 'environment.txt').read_text().startswith('=== environment ===\ncaptured_utc: 2026-10-27T00:00:00Z\ncontroller_commit: unset\n'))
        self.assertIn('S: ' + 'f' * 64, (self.out / 'build-identity.txt').read_text())

    def test_tree_recipe_control_samples_l_x_y_z_s_and_writes_ratios_and_decomposition(self):
        with mock.patch.object(identity, 'sha256', lambda path: SHA_X):
            status, out, err = self.execute(campaigns.tree_recipe_control, ['--unordered-arm', 'X'],
                                            {**self.env, 'CTRL_COMMIT': 'abc123'})
        self.assertEqual(status, 0, err)
        self.assertIn(f"GATE: X reproduces the record's artifact byte-for-byte ({SHA_X})", out)
        self.assertEqual(self.kinds('order')[0][1:], (['X/pinyin', 'Y/pinyin', 'Z/pinyin', 'S/pinyin', 'L/pinyin'],
                                                       ['--unordered-arm', 'X'], ['L']))
        self.assert_guarded_before_work()
        order = [(w[1], w[4]) for w in self.workers_of()]
        cells = ['libpinyin-tkrzw', 'oxpinyin-old-cargo', 'oxpinyin-cur-cargo', 'oxpinyin-cur-cinstall', 'oxpinyin-cur-shipped']
        self.assertEqual(order, [(cell, '1') for cell in cells] * 2 + [(cell, '8') for cell in cells] * 2)
        self.assertIn('--- n=1: 2 runs x 8 cycles x 1 repeats, round-robin L X Y Z S ---', out)
        names = [Path(script).name for script, _ in self.reports]
        self.assertEqual(names, ['perf-ratio.py'] * 10 + ['perf-decomp.py'] * 2)
        self.assertEqual(Path(self.reports[0][0]).parent, self.root / 'ctltools/bisection')
        ratio_pairs = [(argv[argv.index('--numerator') + 1], argv[argv.index('--denominator') + 1]) for script, argv in self.reports[:5]]
        self.assertEqual(ratio_pairs, [('oxpinyin-old-cargo', 'libpinyin-tkrzw'), ('oxpinyin-cur-cargo', 'libpinyin-tkrzw'),
                                       ('oxpinyin-cur-cinstall', 'libpinyin-tkrzw'), ('oxpinyin-cur-shipped', 'libpinyin-tkrzw'),
                                       ('oxpinyin-cur-shipped', 'oxpinyin-cur-cinstall')])
        self.assertTrue((self.out / 'ratios-n1.md').read_text().startswith('## size n=1 — ratios vs L (perf-ratio.py, seed 20260907)\n'))
        self.assertTrue((self.out / 'decomp-n8.md').exists())
        load = (self.out / 'load.txt').read_text().splitlines()
        self.assertEqual(load, ['before n=1: 0.1 0.2 0.3 1/2 3', 'after  n=1: 0.1 0.2 0.3 1/2 3',
                                'before n=8: 0.1 0.2 0.3 1/2 3', 'after  n=8: 0.1 0.2 0.3 1/2 3'])
        self.assertIn(f'=== captures written to {self.out} ===', out)
        self.assertEqual(out.splitlines()[-1], 'speed.jsonl')
        text = (self.out / 'build-identity.txt').read_text()
        self.assertIn('Z: ' + SHA_X, text)
        self.assertIn('fixture-hooks: 2  NEEDED: libtkrzw.so.1 libc.so.6 \n', text)
        self.assertTrue(text.endswith('fixture-hook expectation: Z 2, S 0 (the shipped feature compiles them out)\n'))
        self.assertIn('controller_commit: abc123', (self.out / 'environment.txt').read_text())
        self.assertIn('\nspeed rows: 20\n', out)

    def test_a_pin_root_that_is_not_there_is_finds_own_complaint(self):
        missing = self.root / 'no-pin'
        with mock.patch.object(campaigns, 'LIBPINYIN_TKRZW', missing):
            status, out, err = self.execute(campaigns.shipped_check, ['--unordered-arm', 'Z', '--unordered-arm', 'S'],
                                            self.env)
        self.assertEqual((status, out, err), (1, '', f"find: '{missing}': No such file or directory\n"))

    def test_a_pin_root_without_the_library_names_it(self):
        empty = self.root / 'empty-pin'
        empty.mkdir()
        with mock.patch.object(campaigns, 'LIBPINYIN_TKRZW', empty):
            status, _, err = self.execute(campaigns.shipped_check, ['--unordered-arm', 'Z', '--unordered-arm', 'S'],
                                          self.env)
        self.assertEqual((status, err), (1, f'fatal: libpinyin.so not found under {empty}\n'))

    def test_a_failing_ratio_script_leaves_the_header_and_what_it_printed_and_ends_the_run(self):
        self.report_status = {'perf-ratio.py': 3}
        status, _, _ = self.execute(campaigns.shipped_check, ['--unordered-arm', 'Z', '--unordered-arm', 'S'], self.env)
        self.assertEqual(status, 3)
        self.assertEqual((self.out / 'shipped-ratios-n1.md').read_text(), '## n=1\nREPORT perf-ratio.py\n')
        self.assertEqual(sorted(path.name for path in self.out.glob('shipped-ratios*')), ['shipped-ratios-n1.md'])
        self.assertEqual(len(self.reports), 1)

    def test_a_failing_worker_stops_the_control_with_its_status(self):
        self.worker_status = {3: 9}
        status, _, _ = self.execute(campaigns.shipped_check, ['--unordered-arm', 'Z', '--unordered-arm', 'S'], self.env)
        self.assertEqual((status, len(self.workers_of())), (9, 3))
        self.assertEqual(list(self.out.glob('shipped-ratios*')), [])


class RssDiagTests(World):
    def setUp(self):
        super().setUp()
        self.bisect = self.root / 'bisection'
        self.bisect.mkdir()
        (self.bisect / 'bisect.c').write_text('int main(void){return 0;}\n')
        for name, value in (('BISECT', self.bisect), ('HARNESS_SOURCE', self.bisect / 'bisect.c')):
            self.enterContext(mock.patch.object(bench, name, value))
        self.lp, self.ox = self.root / 'lp.so', self.root / 'ox.so'
        self.lp.write_bytes(b'lp')
        self.ox.write_bytes(b'ox')
        self.out = self.root / 'rss'
        self.args = ['--lp', str(self.lp), '--ox', str(self.ox), '--data', str(self.root / 'data'), '--out', str(self.out),
                     '--rounds', '2', '--cycles', '3']
        self.enterContext(mock.patch.object(identity, 'needed', lambda library, **k: ['libtkrzw.so.1']))

    def test_rounds_alternate_l_and_s_then_one_mapping_round_per_cell(self):
        status, out, err = self.execute(campaigns.rss_diag, self.args + ['--unordered-arm', 'S'], {'PERF_CPU': '2'})
        self.assertEqual(status, 0, err)
        self.assertEqual([(w[1], w[2], w[3]) for w in self.workers_of()], [('L', 'rss-diag', '3'), ('S', 'rss-diag', '3')] * 2
                         + [('L', 'rss-diag', '3'), ('S', 'rss-diag', '3')])
        self.assertEqual(out, f'wrote {self.out}\n')
        rounds = [json.loads(line) for line in (self.out / 'rounds.jsonl').read_text().splitlines()]
        self.assertEqual([(row['cell'], row['round']) for row in rounds], [('L', 1), ('S', 1), ('L', 2), ('S', 2)])
        self.assertEqual(rounds[0]['loadavg_before'], '0.1 0.2 0.3')
        self.assertEqual(rounds[0]['result']['mode'], 'rss-diag')
        self.assertEqual({tag for tag in (self.out / 'maps').iterdir()}, {self.out / 'maps/L-round.json', self.out / 'maps/S-round.json'})
        self.assertEqual(self.kinds('order')[0][1:], (['S/pinyin', 'L/pinyin'], ['--unordered-arm', 'S'], ['L']))
        self.assert_guarded_before_work()

    def test_the_mapping_round_alone_sets_the_diagnostic_directory(self):
        seen = []
        original = self.fake_capture

        def capture(command, *, env=None, **kw):
            seen.append((env.get('RSS_DIAG_DIR'), env.get('RSS_DIAG_TAG')))
            return original(command, env=env, **kw)
        with mock.patch.object(bench, 'capture', capture):
            self.execute(campaigns.rss_diag, self.args + ['--unordered-arm', 'S'], {})
        self.assertEqual(seen, [(None, None)] * 4 + [(str(self.out / 'maps'), 'L'), (str(self.out / 'maps'), 'S')])

    def test_the_identity_names_everything_the_numbers_depend_on(self):
        self.execute(campaigns.rss_diag, self.args, {})
        text = (self.out / 'identity.txt').read_text().splitlines()
        self.assertEqual(text[0], 'captured_utc=2026-10-27T00:00:00Z')
        self.assertEqual(text[4], 'rounds=2 cycles=3 cpu=0')
        self.assertEqual(text[5], f'data_dir={self.root / "data"}')
        self.assertTrue(text[6].startswith('repo_revision='))
        self.assertTrue(text[7].startswith(f'bisect={self.bisect / "bisect"} sha256='))
        self.assertEqual(text[-4:-2][0].split(' ')[0], f'so={self.lp}')
        self.assertEqual(text[-1], '  needed=libtkrzw.so.1 ')

    def test_the_harness_is_rebuilt_only_when_missing_or_older_than_its_source(self):
        self.execute(campaigns.rss_diag, self.args + ['--unordered-arm', 'S'], {})
        self.assertEqual(len([e for e in self.kinds('run') if e[1][0] == 'gcc']), 1)
        harness = self.bisect / 'bisect'
        harness.write_bytes(b'ok')
        harness.chmod(0o755)
        os.utime(harness, (2_000_000_000, 2_000_000_000))
        os.utime(self.bisect / 'bisect.c', (1_000_000_000, 1_000_000_000))
        self.log.clear()
        self.execute(campaigns.rss_diag, self.args + ['--unordered-arm', 'S'], {})
        self.assertEqual([e for e in self.kinds('run') if e[1][0] == 'gcc'], [])
        os.utime(self.bisect / 'bisect.c', (3_000_000_000, 3_000_000_000))
        self.execute(campaigns.rss_diag, self.args + ['--unordered-arm', 'S'], {})
        self.assertEqual(len([e for e in self.kinds('run') if e[1][0] == 'gcc']), 1)

    def test_help_and_every_usage_error_print_the_header_through_usage_and_exit_2(self):
        for argv, preface in (([ '-h'], ''), (['--help'], ''), (['--bogus'], 'unknown argument: --bogus\n'),
                              (['--lp', 'x'], ''), (['--rounds', 'x', '--lp', 'a'], '--rounds must be a decimal integer: x\n'),
                              (['--rounds'], '--rounds must be a decimal integer: \n')):
            with self.subTest(argv=argv):
                status, out, err = self.execute(campaigns.rss_diag, argv, {})
                self.assertEqual((status, out), (2, ''))
                self.assertEqual(err, preface + campaigns.RSS_USAGE)
        self.assertEqual(self.log, [])
        self.assertTrue(campaigns.RSS_USAGE.startswith('# rss-diag'))
        self.assertTrue(campaigns.RSS_USAGE.endswith('[--out DIR] [--rounds N] [--cycles N]\n'))

    def test_an_option_without_its_operand_is_a_failure_not_a_usage_error(self):
        status, _, err = self.execute(campaigns.rss_diag, ['--lp'], {})
        self.assertEqual(status, 1)
        self.assertIn('--lp', err)

    def test_zero_rounds_still_takes_the_mapping_rounds(self):
        status, _, _ = self.execute(campaigns.rss_diag, self.args[:-4] + ['--rounds', '0'], {})
        self.assertEqual(status, 0)
        self.assertEqual(len(self.workers_of()), 2)
        self.assertEqual((self.out / 'rounds.jsonl').read_text(), '')


if __name__ == '__main__':
    unittest.main()
