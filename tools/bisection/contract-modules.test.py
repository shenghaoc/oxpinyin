#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Behavioural integration checks for the contract differential."""
import importlib.util
import json
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
for path in (str(ROOT), str(HERE)):
    if path not in sys.path:
        sys.path.insert(0, path)

spec = importlib.util.spec_from_file_location('contract_diff', HERE / 'contract-diff.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


class ContractIntegrationTests(unittest.TestCase):
    def test_driver_import_and_listing_from_another_directory(self):
        with tempfile.TemporaryDirectory() as elsewhere:
            result = subprocess.run(
                [sys.executable, '-X', 'lazy_imports=normal', str(HERE / 'contract-diff.py'), '--list-tags'],
                cwd=elsewhere, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)['modes'],
                         {name: spec['mode'] for name, spec in driver.CASES.items()})

    def test_reduced_longer_normal_cases_keep_their_own_policy_metadata(self):
        for name in ('longer-normal-frequency-dedup', 'longer-normal-frequency-tie', 'longer-normal-disabled'):
            with self.subTest(name=name):
                self.assertIsNone(driver.CASES[name]['policy_record'])
                self.assertIn('913', driver.CASES[name]['issues'])
        witness = driver.CASES['s2-nbest-longer-normal-precedence']
        self.assertEqual(witness['policy_record'], 'trellis-adjusted-cost-s1-s2.md')
        self.assertEqual(witness['cells'], ('bdb',))

    def test_stored_search_selection_keeps_the_registered_inventory_cases(self):
        names = (ROOT / 'fixtures/w4/1297-search-cases.txt').read_text().splitlines()
        selected, _ = driver.select_cases(issues=('757', '785'))
        self.assertEqual(names, [name for name in selected if name != 'nbest-user-table-order-pinyin'])
        self.assertEqual(len(names), len(set(names)))

    def test_selection_adapters_select_from_the_registry_the_driver_holds_now(self):
        registry = {'only/one': dict(mode='zhuyin', cells=('kc',), issues=('7',))}
        with patch.object(driver, 'CASES', registry):
            self.assertEqual(driver.select_cases(), (['only/one'], {'only/one': ['default suite']}))
            self.assertEqual(driver.select_cases(issues=['#7'])[0], ['only/one'])
            self.assertEqual(driver.select_cases(prefixes='only/'),
                             (['only/one'], {'only/one': ['prefix only/']}))
            self.assertEqual(driver.tag_index(), {'7': ['only/one'], 'family/only': ['only/one']})
            names, reasons = driver.select_cases('only/one')
            self.assertEqual(driver.plan_lines(names, reasons, Path('/s'), Path('/z'), Path('/pin'), 'bdb')[1],
                             "  1 case skip on this cell (fixture in another backend's format)")
            with self.assertRaises(driver.SelectionError):
                driver.select_cases('plain')

    def test_case_registers_into_the_registry_the_driver_holds_when_the_decorator_is_applied(self):
        registers = sys.modules['contract_registry']
        decorate = driver.case('late/bound', mode='zhuyin', issues=('#7',))
        fn = lambda k: {}
        with patch.object(driver, 'CASES', {}) as registry, \
                patch.object(registers, 'register', wraps=registers.register) as moved:
            self.assertIs(decorate(fn), fn)
            moved.assert_called_once_with(registry, 'late/bound', fn, mode='zhuyin', issues=('#7',))
            self.assertEqual(list(registry), ['late/bound'])
            self.assertEqual(registry['late/bound']['issues'], ('7',))
        self.assertNotIn('late/bound', driver.CASES)

    def test_per_call_worker_entry_dispatches_through_the_driver_adapter(self):
        measurement = sys.modules['contract_measurement']
        entry = HERE / 'contract-diff.py'
        arguments = ['pinyin', 'so', 'data', 'calibrate', 'helper', 'ir', 'count']
        with patch.object(sys, 'argv', [str(entry), '--per-call-worker', *arguments]), \
                patch.object(measurement, 'per_call_worker') as worker:
            runpy.run_path(str(entry), run_name='__main__')
        self.assertEqual(worker.call_args.args, tuple(arguments))
        self.assertIn('wrap-256th-reading', worker.call_args.kwargs['CASES'])

    def test_the_axis_c_cases_register_from_their_family_into_the_tables_this_driver_built(self):
        definition = sys.modules['contract_axis_c'].axis_c_definition()
        self.assertEqual(driver.AXIS_C_DEFINITION, definition)
        names = [name for name in driver.CASES if name.startswith('axis-c-')]
        self.assertCountEqual(names, ['axis-c-export-user', *sum(driver.AXIS_C_GROUPS.values(), [])])
        self.assertLessEqual(set(driver.AXIS_C_LAYOUT_COVERED), set(driver.AXIS_C_GROUPS['layouts']))

    def test_every_driver_load_builds_its_own_axis_c_tables(self):
        spec = importlib.util.spec_from_file_location('contract_diff_again', HERE / 'contract-diff.py')
        again = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(again)
        for table in ('AXIS_C_GROUPS', 'AXIS_C_LAYOUT_COVERED'):
            with self.subTest(table=table):
                self.assertIsNot(getattr(again, table), getattr(driver, table))
                self.assertEqual(getattr(again, table), getattr(driver, table))

    def test_timing_summary_totals_the_timings_the_driver_holds_now(self):
        with patch.object(driver, 'TIMINGS', [dict(execute=2, side='pin'), dict(execute=3)]):
            summary = driver.timing_summary(['a'], 0, 1)
        self.assertEqual((summary['workers'], summary['seconds']['pin_execute'], summary['seconds']['subject_execute']),
                         (2, 2, 3))


if __name__ == '__main__':
    unittest.main()
