#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Boundary checks for the modules split out of contract-diff.py (#1175).

Each helper module is a stateless library beside the driver. It binds every
global its functions read, imports nothing from the driver, and the driver
re-exports its names, so the cases, the tests and the other tools resolve them
where they always did."""

import ast
import builtins
import collections
import dis
import functools
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
        ('B', 'I', 'P', 'S', 'U', 'UNTOUCHED', 'Z', 'Arr', 'Rest', 'choose_text', 'consumed', 'final_step_sentences',
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
        ('contract_policy',)),
    'contract_policy': ((), ()),
    'contract_selection': (('SelectionError', 'case_names', 'family_of', 'normal_tag'), ()),
    'contract_registry': (('case_spec',), ('contract_policy', 'contract_selection', 'contract_verdict')),
    'contract_measurement': (
        ('PER_CALL_OPS', 'PER_CALL_NAMES', 'PER_CALL_NATIVE', 'per_call_native', 'per_call_ir',
         'per_call_profile', 'per_call_attribution', 'per_call_calibration'),
        ('contract_abi',)),
    'contract_axis_c': (
        ('axis_c_definition', 'axis_c_canonical', 'axis_c_normalize_diagnostics', 'axis_c_covered_abort',
         'AXIS_C_ABORT_ROW_FIELDS', 'AXIS_C_REFUSAL_RESULT_FIELDS', 'axis_c_live', 'axis_c_licensed_remnant',
         'axis_c_covered_residual', 'axis_c_first_difference'),
        ('contract_policy', 'contract_verdict')),
    'contract_alloc_after_fini': ((), ('contract_abi',)),
    'contract_default_options': ((), ('contract_abi',)),
    'contract_invalid_input_bytes': ((), ('contract_abi',)),
    'contract_negative_remember': ((), ('contract_abi',)),
    'contract_input_length_cap': ((), ('contract_abi',)),
    'contract_double_fini': ((), ('contract_abi',)),
    'contract_non_utf8_directories': ((), ('contract_abi',)),
    'contract_axis_j': ((), ('contract_abi',)),
    'contract_predicted_types': ((), ('contract_abi',)),
    'contract_facade_total': ((), ('contract_abi',)),
    'contract_stored_count_headers': ((), ('contract_abi', 'contract_fixtures')),
    'contract_issue542_remaining': ((), ('contract_abi',)),
    'contract_pinyin_out_parameters': ((), ('contract_abi',)),
    'contract_candidate_lookup_offsets': ((), ('contract_abi',)),
    'contract_remember_tone': ((), ('contract_abi',)),
    'contract_allocator_register': ((), ('contract_abi',)),
    'contract_cursor_sentence_refusals': ((), ('contract_abi',)),
    'contract_duplicate_phrase_add': ((), ('contract_abi', 'contract_fixtures')),
    'contract_zhuyin_facade_twins': ((), ('contract_abi',)),
    'contract_scheme_library_refusals': ((), ('contract_abi',)),
    'contract_zero_consumption': ((), ('contract_abi',)),
    'contract_row54_empty_forcing': ((), ('contract_abi',)),
    'contract_glib_arrays': (('glib_924_oom_array',), ('contract_abi',)),
    'contract_reading_256th': ((), ('contract_abi',)),
    'contract_auxiliary_text': ((), ('contract_abi',)),
    'contract_combined_import_nbest': ((), ('contract_abi',)),
    'contract_stored_total_overflow': ((), ('contract_abi', 'contract_fixtures')),
    'contract_zhuyin_per_entry': ((), ('contract_abi',)),
    'contract_case_helpers': (
        ('ADDON_PROMOTE_INPUTS', 'FACADE_TOTAL', 'LIBRARY_FACADES', 'PHRASE_TEXT_WORDS', 'PREFIX_WORDS',
         'TOKEN_REUSE_PHRASES', 'USER_ALL', 'USER_INDEX_FILES', '_DALONG', '_PAST_RANGES', '_add_user_phrase',
         '_chunk', '_dynamic_adjust_out', '_facade_total_out', '_gram_bytes', '_gram_items', '_guess_out',
         '_modify_header', '_plant_user_grams', '_reload_pron_keys', '_replace_system_start_row',
         '_replace_user_bigram', '_segment_out', '_stored_library_total', '_ucs4_key', '_user_candidate_rows',
         '_write_log', 'candidate_rows', 'd18_bigram_db', 'd18_export_phrases', 'd18_gram', 'glib_utf8',
         'guess_rows', 'log_walk', 'phrase_table_reopen', 'phrase_text_bytes', 'predicted_rows', 'remember_export',
         'row58_rows', 'token_phrase_of', 'token_unigram_trained', 'token_unigram_user', 'train_once',
         'train_selected_top', 'transient_files', 'wrap_total_to_zero', 'zhuyin_guess_rows'),
        ('contract_abi', 'contract_fixtures')),
    'contract_before_cursor_dedup': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_token_unigram_training': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_bigram_removal': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_candidate_removals': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_null_user_library': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_bigram_mask_prediction': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures', 'contract_policy', 'contract_verdict')),
    'contract_sort_pinyin_length': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_system_mask_out': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_phrase_index_logger': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_phrase_table_mask': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_punctuation': ((), ('contract_abi', 'contract_fixtures')),
    'contract_training_constraints': ((), ('contract_abi',)),
    'contract_user_stored_words': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_user_key_spellings': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_toned_keys_zhuyin_refusals': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_final_step_surface': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_zero_key_guess': ((), ('contract_abi',)),
    'contract_token_reuse_addon_import': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_system_phrase_text': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_prefix_conversion': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_facade_totals_counts': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_changed_parse_constraints': ((), ('contract_abi',)),
    'contract_stored_user_gram': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_phrase_export_training': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_user_index_phrase_mask': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_glib_path_construction': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_matrix_resplit_windows': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_library_reload': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_user_conf_overlong_index': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_row58_ranking': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_zhuyin_null_arguments': ((), ('contract_abi',)),
    'contract_save_failures_stderr': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_integer_limits_d18': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_c_string_inputs': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_gram_lookup': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_library_loading': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures', 'contract_policy', 'contract_verdict')),
    'contract_table_conf_scanner': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures', 'contract_policy', 'contract_verdict')),
    'contract_journal_replay': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_axis_c_cases': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_separator_before_cursor': ((), ('contract_abi',)),
    'contract_cell_fixtures': ((), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
    'contract_sentence_ties': (('declared_sentence_residuals', 'encode_sentence_tie'), ('contract_abi', 'contract_case_helpers', 'contract_fixtures')),
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
    'policy_diagnosis': '(spec, pin, subject)',
    'verdict_for': '(spec, pin, subject, expect_parent)',
    'short_dir_chunk': '(spec)',
    'matches': '(spec, pin, subject)',
    'case': '(name, *args, **kwargs)',
    'tag_index': '()',
    'select_cases': '(cases=None, issues=(), families=(), prefixes=None)',
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
    '_craft_db': '(path, rows, dbtype, keep=False, *, system_index=False)',
    '_craft_hash': '(path, rows)',
    'crafted_bigram_context': '(k, rows)',
    'predicted_bigram_context': '(k, phrase, value)',
    'token_reuse_sequence': '(k, steps, bigram_rows=None, library=7)',
    'token_reuse_files': '(k)',
    'addon_promote_sequence': '(k, mask_value)',
    'phrase_text_system': '(k, variant)',
    'items_system': '(k, items, total=200, *, isolated=False)',
    'regenerate_sentence_ties': '(prefix, fixture)',
    'check_axis_c_snapshots': '(args)',
    'run_axis_c': '(args, names)',
}

# Everything contract-diff.py still defines (#1175): the registry, worker entry and CLI,
# and same-signature adapters passing its per-run state to the modules beside it.
DRIVER_DEFINITIONS = (
    # registry and worker entry
    'host_oracle_platform', 'case', 'tag_index', 'select_cases', 'plan_lines', 'Kit', 'worker', 'platform_skips',
    # cell-bound fixture adapters (contract_cell_fixtures, contract_fixtures)
    '_craft_db', '_craft_hash', 'crafted_bigram_context', 'predicted_bigram_context', 'token_reuse_sequence',
    'index_file_rows', 'token_reuse_files', 'addon_promote_sequence', 'phrase_text_system', 'items_system',
    # runner adapters (contract_runner)
    '__getattr__', 'check_cancelled', 'cancellation_handlers', 'run_worker', 'run_case', 'case_jobs', 'case_results',
    # mode adapters (contract_sentence_ties, contract_axis_c, contract_measurement)
    'regenerate_sentence_ties', 'check_axis_c_snapshots', 'run_axis_c', 'PerCallKit', 'per_call_helper',
    'per_call_worker', 'per_call_run', 'per_call_checked', 'run_per_call',
    # verdict adapters (contract_verdict)
    'policy_expectations', 'policy_diagnosis', 'verdict_for', 'short_dir_chunk', 'matches',
    # command line
    'run_main', 'timing_summary', 'main',
)

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

    def test_axis_c_adapters_pass_the_registry_worker_and_tables_the_driver_holds_now(self):
        axis_c = sys.modules['contract_axis_c']
        registry, worker = {'axis-c-options-single-00000008': {}}, object()
        covered, definition = {'axis-c-layout-pinyin-9-00000000': '5c'}, {'mutations': {}}
        with patch.object(driver, 'CASES', registry), patch.object(driver, 'run_worker', worker), \
                patch.object(driver, 'AXIS_C_LAYOUT_COVERED', covered), \
                patch.object(driver, 'AXIS_C_DEFINITION', definition), \
                patch.object(axis_c, 'check_axis_c_snapshots', return_value=0) as snapshots, \
                patch.object(axis_c, 'run_axis_c', return_value=1) as battery:
            self.assertEqual(driver.check_axis_c_snapshots('args'), 0)
            snapshots.assert_called_once_with('args', run_worker=worker)
            self.assertEqual(driver.run_axis_c('args', ['n']), 1)
            battery.assert_called_once_with('args', ['n'], CASES=registry, run_worker=worker,
                                            AXIS_C_LAYOUT_COVERED=covered, AXIS_C_DEFINITION=definition)

    def test_the_axis_c_cases_register_from_their_family_into_the_tables_this_driver_built(self):
        definition = sys.modules['contract_axis_c'].axis_c_definition()
        self.assertEqual(driver.AXIS_C_DEFINITION, definition)
        names = [name for name in driver.CASES if name.startswith('axis-c-')]
        self.assertEqual(names[0], 'axis-c-export-user')
        self.assertEqual(names[1:], sum(driver.AXIS_C_GROUPS.values(), []))
        for name in names:
            with self.subTest(name=name):
                self.assertEqual(driver.CASES[name]['fn'].__module__, 'contract_axis_c_cases')
        self.assertLessEqual(set(driver.AXIS_C_LAYOUT_COVERED), set(driver.AXIS_C_GROUPS['layouts']))

    def test_every_driver_load_builds_its_own_axis_c_tables(self):
        spec = importlib.util.spec_from_file_location('contract_diff_again', HERE / 'contract-diff.py')
        again = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(again)
        for table in ('AXIS_C_GROUPS', 'AXIS_C_LAYOUT_COVERED'):
            with self.subTest(table=table):
                self.assertIsNot(getattr(again, table), getattr(driver, table))
                self.assertEqual(getattr(again, table), getattr(driver, table))

    def test_post_fini_case_is_registered_at_its_original_position(self):
        names = list(driver.CASES)
        index = names.index('alloc-instance-after-fini')
        self.assertEqual(names[index - 1:index + 2],
                         ['long-input-sentence', 'alloc-instance-after-fini', 'fini-twice'])
        self.assertEqual(driver.CASES[names[index]]['fn'].__module__, 'contract_alloc_after_fini')

    def test_moved_case_sections_register_contiguously_at_their_original_positions(self):
        names = list(driver.CASES)
        sections = (
            ('contract_default_options', 'user-key-spelling/zhuyin-eng',
             'default-option-parse', 'explicit-incomplete-parse', 'bytes-parse-more-full', 4),
            ('contract_invalid_input_bytes', 'explicit-incomplete-parse', 'bytes-parse-more-full',
             'bytes-zhuyin-add-phrase', "zero-key-guess/pinyin/default/'/guess_sentence", 7),
            ('contract_negative_remember', 'punct-alias/owned-predicted', 'remember-count-2',
             'remember-count-seven', 'input-length-cap', 5),
            ('contract_input_length_cap', 'remember-count-seven', 'input-length-cap', 'long-input-sentence',
             'alloc-instance-after-fini', 2),
            ('contract_double_fini', 'alloc-instance-after-fini', 'fini-twice', 'zhuyin-fini-twice',
             'facade-total-zero-ranks-nothing-end-sort-3', 2),
            ('contract_non_utf8_directories', 'guess-candidates-past-the-reserved-slot', 'non-utf8-directory-system',
             'non-utf8-directory-zhuyin-both', 'user-library-token-unload-pinyin', 4),
            ('contract_axis_j', 'nbest-user-table-order-zhuyin', 'j-choose-nbest-diff-result',
             'j-chewing-multi-key-zhuyin-9', 'separator/pinyin/0/parse/0', 5))
        for module, before, first, last, after, count in sections:
            with self.subTest(module=module):
                start, end = names.index(first), names.index(last)
                self.assertEqual(end - start + 1, count)
                self.assertEqual((names[start - 1], names[end + 1]), (before, after))
                owners = {inspect.unwrap(driver.CASES[name]['fn']).__module__ for name in names[start:end + 1]}
                self.assertEqual(owners, {module})
                self.assertEqual([declared[0] for declared in sys.modules[module]._REGISTRATIONS],
                                 names[start:end + 1])

    def test_next_case_sections_keep_their_positions_and_owners(self):
        names = list(driver.CASES)
        sections = (
            ('contract_predicted_types', 'predicted-unigram-overflow', 'choose-predicted-types',
             'choose-predicted-types', 'punct-decode/valid', 1),
            ('contract_facade_total', 'zhuyin-fini-twice', 'facade-total-zero-ranks-nothing-end-sort-3',
             'facade-total-zero', 'abort-set-full-pinyin-scheme-0', 4),
            ('contract_stored_count_headers', 'user-stored-words/zhuyin-7-nul', 'stored-count-headers-system-pinyin',
             'stored-count-headers-user-zhuyin', 'wrap-256th-reading', 4),
            ('contract_issue542_remaining', 'glib-924-null-array-pronunciation-zhuyin',
             'issue542-phrase-segment-invalid-utf8-pinyin', 'issue542-alias-prefix-borrowed',
             'removal-bigram-reported-mask-out-pinyin', 7),
            ('contract_pinyin_out_parameters', "zero-key-guess/zhuyin/0x18a/'''/guess_candidates_before_cursor",
             'empty-parse-sentence', 'sentence-with-prefix-bytes', 'aux-text-full-parse', 12),
            ('contract_candidate_lookup_offsets', 'abort-guess-candidates-past-matrix',
             'guess-candidates-lookup-offset', 'guess-candidates-past-the-reserved-slot', 'non-utf8-directory-system', 3),
            ('contract_remember_tone', 'third-separator-library-init-zhuyin', 'remember-tones-pinyin',
             'remember-api-absent-zhuyin', 'init-system-empty-pinyin', 2),
            ('contract_remember_tone', 'glib-922-table-conf-rejected-dup-trailing-zhuyin',
             'remember-cleared-tone-pinyin', 'remember-double-tone-pinyin', 'null-user-session-pinyin', 2),
            ('contract_allocator_register', "allocator-zero/zhuyin/xi1'an1/key/4",
             'alloc-register-pinyin_get_pinyin-zero-key', 'alloc-register-zhuyin_get_pinyin-real-key',
             'row54-empty-pinyin-full_pinyins', 14),
            ('contract_cursor_sentence_refusals', 'scheme-and-library-neighbours', 'abort-get-sentence-past-rows',
             'cursor-and-sentence-neighbours', 'abort-get-candidate-nbest-index-normal-row', 15),
            ('contract_duplicate_phrase_add', 'removed-predict-stale-skip-pinyin',
             'abort-zhuyin-add-phrase-duplicate-library-token', 'abort-add-phrase-index-text-mismatch',
             'phrase-table-mask-long-short', 6),
        )
        for module, before, first, last, after, count in sections:
            with self.subTest(module=module, first=first):
                start, end = names.index(first), names.index(last)
                self.assertEqual(end - start + 1, count)
                self.assertEqual((names[start - 1], names[end + 1]), (before, after))
                self.assertEqual({driver.CASES[name]['fn'].__module__ for name in names[start:end + 1]}, {module})

    def test_following_case_sections_keep_their_positions_and_owners(self):
        names = list(driver.CASES)
        sections = (
            ('contract_zhuyin_facade_twins', 'non-utf8-directory-zhuyin-both', 'user-library-token-unload-pinyin',
             'zhuyin-alloc-instance-after-fini', 'stderr-save-one-tmp-blocked', 11),
            ('contract_scheme_library_refusals', 'facade-total-zero', 'abort-set-full-pinyin-scheme-0',
             'scheme-and-library-neighbours', 'abort-get-sentence-past-rows', 13),
            ('contract_scheme_library_refusals', 'toned-initial-neighbours', 'abort-zhuyin-chewing-scheme-0',
             'abort-zhuyin-unload-phrase-library-255', 'abort-zhuyin-get-left-zhuyin-offset-past-matrix', 10),
            ('contract_zero_consumption', 'row54-forcing-zhuyin-no-empty', 'zero-consumption-pinyin-full_pinyins-n',
             'zero-consumption-zhuyin-chewings-empty-prefix', 'parse-constraints-pinyin-changed-suffix-direct', 24),
            ('contract_row54_empty_forcing', 'alloc-register-zhuyin_get_pinyin-real-key',
             'row54-empty-pinyin-full_pinyins', 'row54-forcing-zhuyin-no-empty', 'zero-consumption-pinyin-full_pinyins-n', 16),
            ('contract_glib_arrays', 'issue542-stale-user_phrase_index.bin-zhuyin', 'glib-924-array-lifecycle-pinyin',
             'glib-924-segment-fatal-critical-zhuyin', 'glib-924-oom-array-growth-pinyin', 6),
            ('contract_glib_arrays', 'glib-924-oom-array-growth-zhuyin', 'glib-924-null-array-lookup-pinyin',
             'glib-924-null-array-pronunciation-zhuyin', 'issue542-phrase-segment-invalid-utf8-pinyin', 4),
            ('contract_reading_256th', 'stored-count-headers-user-zhuyin', 'wrap-256th-reading',
             'wrap-256th-reading-zhuyin-same-iterator', 'zhuyin-add-phrase-library-16', 4),
            ('contract_auxiliary_text', 'sentence-with-prefix-bytes', 'aux-text-full-parse', 'aux-text-mixed-parses',
             'system-zhuyin-only-readings', 14),
            ('contract_combined_import_nbest', 'issue759-export-open-across-import-user',
             'combined-library-sixth-add-pinyin', 'nbest-user-table-order-zhuyin', 'j-choose-nbest-diff-result', 10),
            ('contract_stored_total_overflow', 'mask-out-after-compact-keeps-reload-offset',
             'stored-total-overflow-below', 'log-headers-chained-both-apply', 'user-gram-unsorted-guess', 6),
            ('contract_zhuyin_per_entry', 'guess-candidates-short-user-gram-dynamic-adjust-off',
             'zhuyin-parse-chewing-key', 'zhuyin-token-get-phrase-optional-out', 'phrase-text/pinyin-valid-candidate', 16),
        )
        for module, before, first, last, after, count in sections:
            with self.subTest(module=module, first=first):
                start, end = names.index(first), names.index(last)
                self.assertEqual(end - start + 1, count)
                self.assertEqual((names[start - 1], names[end + 1]), (before, after))
                self.assertEqual({driver.CASES[name]['fn'].__module__ for name in names[start:end + 1]}, {module})

    def test_oom_cases_register_from_the_glib_family_in_their_original_order(self):
        names = list(driver.CASES)
        groups = (
            ('glib-924-segment-fatal-critical-zhuyin', 'glib-924-oom-array-growth-pinyin',
             'glib-924-oom-array-growth-zhuyin', 'glib-924-null-array-lookup-pinyin', 2, driver.glib_924_oom_array.__code__),
        )
        for before, first, last, after, count, helper_code in groups:
            with self.subTest(first=first):
                start, end = names.index(first), names.index(last)
                self.assertEqual(end - start + 1, count)
                self.assertEqual((names[start - 1], names[end + 1]), (before, after))
                helper_codes = {helper_code, *(c for c in helper_code.co_consts if isinstance(c, types.CodeType))}
                for name in names[start:end + 1]:
                    fn = driver.CASES[name]['fn']
                    self.assertEqual(fn.__module__, 'contract_glib_arrays')
                    self.assertIn(fn.__code__, helper_codes)
        self.assertIs(driver.glib_924_oom_array, sys.modules['contract_glib_arrays'].glib_924_oom_array)

    def test_shared_oom_registration_receives_the_current_driver_function(self):
        glib = sys.modules['contract_glib_arrays']
        with patch.object(glib, 'register_oom', wraps=glib.register_oom) as oom:
            spec = importlib.util.spec_from_file_location('contract_diff', HERE / 'contract-diff.py')
            fresh = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(fresh)
        self.assertIs(oom.call_args.kwargs['glib_924_oom_array'], fresh.glib_924_oom_array)

    def test_shared_helpers_are_reexported_by_assignment_so_driver_code_compiles_unchanged(self):
        tree = ast.parse((HERE / 'contract-diff.py').read_text())
        imported = {alias.asname or alias.name for node in tree.body if isinstance(node, ast.ImportFrom)
                    and node.module == 'contract_case_helpers' for alias in node.names}
        assigned = {node.targets[0].id for node in tree.body if isinstance(node, ast.Assign)
                    and isinstance(node.value, ast.Attribute) and isinstance(node.value.value, ast.Name)
                    and node.value.value.id == 'contract_case_helpers'
                    and node.value.attr == node.targets[0].id}
        self.assertEqual(imported, set())
        self.assertEqual(assigned, set(MODULES['contract_case_helpers'][0]))

    def test_cell_and_tie_adapters_pass_the_driver_state_they_hold_at_call_time(self):
        state = {'_craft_db': ('CELL', '_craft_bdb', '_craft_kc', '_craft_tkrzw'), '_craft_hash': ('_craft_db',),
                 'crafted_bigram_context': ('_craft_hash',), 'predicted_bigram_context': ('_craft_hash',),
                 'token_reuse_sequence': ('crafted_bigram_context', 'token_reuse_files'),
                 'token_reuse_files': ('CELL', 'index_file_rows'), 'addon_promote_sequence': ('token_reuse_files',),
                 'phrase_text_system': ('items_system',), 'items_system': ('_craft_db',),
                 'regenerate_sentence_ties': ('Kit',)}
        for name, names in state.items():
            module = sys.modules['contract_sentence_ties' if name == 'regenerate_sentence_ties' else 'contract_cell_fixtures']
            adapter = getattr(driver, name)
            params = [p for p in inspect.signature(adapter).parameters.values()]
            positional = [object() for p in params if p.default is inspect.Parameter.empty
                          and p.kind is p.POSITIONAL_OR_KEYWORD]
            current = {n: object() for n in names}
            with self.subTest(name=name), patch.object(module, name, return_value='done') as moved, \
                    patch.multiple(driver, **current):
                self.assertEqual(adapter(*positional), 'done')
                self.assertEqual({key: moved.call_args.kwargs[key] for key in names}, current)
                self.assertEqual(moved.call_args.args[:len(positional)], tuple(positional))

    def test_no_case_function_is_defined_in_the_driver(self):
        owners = collections.Counter()
        for spec in driver.CASES.values():
            fn = spec['fn']
            while isinstance(fn, functools.partial):
                fn = fn.func
            owners[fn.__module__] += 1
        self.assertNotIn(driver.__name__, owners)
        self.assertEqual(sum(owners.values()), len(driver.CASES))

    def test_the_driver_defines_only_wiring_adapters_and_the_cli(self):
        tree = ast.parse((HERE / 'contract-diff.py').read_text())
        defined = sorted(node.name for node in tree.body if isinstance(node, (ast.FunctionDef, ast.ClassDef)))
        self.assertEqual(defined, sorted(DRIVER_DEFINITIONS))

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
