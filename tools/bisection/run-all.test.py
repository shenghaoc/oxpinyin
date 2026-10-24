#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Regressions for run-all.py's issue resolution, runner timing and artifact selection."""
import json
import contextlib
import io
import os
from pathlib import Path
import runpy
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

run_all = runpy.run_path(str(Path(__file__).with_name('run-all.py')))


class RunnerSelectionTests(unittest.TestCase):
    def test_absent_runners_lists_the_default_registry(self):
        expected = json.loads(Path(__file__).with_name('runners.json').read_text())
        output = io.StringIO()
        namespace = run_all['main'].__globals__
        with mock.patch.object(sys, 'argv', ['run-all.py', '--list']), \
                mock.patch.dict(namespace, resource_snapshot=mock.Mock(side_effect=AssertionError('scheduled work'))), \
                contextlib.redirect_stdout(output):
            self.assertEqual(run_all['main'](), 0)
        self.assertEqual([json.loads(line) for line in output.getvalue().splitlines()], expected)

    def test_empty_runners_exit_two_before_scheduling_even_with_an_issue(self):
        namespace = run_all['main'].__globals__
        for value in ('', ' ', ',', 'contract-diff,', ',contract-diff'):
            for extra in ([], ['--issue', '532'], ['--list']):
                with self.subTest(value=value, extra=extra):
                    error = io.StringIO()
                    schedule = mock.Mock(side_effect=AssertionError('scheduled work'))
                    with mock.patch.object(sys, 'argv', ['run-all.py', '--runners', value, *extra]), \
                            mock.patch.dict(namespace, resource_snapshot=schedule, execute=schedule), \
                            contextlib.redirect_stderr(error), self.assertRaises(SystemExit) as caught:
                        run_all['main']()
                    self.assertEqual(caught.exception.code, 2)
                    self.assertIn('--runners must not contain an empty value', error.getvalue())
                    schedule.assert_not_called()


class IssueResolutionTests(unittest.TestCase):
    index = {'tags': {'532': ['default-option-parse', 'explicit-use-tone-parse'], '640': ['separator/pinyin/0/parse/0'],
                      'family/separator': ['separator/pinyin/0/parse/0']},
             'modes': {'default-option-parse': 'pinyin', 'explicit-use-tone-parse': 'pinyin',
                       'separator/pinyin/0/parse/0': 'pinyin'}}

    def test_tags_normalise_like_the_contract_driver(self):
        self.assertEqual(run_all['normal_tag']('#W11'), 'w11')
        self.assertEqual(run_all['normal_tag']('Row 33'), 'row:33')

    def test_requested_tags_resolve_to_exact_case_ids(self):
        chosen = run_all['contract_cases_for']({'532', '640', '9999'}, self.index)
        self.assertEqual(chosen, {'default-option-parse': ['532'], 'explicit-use-tone-parse': ['532'],
                                  'separator/pinyin/0/parse/0': ['640']})
        self.assertEqual(run_all['contract_cases_for']({'9999'}, self.index), {})

    def test_plan_names_runners_inputs_cases_and_facades(self):
        row = dict(name='contract-diff', input_paths=['{capi}', '{zhuyin}'])
        other = dict(name='pc-version', oracle=False, input_paths=[])
        jobs = [(row, None, {}, dict(capi='/t/libpinyin_capi.so', zhuyin='/t/libzhuyin_capi.so')),
                (other, None, {}, {})]
        chosen = run_all['contract_cases_for']({'532'}, self.index)
        lines = run_all['plan_lines']('bdb', jobs, chosen, {'contract-diff': '--issue 532: 2 contract cases',
                                                            'pc-version': '--runners'}, self.index)
        self.assertEqual(lines[0], 'plan (bdb): 2 runners')
        self.assertIn('contract-diff: --issue 532: 2 contract cases; oracle: yes; inputs: /t/libpinyin_capi.so, /t/libzhuyin_capi.so', lines[1])
        self.assertIn('2 contract cases; facades: pinyin 2', lines[2])
        self.assertTrue(lines[3].strip().startswith('default-option-parse') and 'issue 532' in lines[3])
        self.assertIn('pc-version: --runners; oracle: no; inputs: none', lines[-1])

    def test_plan_resolves_env_placeholders_like_the_runner(self):
        row = dict(name='user-dir-round-trip', input_paths=['{capi}', '{env[OXPINYIN_USER_RT_TEST]}'],
                   env={'OXPINYIN_USER_RT_TEST': '{root}/target/rt-test'})
        substitutions = dict(root='/w', capi='/w/libpinyin_capi.so')
        self.assertEqual(run_all['planned_inputs'](row, {}, substitutions),
                         ['/w/libpinyin_capi.so', '/w/target/rt-test'])
        unset = dict(name='pc-metadata', input_paths=['{env[OXPINYIN_METADATA_PIN_ROOT]}'])
        self.assertEqual(run_all['planned_inputs'](unset, {'PATH': '/bin'}, {}),
                         ['{env[OXPINYIN_METADATA_PIN_ROOT]}'])
        self.assertEqual(run_all['planned_inputs'](unset, {'OXPINYIN_METADATA_PIN_ROOT': '/pin'}, {}), ['/pin'])
        lines = run_all['plan_lines']('bdb', [(unset, None, {'OXPINYIN_METADATA_PIN_ROOT': '/pin'}, {})], {},
                                      {'pc-metadata': 'default'}, {'tags': {}, 'modes': {}})
        self.assertIn('pc-metadata: default; oracle: yes; inputs: /pin', lines[1])


class TimingTests(unittest.TestCase):
    def test_runner_timing_line_is_read_from_its_output(self):
        output = 'noise\n{"case": "x", "verdict": "MATCH"}\n' + json.dumps(dict(timing=dict(cases=1, seconds={}))) + '\n'
        self.assertEqual(run_all['timing_of'](output), dict(cases=1, seconds={}))
        self.assertIsNone(run_all['timing_of']('pc-version: ok\n'))
        self.assertIsNone(run_all['timing_of']('{"timing": nonsense\n'))
class ArtifactUnionTests(unittest.TestCase):
    def test_union_is_ordered_and_deduplicated(self):
        rows = [dict(name='a', artifacts=['zhuyin-capi', 'pinyin-capi']), dict(name='b', artifacts=['pinyin-capi', 'dictool']),
                dict(name='c', artifacts=[])]
        self.assertEqual(run_all['artifact_union'](rows), ['pinyin-capi', 'zhuyin-capi', 'dictool'])

    def test_a_subjectless_selection_needs_no_build(self):
        self.assertEqual(run_all['artifact_union']([dict(name='pc-version', artifacts=[])]), [])

    def test_a_row_without_a_declaration_is_a_registry_error(self):
        with self.assertRaises(ValueError):
            run_all['artifact_union']([dict(name='x')])
        with self.assertRaises(ValueError):
            run_all['artifact_union']([dict(name='x', artifacts=['libfoo'])])

    def test_every_registered_gate_declares_its_set(self):
        rows = json.loads((Path(__file__).with_name('runners.json')).read_text())
        for row in rows:
            with self.subTest(runner=row['name']):
                self.assertIsInstance(row.get('artifacts'), list)
                self.assertFalse(set(row['artifacts']) - set(run_all['ARTIFACTS']))
                blob = json.dumps(row['command']) + json.dumps(row.get('input_paths', []))
                if '{capi}' in blob:
                    self.assertIn('pinyin-capi', row['artifacts'])
                if '{zhuyin}' in blob:
                    self.assertIn('zhuyin-capi', row['artifacts'])
        self.assertEqual(next(r for r in rows if r['name'] == 'pc-version')['artifacts'], [])
        self.assertEqual(next(r for r in rows if r['name'] == 'user-dir-round-trip')['artifacts'],
                         ['pinyin-capi', 'user-rt-test'])
        self.assertEqual(next(r for r in rows if r['name'] == 'import-diff')['artifacts'], ['pinyin-capi', 'dictool'])


class TimingColumnsTests(unittest.TestCase):
    def test_each_wait_has_its_own_column(self):
        columns = run_all['COLUMNS']
        self.assertEqual(columns[:6], ('runner', 'cell', 'status', 'seconds', 'build_seconds', 'lock_wait_seconds'))
        self.assertEqual(columns[6:9], ('slot_wait_seconds', 'build_slot_wait_seconds', 'memory_wait_seconds'))
        self.assertNotIn('build_wait_seconds', columns)
        self.assertNotIn('resource_wait_seconds', columns)


class ConcurrentContractCellsTests(unittest.TestCase):
    def run_cells(self, available, expected_overlap):
        namespace = run_all['main'].__globals__
        original = Path.read_text
        row = next(r for r in json.loads(original(Path(__file__).with_name('runners.json')))
                   if r['name'] == 'contract-diff')
        row = dict(row, oracle=False, artifacts=[], input_paths=[], required_env=[],
                   command=['synthetic-contract-worker'])
        def read(path, *args, **kwargs):
            return json.dumps([row]) if path.name == 'runners.json' else original(path, *args, **kwargs)
        lock = threading.Lock()
        barrier = threading.Barrier(expected_overlap)
        active, peak, seen = 0, 0, []
        def execute(command, env, cwd, timeout, output, slot=None):
            nonlocal active, peak
            cell = env['PINYIN_ORACLE_DBM']
            marker = json.loads(env['OXPINYIN_HEAVY_SLOT_HELD'])
            record = json.loads(original(Path(marker['lock_dir']) / f"heavy-slot-{marker['slot']}"))
            workers = int(env['OXPINYIN_CONTRACT_JOBS'])
            self.assertGreaterEqual(record['estimate_mib'], workers * 1024)
            self.assertEqual(Path(env['CARGO_TARGET_DIR']).name, cell)
            self.assertEqual(Path(env['PINYIN_ORACLE_PREFIX']).name, cell)
            self.assertIn(marker['fd'], slot.pass_fds())
            with lock:
                active += 1
                peak = max(peak, active)
                seen.append((cell, workers, env['CARGO_TARGET_DIR'], env['TMPDIR']))
            try:
                barrier.wait(timeout=5)
                time.sleep(.05)
                return 0, .05, json.dumps({'timing': {'jobs': workers}}), False
            finally:
                with lock:
                    active -= 1
        with tempfile.TemporaryDirectory() as directory:
            env = dict(os.environ, OXPINYIN_HEAVY_SLOTS='3', OXPINYIN_LOCK_DIR=directory,
                       OXPINYIN_RESOURCE_WAIT_SECONDS='5', CARGO_BUILD_JOBS='1', RUST_TEST_THREADS='1')
            for key in ('OXPINYIN_HEAVY_SLOT_HELD', 'CARGO_TARGET_DIR', 'OXPINYIN_CONTRACT_JOBS'):
                env.pop(key, None)
            resources = dict(available_mib=available, total_mib=16384, cpus=4, cargo_cpus=4, notes=[])
            args = ['run-all.py', '--runners', 'contract-diff', '--cells', 'bdb,kc,tkrzw',
                    '--no-build', '--jobs', '3', '--contract-jobs', '4',
                    '--target-root', directory + '/targets', '--oracle-root', directory + '/oracles']
            output = io.StringIO()
            with mock.patch.dict(os.environ, env, clear=True), mock.patch.object(Path, 'read_text', read), \
                    mock.patch.object(sys, 'argv', args), \
                    mock.patch.dict(namespace, execute=execute,
                                    resource_snapshot=lambda: dict(resources, notes=[])), \
                    mock.patch.object(namespace['_host'], 'process_tree_use_mib', return_value=0), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(run_all['main'](), 0)
            self.assertEqual(resources['available_mib'], available)
            self.assertEqual(resources['notes'], [])
            self.assertEqual(output.getvalue().count(' | PASS | '), 3)
        self.assertEqual({item[0] for item in seen}, {'bdb', 'kc', 'tkrzw'})
        self.assertEqual(len({item[2] for item in seen}), 3)
        self.assertEqual(len({item[3] for item in seen}), 3)
        self.assertEqual(peak, expected_overlap)
        return seen

    def test_three_cells_overlap_with_private_inputs_and_full_worker_reservations(self):
        self.assertEqual({item[1] for item in self.run_cells(16000, 3)}, {4})

    def test_short_memory_reduces_workers_and_serializes_cells(self):
        self.assertEqual({item[1] for item in self.run_cells(5000, 1)}, {3})


if __name__ == '__main__':
    unittest.main()
