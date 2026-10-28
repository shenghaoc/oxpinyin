#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Boundary checks for the modules split out of contract-diff.py (#1175).

Each helper module is a stateless library beside the driver. It binds every
global its functions read, imports nothing from the driver, and the driver
re-exports its names, so the cases, the tests and the other tools resolve them
where they always did."""

import builtins
import dis
import importlib.util
import inspect
import json
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
for path in (str(ROOT), str(HERE)):
    if path not in sys.path:
        sys.path.insert(0, path)

from tools.dev import test_runner

# Each module, the names the driver must keep resolving to that module's objects,
# and the other helper modules it may import.
MODULES = {
    'contract_abi': (
        ('B', 'I', 'P', 'S', 'U', 'UNTOUCHED', 'Z', 'Arr', 'choose_text', 'consumed', 'final_step_sentences',
         'full_inst', 'guessed', 'key_out', 'owned_text', 'row_of', 'row_with_text', 'sentence_out', 'tokens_of',
         'unigram_of', 'zhuyin_chewing', 'zhuyin_sentence_out'),
        ()),
    'contract_fixtures': (
        ('_craft_bdb', '_craft_kc', '_craft_tkrzw', '_write_user_conf', 'conf_system', 'import_rows',
         'private_system', 'row58_import', 'user_listing', 'zero_item_padding'),
        ('contract_abi',)),
    'contract_verdict': (
        ('META_OFFSET_TIMESTAMP', 'NO_ABORT', 'TKRZW_HASH_METADATA_SIZE', 'TKRZW_TIMESTAMP_WIDTH', 'WARNING_DOMAIN',
         '_FATAL_GLIB_PREFIX', 'abort_answered', 'directory_reads_as_a_chunk', 'fatal_stderr', 'host_scanf_accepts',
         'observed', 'pin_loaded_subject_refused_table_conf', 'policy_observation', 'registered_termination',
         'strjoinv_fault', 'tkrzw_saved_bigram_bytes', 'tkrzw_saved_bigram_equal', 'valid_observation'),
        ()),
    'contract_selection': (('SelectionError', 'case_names', 'family_of', 'normal_tag'), ()),
    'contract_registry': (('case_spec',), ('contract_selection', 'contract_verdict')),
    'contract_measurement': (
        ('PER_CALL_OPS', 'PER_CALL_NAMES', 'PER_CALL_NATIVE', 'per_call_native', 'per_call_ir',
         'per_call_profile', 'per_call_attribution', 'per_call_calibration'),
        ('contract_abi',)),
    'contract_execution': (('ordinary_jobs', 'positive_jobs', 'terminate_group', 'user_dir_listing'), ()),
    'contract_runner': (('Cancelled',), ()),
    'contract_kit': ((), ('contract_abi',)),
}

# Driver functions that stay as adapters: the original signature, kept for every caller,
# and the state they hand the module from the driver's own globals.
ADAPTERS = {
    'Kit': '(mode, so, data, scratch)',
    'index_file_rows': "(path, *, kind='tree', allow_empty=False, library=None)",
    'policy_expectations': '(spec)',
    'verdict_for': '(spec, pin, subject, expect_parent)',
    'short_dir_chunk': '(spec)',
    'matches': '(spec, pin, subject)',
    'case': '(name, *args, **kwargs)',
    'tag_index': '()',
    'select_cases': '(cases=None, issues=(), families=())',
    'plan_lines': '(names, reasons, pinyin_so, zhuyin_so, prefix, cell)',
    'timing_summary': '(names, compare_seconds, total)',
    'check_cancelled': '()',
    'cancellation_handlers': '()',
    'run_worker': '(mode, so, data, name, scratch, extra_env=None, timeout=None, entry=None)',
    'run_case': '(spec, pin_so, subject_so, data, name, scratch, timeout)',
    'case_jobs': '(requested)',
    'case_results': '(args, names, data, scratch, jobs)',
    'PerCallKit': '(mode, so, data, scratch, native, operations)',
    'per_call_helper': '(scratch)',
    'per_call_worker': '(mode, so, data, name, library, metric, operations)',
    'per_call_run': '(mode, so, data, name, helper, metric, operations, scratch, cpu, attribution=False)',
    'per_call_checked': '(spec, reference, sample, operations)',
    'run_per_call': '(args, parser)',
}

spec = importlib.util.spec_from_file_location('contract_diff', HERE / 'contract-diff.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


def code_objects(module):
    """The code object of every function and method the module defines, nested ones included."""
    def nested(code):
        yield code
        for constant in code.co_consts:
            if isinstance(constant, types.CodeType):
                yield from nested(constant)

    for name, value in vars(module).items():
        value = inspect.unwrap(value) if callable(value) else value
        if isinstance(value, types.FunctionType) and value.__module__ == module.__name__:
            yield from nested(value.__code__)
        elif isinstance(value, type) and value.__module__ == module.__name__:
            for member in vars(value).values():
                member = inspect.unwrap(getattr(member, '__func__', member))
                if isinstance(member, types.FunctionType):
                    yield from nested(member.__code__)


def unbound_globals(module):
    """Names the module's functions read from their globals that neither it nor builtins bind."""
    read = {instruction.argval for code in code_objects(module) for instruction in dis.get_instructions(code)
            if instruction.opname == 'LOAD_GLOBAL'}
    return sorted(name for name in read if not hasattr(builtins, name) and name not in vars(module))


class ModuleBoundaryTests(unittest.TestCase):
    def test_the_scan_checks_the_underlying_decorated_method_not_the_library_wrapper(self):
        probe = types.ModuleType('decorated_probe')
        exec('import contextlib\n\nclass Holder:\n'
             '    @contextlib.contextmanager\n'
             '    def scope(self):\n        yield MISSING_DRIVER_STATE\n', probe.__dict__)
        self.assertEqual(unbound_globals(probe), ['MISSING_DRIVER_STATE'])

    def test_the_scan_names_a_global_that_only_the_driver_binds(self):
        probe = types.ModuleType('probe')
        exec('import os\n\n\ndef reads():\n    return os.sep, CELL, len\n\n\n'
             'class Holder:\n    def method(self):\n        return [WORKER for _ in ()]\n', probe.__dict__)
        self.assertEqual(unbound_globals(probe), ['CELL', 'WORKER'])

    def test_every_global_a_function_reads_is_bound_in_its_own_module(self):
        for name in MODULES:
            with self.subTest(module=name):
                self.assertEqual(unbound_globals(sys.modules[name]), [])

    def test_the_driver_resolves_each_exported_name_to_the_module_object(self):
        for name, (exports, _) in MODULES.items():
            module = sys.modules[name]
            for export in exports:
                with self.subTest(module=name, export=export):
                    self.assertIs(getattr(driver, export), getattr(module, export))

    def test_a_module_loads_alone_and_imports_only_its_declared_peers(self):
        for name, (_, peers) in MODULES.items():
            with self.subTest(module=name), tempfile.TemporaryDirectory() as elsewhere:
                code = ('import json, sys; sys.path.insert(0, %r); import %s; '
                        'print(json.dumps(sorted(m for m in sys.modules if m.startswith("contract"))))'
                        % (str(HERE), name))
                result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', code], cwd=elsewhere,
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), sorted((name, *peers)))

    def test_adapters_keep_the_signatures_their_callers_use(self):
        for name, signature in ADAPTERS.items():
            with self.subTest(name=name):
                self.assertEqual(str(inspect.signature(getattr(driver, name))), signature)

    def test_index_file_rows_reads_the_cell_from_the_driver_at_call_time(self):
        for cell in ('bdb', 'kc', 'tkrzw'):
            with self.subTest(cell=cell), patch.object(driver, 'CELL', cell), \
                    patch.object(sys.modules['contract_fixtures'], 'index_file_rows', return_value='rows') as moved:
                self.assertEqual(driver.index_file_rows('p', kind='hash', allow_empty=True, library='L'), 'rows')
                moved.assert_called_once_with('p', cell=cell, kind='hash', allow_empty=True, library='L')

    def test_verdict_adapters_pass_the_findings_directory_cell_and_probe_the_driver_holds_now(self):
        verdicts = sys.modules['contract_verdict']
        probe = lambda: False
        case, pin, subject = {'case': 1}, {'side': 'pin'}, {'side': 'subject'}
        state = dict(policy_root=Path('/findings'), cell='kc', directory_probe=probe)
        calls = (('policy_expectations', (case,), dict(policy_root=state['policy_root'], cell='kc')),
                 ('verdict_for', (case, pin, subject, True), state),
                 ('short_dir_chunk', (case,), dict(directory_probe=probe)),
                 ('matches', (case, pin, subject), dict(directory_probe=probe)))
        with patch.object(driver, 'POLICY_ROOT', Path('/findings')), patch.object(driver, 'CELL', 'kc'), \
                patch.object(driver, 'directory_reads_as_a_chunk', probe):
            for name, arguments, keywords in calls:
                with self.subTest(name=name), patch.object(verdicts, name, return_value=name) as moved:
                    self.assertEqual(getattr(driver, name)(*arguments), name)
                    moved.assert_called_once_with(*arguments, **keywords)

    def test_selection_adapters_select_from_the_registry_the_driver_holds_now(self):
        registry = {'only/one': dict(mode='zhuyin', cells=('kc',), issues=('7',))}
        with patch.object(driver, 'CASES', registry):
            self.assertEqual(driver.select_cases(), (['only/one'], {'only/one': ['default suite']}))
            self.assertEqual(driver.select_cases(issues=['#7'])[0], ['only/one'])
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

    def test_measurement_adapters_pass_current_driver_state_and_keep_the_kit_patch_point(self):
        measurement = sys.modules['contract_measurement']
        registry, ordering, native_source = {}, {'check_arms': object()}, 'source'
        callbacks = {name: object() for name in ('check_tkrzw', 'check_cancelled', 'terminate_group',
                                                  'user_dir_listing', 'per_call_ir', 'per_call_profile',
                                                  'verdict_for', 'per_call_native', 'per_call_helper',
                                                  'per_call_run', 'per_call_checked', 'per_call_calibration',
                                                  'per_call_attribution', 'observed')}
        self.assertTrue(issubclass(driver.PerCallKit, driver.Kit))
        self.assertTrue(issubclass(driver.PerCallKit, measurement.PerCallKit))
        with patch.object(driver, 'CASES', registry), patch.object(driver, 'CELL', 'kc'), \
                patch.object(driver, 'FUNCTION_ORDER', ordering), patch.object(driver, 'PER_CALL_NATIVE', native_source), \
                patch.object(measurement, 'per_call_helper', return_value='helper') as helper:
            self.assertEqual(driver.per_call_helper('scratch'), 'helper')
            helper.assert_called_once_with('scratch', PER_CALL_NATIVE=native_source)
            with patch.multiple(driver, **{key: value for key, value in callbacks.items()
                                           if key not in ('per_call_helper', 'per_call_run', 'per_call_checked')}), \
                    patch.object(measurement, 'per_call_worker', return_value='worker') as worker, \
                    patch.object(measurement, 'per_call_run', return_value='sample') as sample, \
                    patch.object(measurement, 'per_call_checked', return_value='checked') as checked, \
                    patch.object(measurement, 'run_per_call', return_value='controller') as controller:
                self.assertEqual(driver.per_call_worker('p', 'so', 'data', 'name', 'lib', 'ir', 'add'), 'worker')
                worker.assert_called_once_with('p', 'so', 'data', 'name', 'lib', 'ir', 'add',
                                               PerCallKit=driver.PerCallKit, Kit=driver.Kit, CASES=registry,
                                               per_call_native=callbacks['per_call_native'])
                self.assertEqual(driver.per_call_run('p', 'so', 'data', 'name', 'helper', 'wall',
                                                     'add', 'scratch', 2, True), 'sample')
                sample.assert_called_once_with('p', 'so', 'data', 'name', 'helper', 'wall', 'add', 'scratch',
                                               2, True, driver_file=driver.__file__, CELL='kc',
                                               **{key: callbacks[key] for key in ('check_tkrzw', 'check_cancelled',
                                                                                 'terminate_group', 'user_dir_listing',
                                                                                 'per_call_ir', 'per_call_profile')})
                self.assertEqual(driver.per_call_checked('spec', 'ref', 'sample', ('add',)), 'checked')
                checked.assert_called_once_with('spec', 'ref', 'sample', ('add',), verdict_for=callbacks['verdict_for'])
                self.assertEqual(driver.run_per_call('args', 'parser'), 'controller')
                controller.assert_called_once_with('args', 'parser', CASES=registry, FUNCTION_ORDER=ordering,
                                                   per_call_helper=driver.per_call_helper, per_call_run=driver.per_call_run,
                                                   per_call_checked=driver.per_call_checked,
                                                   per_call_calibration=callbacks['per_call_calibration'],
                                                   per_call_attribution=callbacks['per_call_attribution'],
                                                   observed=callbacks['observed'])

    def test_per_call_worker_entry_dispatches_through_the_driver_adapter(self):
        measurement = sys.modules['contract_measurement']
        entry = HERE / 'contract-diff.py'
        arguments = ['pinyin', 'so', 'data', 'calibrate', 'helper', 'ir', 'count']
        with patch.object(sys, 'argv', [str(entry), '--per-call-worker', *arguments]), \
                patch.object(measurement, 'per_call_worker') as worker:
            runpy.run_path(str(entry), run_name='__main__')
        self.assertEqual(worker.call_args.args, tuple(arguments))
        self.assertIn('wrap-256th-reading', worker.call_args.kwargs['CASES'])
        self.assertTrue(issubclass(worker.call_args.kwargs['PerCallKit'], worker.call_args.kwargs['Kit']))

    def test_timing_summary_totals_the_timings_the_driver_holds_now(self):
        with patch.object(driver, 'TIMINGS', [dict(execute=2, side='pin'), dict(execute=3)]):
            summary = driver.timing_summary(['a'], 0, 1)
        self.assertEqual((summary['workers'], summary['seconds']['pin_execute'], summary['seconds']['subject_execute']),
                         (2, 2, 3))

    def test_changed_runs_map_each_module_to_its_tests_and_the_driver_tests(self):
        files = set(test_runner.harness_files())
        for name in MODULES:
            path = f'tools/bisection/{name}.py'
            with self.subTest(path=path):
                mapped = test_runner.HARNESS_DEPENDENCIES.get(path)
                self.assertIsNotNone(mapped, 'an unmapped path makes --changed run every harness')
                self.assertLessEqual(set(mapped), files)
                self.assertLessEqual({f'tools/bisection/{name}.test.py', 'tools/bisection/contract-modules.test.py',
                                      'tools/bisection/contract-diff.test.py'}, set(mapped))


if __name__ == '__main__':
    unittest.main()
