#!/usr/bin/env python3
"""Regressions for the contract differential's fatal-log predicate."""

import copy
import json
import runpy
import signal
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

contract = runpy.run_path(str(Path(__file__).with_name('contract-diff.py')))


class FatalLogTests(unittest.TestCase):
    def setUp(self):
        self.spec = contract['CASES']['save-fatal-valid-to-invalid']
        self.pin = {
            'exit': -signal.SIGTRAP,
            'result': None,
            'stderr': 'open SCRATCH/user-X/user.conf failed.\n'
            '** (process:123): CRITICAL **: 16:44:30.526: assertion failed\n',
            'userfiles': {'gb_char.dbin.tmp': [26, 'digest']},
        }
        self.subject = copy.deepcopy(self.pin)
        self.subject['stderr'] = (
            self.pin['stderr'].replace('123', '456').replace('30.526', '31.045')
        )

    def matches(self):
        return contract['matches'](self.spec, self.pin, self.subject)

    def test_same_glib_fatal_signal(self):
        for sig in (signal.SIGABRT, signal.SIGTRAP):
            with self.subTest(signal=sig):
                self.pin['exit'] = self.subject['exit'] = -sig
                self.assertTrue(self.matches())

    def test_different_fatal_signals(self):
        self.subject['exit'] = -signal.SIGABRT
        self.assertFalse(self.matches())

    def test_other_equal_exits_are_not_fatal_logs(self):
        for status in (0, 1, -signal.SIGSEGV, -signal.SIGILL, -signal.SIGTERM):
            with self.subTest(status=status):
                self.pin['exit'] = self.subject['exit'] = status
                self.assertFalse(self.matches())

    def test_message_difference_is_preserved(self):
        self.subject['stderr'] = self.subject['stderr'].replace(
            'assertion failed', 'different assertion'
        )
        self.assertFalse(self.matches())

    def test_severity_difference_is_preserved(self):
        self.subject['stderr'] = self.subject['stderr'].replace('CRITICAL', 'WARNING')
        self.assertFalse(self.matches())

    def test_partial_profile_difference_is_preserved(self):
        self.subject['userfiles']['gb_char.dbin.tmp'][1] = 'different digest'
        self.assertFalse(self.matches())


class DiagnosticPathTests(unittest.TestCase):
    def observations(self, second_message='failed'):
        values = []
        with tempfile.TemporaryDirectory(
            prefix='ox886-home-user-path-sys-root-'
        ) as root:
            for message in ('failed', second_message):
                with tempfile.TemporaryDirectory(prefix='worker-', dir=root) as scratch:
                    text = f'open {scratch}/user-generated/user.conf {message}.'
                    result = {
                        'log_messages': [text],
                        'reopen_stderr': text,
                        'payload': 'literal user-input and sys-input',
                    }
                    process = SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(result).encode(),
                        stderr=text.encode(),
                    )
                    with patch.object(
                        contract['subprocess'], 'run', return_value=process
                    ):
                        value = contract['run_worker'](
                            'pinyin', '/unused', '/unused', 'case', scratch
                        )
                    values.append(
                        contract['axis_c_normalize_diagnostics'](
                            {key: value[key] for key in ('exit', 'result', 'stderr')},
                            scratch,
                        )
                    )
        return values

    def test_generated_worker_paths_normalize_before_user_sys_names(self):
        first, second = self.observations()
        self.assertEqual(first, second)
        expected = 'open SCRATCH/user-X/user.conf failed.'
        self.assertEqual(first['stderr'], expected)
        self.assertEqual(first['result']['reopen_stderr'], expected)
        self.assertEqual(first['result']['log_messages'], [expected])
        self.assertEqual(first['result']['payload'], 'literal user-input and sys-input')

    def test_real_message_difference_survives_path_normalization(self):
        first, second = self.observations('refused')
        self.assertNotEqual(first, second)
        self.assertNotEqual(first['stderr'], second['stderr'])
        for field in ('reopen_stderr', 'log_messages'):
            self.assertNotEqual(first['result'][field], second['result'][field])


def observation(exit=0, result=None, stderr='', userfiles=None):
    return dict(exit=exit, result=result, stderr=stderr, stderr_lines=len(stderr.splitlines()),
                userfiles=userfiles if userfiles is not None else {})


class VerdictTests(unittest.TestCase):
    """verdict_for: a difference needs two valid observations; a registered
    termination keeps the matcher's own semantics."""

    def plain(self, control=False):
        return dict(contract['CASES']['default-option-parse'], control=control)

    def decide(self, spec, pin, subject, parent):
        return contract['verdict_for'](spec, pin, subject, parent)

    def test_valid_equal(self):
        pin, subject = observation(result={'ret': 1}), observation(result={'ret': 1})
        self.assertEqual(self.decide(self.plain(), pin, subject, False), ('MATCH', True, True))
        self.assertEqual(self.decide(self.plain(), pin, subject, True), ('MATCH', False, True))
        self.assertEqual(self.decide(self.plain(control=True), pin, subject, True), ('MATCH', True, True))

    def test_valid_different(self):
        pin, subject = observation(result={'ret': 1}), observation(result={'ret': 2})
        self.assertEqual(self.decide(self.plain(), pin, subject, False), ('DIFFER', False, False))
        self.assertEqual(self.decide(self.plain(), pin, subject, True), ('DIFFER', True, False))
        self.assertEqual(self.decide(self.plain(control=True), pin, subject, True), ('DIFFER', False, False))

    def test_invalid_either_side_fails_in_both_modes(self):
        good = observation(result={'ret': 1})
        for bad in (observation(exit=1), observation(exit=-signal.SIGSEGV), observation(exit=0, result=None),
                    observation(exit='TIMEOUT'), observation(exit=1, result={'ret': 1})):
            for pin, subject in ((bad, good), (good, bad), (bad, bad)):
                for parent in (False, True):
                    for control in (False, True):
                        with self.subTest(bad=bad['exit'], pin=pin is bad, parent=parent, control=control):
                            verdict, expected, same = self.decide(self.plain(control), pin, subject, parent)
                            self.assertEqual((verdict, expected, same), ('INVALID', False, False))

    def test_two_identical_failures_are_not_an_expected_difference(self):
        dead = observation(exit=-signal.SIGABRT)
        self.assertEqual(self.decide(self.plain(), dead, copy.deepcopy(dead), True), ('INVALID', False, False))

    def abort_spec(self, **overrides):
        spec = dict(self.plain(), abort=False, **overrides)
        return spec

    def test_abort_semantics_unchanged(self):
        spec = self.abort_spec()
        pin = observation(exit=-signal.SIGABRT)
        subject = observation(result={'ret': False, 'logs': [['libpinyin', 16]]})
        self.assertEqual(self.decide(spec, pin, subject, False), ('MATCH', True, True))
        # A parent that still aborts like the pin is the expected difference.
        parent = observation(exit=-signal.SIGABRT)
        self.assertEqual(self.decide(spec, pin, parent, True), ('DIFFER', True, False))
        self.assertEqual(self.decide(spec, pin, parent, False), ('DIFFER', False, False))
        # The subject surviving with the wrong value is a plain DIFFER.
        wrong = observation(result={'ret': True, 'logs': [['libpinyin', 16]]})
        self.assertEqual(self.decide(spec, pin, wrong, False), ('DIFFER', False, False))

    def test_crash_semantics_unchanged(self):
        spec = dict(self.plain(), crash={'ret': False}, stderr=True)
        pin = observation(exit=-signal.SIGSEGV, stderr='x\n')
        subject = observation(result={'ret': False}, stderr='x\n')
        self.assertEqual(self.decide(spec, pin, subject, False), ('MATCH', True, True))
        crashed_parent = observation(exit=-signal.SIGSEGV, stderr='x\n')
        self.assertEqual(self.decide(spec, pin, crashed_parent, True), ('DIFFER', True, False))
        self.assertEqual(self.decide(spec, pin, crashed_parent, False), ('DIFFER', False, False))

    def test_either_semantics_unchanged(self):
        spec = dict(self.plain(), abort=False, crash={'ret': False}, either=True, stderr=True)
        subject = observation(result={'ret': False, 'logs': [['libpinyin', 16]]}, stderr='')
        self.assertEqual(self.decide(spec, observation(exit=-signal.SIGABRT), subject, False)[0], 'MATCH')
        crash_subject = observation(result={'ret': False}, stderr='x\n')
        self.assertEqual(self.decide(spec, observation(exit=-signal.SIGSEGV, stderr='x\n'), crash_subject, False)[0],
                         'MATCH')

    def test_both_abort_semantics_unchanged(self):
        spec = contract['CASES']['save-fatal-valid-to-invalid']
        stderr = 'open SCRATCH/user-X/user.conf failed.\n** (process:1): CRITICAL **: 00:00:00.000: boom\n'
        pin = observation(exit=-signal.SIGTRAP, stderr=stderr, userfiles={'a': [1, 'd']})
        subject = observation(exit=-signal.SIGTRAP, stderr=stderr.replace(':1)', ':2)'), userfiles={'a': [1, 'd']})
        self.assertEqual(self.decide(spec, pin, subject, False), ('MATCH', True, True))
        survivor = observation(result={'ret': True}, stderr=stderr, userfiles={'a': [1, 'd']})
        self.assertEqual(self.decide(spec, pin, survivor, True), ('DIFFER', True, False))
        self.assertEqual(self.decide(spec, pin, survivor, False), ('DIFFER', False, False))


class DriverInvalidTests(unittest.TestCase):
    """main(): two failed workers under --expect-parent exit 1 with INVALID."""

    def invoke(self, parent, workers):
        import contextlib
        import io
        with tempfile.TemporaryDirectory() as scratch:
            prefix = Path(scratch)
            (prefix / 'lib/libpinyin/data').mkdir(parents=True)
            (prefix / 'lib/libpinyin.so').touch()
            (prefix / 'lib/libpinyin/data/bigram.db').touch()
            (prefix / 'oracle-pin.txt').write_text(
                'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+dbm-bdb\n')
            argv = ['driver', 'bdb', str(prefix), str(prefix / 'lib/libpinyin.so'),
                    '--cases', 'default-option-parse'] + (['--expect-parent'] if parent else [])
            output = io.StringIO()
            with patch.object(contract['sys'], 'argv', argv), \
                    patch.dict(contract['main'].__globals__, run_worker=lambda *a, **k: workers.pop(0)), \
                    contextlib.redirect_stdout(output):
                status = contract['main']()
        # The case rows; the run's trailing timing line is not one.
        rows = [row for row in (json.loads(line) for line in output.getvalue().splitlines()
                                if line.startswith('{')) if 'case' in row]
        return status, rows

    def test_two_failed_workers_fail_under_expect_parent(self):
        status, rows = self.invoke(True, [observation(exit=1), observation(exit=1)])
        self.assertEqual(status, 1)
        self.assertEqual([(r['verdict'], r['expected_ok'], r['exit']) for r in rows], [('INVALID', False, [1, 1])])

    def test_two_failed_workers_fail_in_normal_mode(self):
        status, rows = self.invoke(False, [observation(exit=-signal.SIGSEGV), observation(exit=0)])
        self.assertEqual(status, 1)
        self.assertEqual(rows[0]['verdict'], 'INVALID')

    def test_valid_parent_difference_still_passes(self):
        status, rows = self.invoke(True, [observation(result={'a': 1}), observation(result={'a': 2})])
        self.assertEqual((status, rows[0]['verdict'], rows[0]['expected_ok']), (0, 'DIFFER', True))


class WorkerEntryTests(unittest.TestCase):
    """Workers start from contract-worker.py, which loads the same driver."""

    def test_run_worker_starts_the_cached_entry(self):
        process = SimpleNamespace(returncode=0, stdout=b'{}\n', stderr=b'')
        with tempfile.TemporaryDirectory() as scratch, \
                patch.object(contract['subprocess'], 'run', return_value=process) as run:
            contract['run_worker']('pinyin', '/so', '/data', 'case', scratch)
        command = run.call_args.args[0]
        self.assertEqual(command[1:], [str(contract['WORKER']), 'pinyin', '/so', '/data', 'case'])
        self.assertEqual(Path(command[1]).name, 'contract-worker.py')
        self.assertTrue(Path(command[1]).is_file())

    def test_cached_entry_loads_the_same_registry(self):
        import importlib.util
        entry = Path(__file__).with_name('contract-worker.py')
        spec = importlib.util.spec_from_file_location(
            'contract_diff', entry.with_name('contract-diff.py'))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self.assertEqual(sorted(module.CASES), sorted(contract['CASES']))
        self.assertEqual({n: {k: v for k, v in s.items() if k != 'fn'} for n, s in module.CASES.items()},
                         {n: {k: v for k, v in s.items() if k != 'fn' and k != 'abort'} | (
                             {'abort': s['abort']} if s['abort'] is not contract['NO_ABORT'] else {'abort': module.NO_ABORT})
                          for n, s in contract['CASES'].items()})
        self.assertEqual(module.worker.__code__.co_code, contract['worker'].__code__.co_code)
        source = entry.read_text()
        self.assertIn("_module.worker(*sys.argv[1:])", source)


class SelectionTests(unittest.TestCase):
    """--cases, --issue and --family resolve to exact case IDs; nothing falls
    back to the default suite."""

    def test_tags_normalise(self):
        for tag in ('#640', '640', ' 640 '):
            self.assertEqual(contract['normal_tag'](tag), '640')
        for tag in ('row:54', 'Row 54', 'row54', 'ROW:54'):
            self.assertEqual(contract['normal_tag'](tag), 'row:54')
        self.assertEqual(contract['normal_tag']('W11'), 'w11')

    def test_issue_selects_registered_cases_only(self):
        names, reasons = contract['select_cases'](issues=['#532'])
        self.assertEqual(names, ['default-option-parse', 'default-option-single-key',
                                 'explicit-incomplete-parse', 'explicit-use-tone-parse'])
        self.assertEqual(reasons['default-option-parse'], ['issue 532'])
        for name in names:
            self.assertIn('532', contract['CASES'][name]['issues'])
        self.assertFalse(any(name.startswith('axis-c-') for name in contract['tag_index']().get('640', [])))

    def test_family_selects_by_id_prefix(self):
        names, reasons = contract['select_cases'](families=['zero-key'])
        self.assertTrue(names and all(name.startswith('zero-key/') for name in names))
        self.assertEqual(set(names), {n for n in contract['CASES'] if n.startswith('zero-key/')})
        self.assertEqual(reasons[names[0]], ['family zero-key'])

    def test_explicit_cases_keep_their_order_and_combine(self):
        names, reasons = contract['select_cases']('fini-twice,default-option-parse', issues=['532'])
        self.assertEqual(names[:2], ['fini-twice', 'default-option-parse'])
        self.assertEqual(reasons['default-option-parse'], ['--cases', 'issue 532'])

    def test_unknown_or_empty_selection_is_an_error_naming_what_exists(self):
        with self.assertRaises(contract['SelectionError']) as caught:
            contract['select_cases'](issues=['9999'])
        self.assertIn('registered tags: 525', str(caught.exception))
        self.assertNotIn('family/', str(caught.exception))
        with self.assertRaises(contract['SelectionError']) as caught:
            contract['select_cases'](families=['nope'])
        self.assertIn('registered families: allocator-zero', str(caught.exception))
        with self.assertRaises(contract['SelectionError']):
            contract['select_cases']('nope')
        with self.assertRaises(contract['SelectionError']):
            contract['select_cases'](',')

    def test_default_suite_only_without_selectors(self):
        names, reasons = contract['select_cases']()
        self.assertEqual(names, sorted(n for n in contract['CASES'] if not n.startswith('axis-c-')))
        self.assertEqual(set(map(tuple, reasons.values())), {('default suite',)})

    def test_plan_lists_ids_reasons_facades_and_artifacts(self):
        names, reasons = contract['select_cases'](issues=['532'], families=['zero-key'])
        lines = contract['plan_lines'](names, reasons, Path('/s/libpinyin_capi.so'), Path('/s/libzhuyin_capi.so'),
                                       Path('/pin'), 'bdb')
        self.assertTrue(lines[0].startswith('plan: %d cases; cell bdb; facades: pinyin ' % len(names)))
        self.assertIn('zhuyin ', lines[0])
        self.assertIn('subject /s/libzhuyin_capi.so', lines[1])
        self.assertIn('pin /pin/lib/libzhuyin.so', lines[1])
        body = lines[2:]
        self.assertEqual(len(body), len(names))
        self.assertTrue(any(line.split()[0] == 'default-option-parse' and 'issue 532' in line for line in body))
        self.assertTrue(any(line.split()[0].startswith('zero-key/') and 'family zero-key' in line for line in body))
        default_lines = contract['plan_lines'](*contract['select_cases'](), Path('/s'), None, Path('/pin'), 'bdb')
        self.assertIn('default suite', default_lines[0])
        # A count, the artifacts and at most the skip line: no per-case lines.
        self.assertLessEqual(len(default_lines), 3)
        self.assertFalse(any(line.startswith('  ') and ' pinyin ' in line for line in default_lines))


class TimingTests(unittest.TestCase):
    """The worker's `T` line feeds TIMINGS and never the observation."""

    def test_timing_line_is_kept_apart_from_the_observation(self):
        stdout = b'T {"startup": 0.1, "load": 0.2, "fixture": 0.3, "execute": 0.4}\n{"ret": true}\n'
        process = SimpleNamespace(returncode=0, stdout=stdout, stderr=b'')
        before = len(contract['TIMINGS'])
        with tempfile.TemporaryDirectory() as scratch, \
                patch.object(contract['subprocess'], 'run', return_value=process) as run:
            value = contract['run_worker']('pinyin', '/so', '/data', 'case', scratch)
        self.assertEqual(value['result'], {'ret': True})
        self.assertNotIn('timing', value)
        self.assertIn('CONTRACT_DIFF_SPAWNED', run.call_args.kwargs['env'])
        timing = contract['TIMINGS'][before]
        self.assertEqual((timing['case'], timing['startup'], timing['execute']), ('case', 0.1, 0.4))
        self.assertIn('wall', timing)
        self.assertAlmostEqual(timing['teardown'], timing['wall'] - 1.0)
        del contract['TIMINGS'][before:]

    def test_summary_splits_pin_and_subject_execution(self):
        saved = list(contract['TIMINGS'])
        contract['TIMINGS'][:] = [dict(startup=1, load=2, fixture=3, execute=4, teardown=5),
                                  dict(startup=1, load=2, fixture=3, execute=6, teardown=5)]
        try:
            summary = contract['timing_summary'](['a'], 0.5, 30.0)
        finally:
            contract['TIMINGS'][:] = saved
        self.assertEqual(summary['cases'], 1)
        self.assertEqual(summary['workers'], 2)
        self.assertEqual(summary['seconds'], dict(startup=2, load=4, fixture=6, pin_execute=4, subject_execute=6,
                                                   teardown=10, compare=0.5, total=30.0))


if __name__ == '__main__':
    unittest.main()
