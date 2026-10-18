#!/usr/bin/env python3
"""Regressions for contract verdicts, selection, timing and worker cleanup."""

import copy
import contextlib
import importlib.util
import io
import os
import subprocess
import sys
import time
import json
import signal
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

spec = importlib.util.spec_from_file_location('contract_diff', Path(__file__).with_name('contract-diff.py'))
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)
contract = vars(driver)


def worker_process(stdout=b'{}\n', stderr=b''):
    return MagicMock(returncode=0, communicate=MagicMock(return_value=(stdout, stderr)))


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
                    process = worker_process(json.dumps(result).encode(), text.encode())
                    with patch.object(
                        contract['subprocess'], 'Popen', return_value=process
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

    def invoke(self, parent, workers, plan=False, host=False):
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
            if plan:
                argv.append('--plan-only')
            output = io.StringIO()
            cases = {'default-option-parse': dict(contract['CASES']['default-option-parse'],
                                                host_scanf=('1e',) if host else ())}
            with patch.dict(contract['main'].__globals__, CASES=cases), \
                    patch.object(contract['sys'], 'argv', argv), \
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

    def test_plan_only_never_runs_a_case(self):
        with patch.object(driver, 'run_case', side_effect=AssertionError('plan executed a case')):
            status, rows = self.invoke(False, [], plan=True)
        self.assertEqual((status, rows), (0, []))

    def test_invalid_observations_cannot_be_host_dependent(self):
        with patch.object(driver, 'host_scanf_accepts', return_value=True), \
                patch.object(driver, 'pin_loaded_subject_refused_table_conf', return_value=True):
            status, rows = self.invoke(False, [observation(exit=1), observation(exit=1)], host=True)
        self.assertEqual((status, rows[0]['verdict']), (1, 'INVALID'))


class WorkerEntryTests(unittest.TestCase):
    """Workers start from contract-worker.py, which loads the same driver."""

    def test_run_worker_starts_the_cached_entry(self):
        process = worker_process()
        with tempfile.TemporaryDirectory() as scratch, \
                patch.object(contract['subprocess'], 'Popen', return_value=process) as run:
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
        process = worker_process(stdout)
        before = len(contract['TIMINGS'])
        with tempfile.TemporaryDirectory() as scratch, \
                patch.object(contract['subprocess'], 'Popen', return_value=process) as run:
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


    def test_summary_preserves_sides_after_a_pin_timeout(self):
        timings = [dict(execute=1, side='pin'), dict(execute=2, side='pin'),
                   dict(execute=3, side='subject')]
        with patch.object(driver, 'TIMINGS', timings):
            summary = driver.timing_summary(['timed-out', 'next'], 0, 6)
        self.assertEqual(summary['workers'], 3)
        self.assertEqual(summary['seconds']['pin_execute'], 3)
        self.assertEqual(summary['seconds']['subject_execute'], 3)


class DeadlineTests(unittest.TestCase):
    def test_shared_budget_and_fatal_environment(self):
        spec = dict(mode='pinyin', fatal=True)
        with patch.object(driver.time, 'monotonic', side_effect=[10, 11, 17]), \
             patch.object(driver, 'run_worker', side_effect=[observation(result={'ret': True}), observation(result={'ret': True})]) as run:
            pin, subject = driver.run_case(spec, Path('/pin'), Path('/subject'), Path('/data'), 'case', '/tmp', 20)
        self.assertEqual([call.kwargs['timeout'] for call in run.call_args_list], [19, 13])
        self.assertEqual(run.call_args.kwargs['extra_env'], {'G_DEBUG': 'fatal-criticals'})
        self.assertEqual(pin, subject)

    def test_pin_timeout_does_not_launch_subject(self):
        with patch.object(driver, 'run_worker', side_effect=subprocess.TimeoutExpired('worker', 1)) as run:
            results = driver.run_case(dict(mode='pinyin', fatal=False), Path('/p'), Path('/s'), '/data', 'case', '/tmp', 1)
        self.assertEqual([r['exit'] for r in results], ['TIMEOUT', 'NOT-RUN'])
        self.assertEqual(run.call_count, 1)

    def test_subject_timeout_preserves_completed_pin(self):
        good = observation(result={'ret': True})
        with patch.object(driver, 'run_worker', side_effect=[good, subprocess.TimeoutExpired('worker', 1, stderr=b'partial\n')]):
            results = driver.run_case(dict(mode='pinyin', fatal=False), Path('/p'), Path('/s'), '/data', 'case', '/tmp', 1)
        self.assertIs(results[0], good)
        self.assertEqual(results[1]['exit'], 'TIMEOUT')
        self.assertEqual(results[1]['stderr_lines'], 1)
        self.assertIsNone(results[1]['result'])

    def test_expired_budget_does_not_start_another_worker(self):
        with patch.object(driver.time, 'monotonic', side_effect=[0, 1, 4]), \
             patch.object(driver, 'run_worker', return_value=observation(result={'ret': True})) as run:
            results = driver.run_case(dict(mode='pinyin', fatal=False), Path('/p'), Path('/s'), '/data', 'case', '/tmp', 3)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(results[1]['exit'], 'TIMEOUT')

    def test_timeout_kills_descendants_holding_output_pipes(self):
        with tempfile.TemporaryDirectory() as scratch:
            worker = Path(scratch) / 'worker.py'
            worker.write_text('import os, time\n'
                              'print(\'T {"startup": 0.01, "load": 0.02}\', flush=True)\n'
                              'if os.fork() == 0:\n'
                              '    time.sleep(60)\n'
                              'else:\n'
                              '    print("ready", flush=True)\n'
                              '    time.sleep(60)\n')
            start = time.monotonic()
            before = len(driver.TIMINGS)
            with patch.object(driver, 'WORKER', worker), self.assertRaises(subprocess.TimeoutExpired) as caught:
                driver.run_worker('pinyin', Path('/unused'), Path('/unused'), 'case', scratch, timeout=.2)
            self.assertIn(b'ready', caught.exception.output)
            timing = driver.TIMINGS[before]
            self.assertEqual((timing['startup'], timing['load']), (0.01, 0.02))
            self.assertGreater(timing['wall'], 0.19)
            del driver.TIMINGS[before:]
            self.assertLess(time.monotonic() - start, 3)


class DriverVerdictTests(unittest.TestCase):
    def invoke(self, parent=False, timeout=None, timeout_side=0):
        with tempfile.TemporaryDirectory() as scratch:
            prefix = Path(scratch)
            (prefix / 'lib/libpinyin/data').mkdir(parents=True)
            (prefix / 'lib/libpinyin.so').touch()
            (prefix / 'lib/libpinyin/data/bigram.db').touch()
            (prefix / 'oracle-pin.txt').write_text('libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+dbm-bdb\n')
            args = ['driver', 'bdb', str(prefix), str(prefix / 'lib/libpinyin.so'), '--cases', 'slow,next']
            if parent:
                args.append('--expect-parent')
            if timeout is not None:
                args.extend(['--case-timeout', str(timeout)])
            cases = {name: dict(driver.CASES['default-option-parse'], control=True) for name in ('slow', 'next')}
            cases['slow'].update(control=False, host_scanf=('1e',))
            calls = []
            def worker(*args, **kwargs):
                calls.append(kwargs)
                if len(calls) == timeout_side + 1:
                    raise subprocess.TimeoutExpired('worker', kwargs['timeout'])
                return observation(result={'ret': True})
            output = io.StringIO()
            with patch.object(driver.sys, 'argv', args), patch.object(driver, 'CASES', cases), \
                 patch.object(driver, 'run_worker', side_effect=worker), contextlib.redirect_stdout(output):
                status = driver.main()
            rows = [json.loads(line) for line in output.getvalue().splitlines() if line.startswith('{') and 'case' in json.loads(line)]
            return status, rows, calls

    def test_pin_timeout_is_failure_and_next_case_runs(self):
        with patch.object(driver, 'host_scanf_accepts', return_value=True), \
                patch.object(driver, 'pin_loaded_subject_refused_table_conf', return_value=True):
            status, rows, calls = self.invoke()
        self.assertEqual(status, 1)
        self.assertEqual([r['verdict'] for r in rows], ['TIMEOUT', 'MATCH'])
        self.assertFalse(rows[0]['expected_ok'])
        self.assertEqual(rows[0]['exit'], ['TIMEOUT', 'NOT-RUN'])
        self.assertTrue(299 < calls[0]['timeout'] <= 300)

    def test_subject_timeout_is_failure_in_parent_mode(self):
        status, rows, calls = self.invoke(parent=True, timeout=7, timeout_side=1)
        self.assertEqual(status, 1)
        self.assertEqual([r['verdict'] for r in rows], ['TIMEOUT', 'MATCH'])
        self.assertFalse(rows[0]['expected_ok'])
        self.assertEqual(rows[0]['exit'], [0, 'TIMEOUT'])
        self.assertTrue(6 < calls[0]['timeout'] <= 7)

    def test_completed_observations_still_match(self):
        status, rows, _ = self.invoke(timeout_side=99)
        self.assertEqual(status, 0)
        self.assertEqual([r['verdict'] for r in rows], ['MATCH', 'MATCH'])

    def test_invalid_limits_are_rejected(self):
        for value in ('0', '-1', 'nan', 'inf'):
            with self.subTest(value=value), contextlib.redirect_stderr(io.StringIO()), \
                 patch.object(driver.sys, 'argv', ['driver', 'bdb', '/unused', '/unused', '--case-timeout', value]), \
                 self.assertRaises(SystemExit) as caught:
                driver.main()
            self.assertEqual(caught.exception.code, 2)


class CancellationTests(unittest.TestCase):
    """Cancel a real driver while its worker and a descendant ignore TERM."""

    def children(self, pid):
        try:
            return [int(item) for item in Path(f'/proc/{pid}/task/{pid}/children').read_text().split()]
        except FileNotFoundError:
            return []

    def tree(self, pid):
        result = [pid]
        for child in self.children(pid):
            result.extend(self.tree(child))
        return result

    def running(self, pid):
        try:
            return Path(f'/proc/{pid}/stat').read_text().split(') ')[1].split()[0] != 'Z'
        except FileNotFoundError:
            return False

    def cancel(self, signum, through_runner=False, leader_exits=False):
        with tempfile.TemporaryDirectory() as scratch:
            prefix = Path(scratch)
            (prefix / 'lib/libpinyin/data').mkdir(parents=True)
            (prefix / 'lib/libpinyin.so').touch()
            (prefix / 'lib/libpinyin/data/bigram.db').touch()
            (prefix / 'oracle-pin.txt').write_text(
                'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+dbm-bdb\n')
            worker = prefix / 'worker.py'
            worker.write_text('import os, signal, time\n'
                              'signal.signal(signal.SIGTERM, signal.SIG_IGN)\n'
                              'child = os.fork()\n'
                              f'if child and {leader_exits!r}:\n'
                              '    signal.signal(signal.SIGTERM, signal.SIG_DFL)\n'
                              'time.sleep(60)\n')
            code = ("import importlib.util, pathlib, sys; "
                    "spec = importlib.util.spec_from_file_location('contract', sys.argv[1]); "
                    "module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module); "
                    "module.WORKER = pathlib.Path(sys.argv.pop(2)); sys.argv.pop(0); "
                    "sys.exit(module.main())")
            command = [sys.executable, '-c', code, str(Path(driver.__file__).resolve()),
                       str(worker), 'bdb', str(prefix), str(prefix / 'lib/libpinyin.so'),
                       '--cases', 'default-option-parse']
            if through_runner:
                command = [sys.executable, str(Path(__file__).with_name('run-all.py')),
                           '--heavy-slot', '--', *command]
            proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    start_new_session=True)
            worker_pid = descendant = None
            try:
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    for pid in self.tree(proc.pid):
                        try:
                            argv = Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')
                        except FileNotFoundError:
                            continue
                        if len(argv) > 1 and argv[1] == os.fsencode(worker):
                            children = self.children(pid)
                            if children:
                                worker_pid, descendant = pid, children[0]
                                break
                    if descendant is not None or proc.poll() is not None:
                        break
                    time.sleep(0.02)
                self.assertIsNotNone(descendant, 'worker and descendant did not start')
                self.assertTrue(self.running(worker_pid))
                self.assertTrue(self.running(descendant))
                proc.send_signal(signum)
                stdout, stderr = proc.communicate(timeout=10)
                self.assertNotEqual(proc.returncode, 0, stderr.decode(errors='replace'))
                self.assertFalse(any(word in stdout for word in (b'MATCH', b'DIFFER', b'TIMEOUT')))
                deadline = time.monotonic() + 2
                while any(self.running(pid) for pid in (worker_pid, descendant)) and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertFalse(self.running(worker_pid), 'worker survived cancellation')
                self.assertFalse(self.running(descendant), 'descendant survived cancellation')
                # The direct worker must be reaped, not left as a zombie.
                self.assertFalse(Path(f'/proc/{worker_pid}').exists())
            finally:
                if worker_pid is not None:
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(worker_pid, signal.SIGKILL)
                if proc.poll() is None:
                    proc.kill()
                proc.communicate()

    def test_sigint(self):
        self.cancel(signal.SIGINT)

    def test_sigterm(self):
        for leader_exits in (False, True):
            with self.subTest(leader_exits=leader_exits):
                self.cancel(signal.SIGTERM, leader_exits=leader_exits)

    def test_sighup(self):
        self.cancel(signal.SIGHUP)

    def test_sigterm_through_run_all(self):
        self.cancel(signal.SIGTERM, through_runner=True)

    def test_exception_and_keyboard_interrupt_stop_group_and_reap(self):
        for error in (KeyboardInterrupt(), RuntimeError('wait failed')):
            with self.subTest(error=type(error).__name__), tempfile.TemporaryDirectory() as scratch:
                process = worker_process()
                process.communicate.side_effect = error
                with patch.object(driver.subprocess, 'Popen', return_value=process), \
                        patch.object(driver.os, 'killpg') as kill, self.assertRaises(type(error)):
                    driver.run_worker('pinyin', '/so', '/data', 'case', scratch)
                self.assertEqual([call.args for call in kill.call_args_list],
                                 [(process.pid, signal.SIGTERM), (process.pid, signal.SIGKILL)])
                self.assertEqual(process.wait.call_count, 2)


if __name__ == '__main__':
    unittest.main()
