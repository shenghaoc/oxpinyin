#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
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


class DriverFixtureCase(unittest.TestCase):
    # These worker/verdict fixtures deliberately use synthetic, non-ELF paths.
    # Loader enforcement has its own real-ELF fixtures in tkrzw/runtime.test.py.
    def setUp(self):
        super().setUp()
        guard = patch.object(driver, 'check_tkrzw')
        self.runtime_guard = guard.start()
        self.addCleanup(guard.stop)


class ProfileRowsTests(DriverFixtureCase):
    def test_profile_worker_certifies_then_loads_exact_runtime_and_accepts_empty_db(self):
        events = []
        runtime = Path('/certified/libtkrzw.so.1')
        self.runtime_guard.side_effect = lambda *a, **kw: events.append('certify') or runtime
        with patch.object(driver, 'index_file_rows', side_effect=lambda *a, **kw: events.append(('read', a, kw)) or []), \
                patch.object(driver, 'Kit') as kit, patch('builtins.print') as output:
            driver.worker('profile-rows', '/copied/subject.so', '/profile/user_bigram.db', 'hash')
        kit.assert_not_called()
        self.assertEqual(events, ['certify', ('read', ('/profile/user_bigram.db',),
                                            dict(kind='hash', allow_empty=True, library=runtime))])
        self.assertEqual(json.loads(output.call_args.args[0]), dict(rows=[]))

    def test_profile_worker_runtime_refusal_prevents_reader(self):
        self.runtime_guard.side_effect = RuntimeError('wrong runtime')
        with patch.object(driver, 'index_file_rows') as read:
            with self.assertRaisesRegex(RuntimeError, 'wrong runtime'):
                driver.worker('profile-rows', '/copied/subject.so', '/profile/user_bigram.db', 'hash')
            read.assert_not_called()

    def test_tkrzw_profile_worker_requires_an_actual_tkrzw_anchor(self):
        self.runtime_guard.return_value = None
        with patch.object(driver, 'CELL', 'tkrzw'), patch.object(driver, 'index_file_rows') as read:
            with self.assertRaisesRegex(ValueError, 'no certified libtkrzw dependency'):
                driver.worker('profile-rows', '/bdb/subject.so', '/profile/db', 'tree')
        read.assert_not_called()

    def tkrzw(self, *, first=True, get=False, next_ok=False, code=7):
        library = MagicMock()
        library.tkrzw_dbm_open.return_value = 123
        library.tkrzw_dbm_make_iterator.return_value = 456
        library.tkrzw_dbm_iter_first.return_value = first
        library.tkrzw_dbm_iter_get.return_value = get
        library.tkrzw_dbm_iter_next.return_value = next_ok
        library.tkrzw_get_last_status_code.return_value = code
        library.tkrzw_dbm_close.return_value = True
        return library

    def test_tkrzw_hash_tree_and_empty_status_are_distinct_from_read_error(self):
        for kind, params in (('tree', b'dbm=TreeDBM'), ('hash', b'dbm=HashDBM')):
            for first in (True, False):
                tk = self.tkrzw(first=first)
                with self.subTest(kind=kind, first=first), patch.object(driver, 'CELL', 'tkrzw'), \
                        patch.object(driver.C, 'CDLL', side_effect=[tk, MagicMock()]) as load:
                    self.assertEqual(driver.index_file_rows('/profile/db', kind=kind, allow_empty=True,
                                                            library=Path('/verified/tkrzw.so')), [])
                self.assertEqual(load.call_args_list[0].args, ('/verified/tkrzw.so',))
                tk.tkrzw_dbm_open.assert_called_once_with(b'/profile/db', False, params)
                tk.tkrzw_dbm_iter_free.assert_called_once_with(456)
                tk.tkrzw_dbm_close.assert_called_once_with(123)
        for first in (True, False):
            tk = self.tkrzw(first=first, code=11)
            with patch.object(driver, 'CELL', 'tkrzw'), patch.object(driver.C, 'CDLL', side_effect=[tk, MagicMock()]):
                with self.assertRaisesRegex(ValueError, 'iterator (first|read) failed'):
                    driver.index_file_rows('/profile/db', allow_empty=True)
            tk.tkrzw_dbm_iter_free.assert_called_once()
            tk.tkrzw_dbm_close.assert_called_once()

    def test_tkrzw_row_retains_empty_value_and_frees_both_allocations_on_next_error(self):
        for code in (7, 11):
            tk = self.tkrzw(get=True, code=code)
            libc = MagicMock()
            def row(iterator, key, key_size, value, value_size):
                key._obj.value, key_size._obj.value = 1000, 1
                value._obj.value, value_size._obj.value = 2000, 0
                return True
            tk.tkrzw_dbm_iter_get.side_effect = row
            with patch.object(driver, 'CELL', 'tkrzw'), patch.object(driver.C, 'CDLL', side_effect=[tk, libc]), \
                    patch.object(driver.C, 'string_at', side_effect=[b'k', b'']):
                if code == 7:
                    self.assertEqual(driver.index_file_rows('/profile/db', kind='hash', allow_empty=True), [['6b', '']])
                else:
                    with self.assertRaisesRegex(ValueError, 'iterator next failed'):
                        driver.index_file_rows('/profile/db', kind='hash', allow_empty=True)
            self.assertEqual([call.args[0].value for call in libc.free.call_args_list], [1000, 2000])
            tk.tkrzw_dbm_iter_free.assert_called_once()
            tk.tkrzw_dbm_close.assert_called_once()

    def test_kc_empty_snapshot_and_cursor_errors_are_distinct_and_closed(self):
        for jump, code, fails in ((False, 7, False), (True, 7, False), (False, 9, True), (True, 9, True)):
            kc = MagicMock()
            kc.kcdbnew.return_value, kc.kcdbcursor.return_value = 123, 456
            kc.kcdbopen.return_value = kc.kcdbloadsnap.return_value = kc.kcdbclose.return_value = True
            kc.kccurjump.return_value, kc.kccurget.return_value = jump, None
            kc.kccurecode.return_value = code
            with self.subTest(jump=jump, code=code), patch.object(driver, 'CELL', 'kc'), \
                    patch.object(driver.C, 'CDLL', return_value=kc):
                if fails:
                    with self.assertRaisesRegex(ValueError, 'cursor (first|read) failed'):
                        driver.index_file_rows('/profile/db', kind='hash', allow_empty=True)
                else:
                    self.assertEqual(driver.index_file_rows('/profile/db', kind='hash', allow_empty=True), [])
            kc.kcdbloadsnap.assert_called_once_with(123, b'/profile/db')
            kc.kccurdel.assert_called_once_with(456)
            kc.kcdbclose.assert_called_once_with(123)
            kc.kcdbdel.assert_called_once_with(123)

    def test_corrupt_kc_snapshot_and_tkrzw_open_failure_are_not_empty_databases(self):
        kc = MagicMock()
        kc.kcdbloadsnap.return_value = False
        with patch.object(driver, 'CELL', 'kc'), patch.object(driver.C, 'CDLL', return_value=kc):
            with self.assertRaisesRegex(ValueError, 'snapshot load failed'):
                driver.index_file_rows('/profile/broken', kind='hash', allow_empty=True)
        kc.kcdbcursor.assert_not_called()
        kc.kcdbclose.assert_called_once()
        kc.kcdbdel.assert_called_once()
        tk = self.tkrzw()
        tk.tkrzw_dbm_open.return_value = None
        with patch.object(driver, 'CELL', 'tkrzw'), patch.object(driver.C, 'CDLL', side_effect=[tk, MagicMock()]):
            with self.assertRaisesRegex(ValueError, 'read-only open failed'):
                driver.index_file_rows('/profile/broken', allow_empty=True)
        tk.tkrzw_dbm_make_iterator.assert_not_called()
        tk.tkrzw_dbm_close.assert_not_called()



class FatalLogTests(DriverFixtureCase):
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


class DiagnosticPathTests(DriverFixtureCase):
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


class VerdictTests(DriverFixtureCase):
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


class DriverInvalidTests(DriverFixtureCase):
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


class DirChunkTests(DriverFixtureCase):
    """`dir_chunk`: the directory a NULL row makes the pin read is held to
    the side of `directory_reads_as_a_chunk` this host is on (#1011)."""

    USER = 'table-conf-abort-system-file-without-user-file'
    SYSTEM = 'table-conf-abort-system-file-without-chunk'
    FRESH = 'open SCRATCH/user-X/user.conf failed.'
    UNMAPPABLE = 'mmap SCRATCH/sys-X failed!\n'

    def tearDown(self):
        driver.directory_reads_as_a_chunk.cache_clear()

    def aborted(self):
        return observation(exit=-signal.SIGABRT, stderr=self.FRESH + 'assert\n', userfiles={'user.conf': [1, 'd']})

    def answered(self, ret=False, logs=(('libpinyin', 16),), stderr=FRESH):
        return observation(result={'ret': ret, 'logs': [list(log) for log in logs]}, stderr=stderr,
                           userfiles={'user.conf': [1, 'd']})

    def segv(self, stderr=FRESH + UNMAPPABLE):
        return observation(exit=-signal.SIGSEGV, stderr=stderr, userfiles={'user.conf': [1, 'd']})

    def decide(self, name, pin, subject, reads):
        with patch.object(driver, 'directory_reads_as_a_chunk', return_value=reads):
            return driver.verdict_for(driver.CASES[name], pin, subject, False)[0]

    def run_driver(self, name, pin, subject, reads):
        with tempfile.TemporaryDirectory() as scratch:
            prefix = Path(scratch)
            (prefix / 'lib/libpinyin/data').mkdir(parents=True)
            (prefix / 'lib/libpinyin.so').touch()
            (prefix / 'lib/libpinyin/data/bigram.db').touch()
            (prefix / 'oracle-pin.txt').write_text(
                'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+dbm-bdb\n')
            argv = ['driver', 'bdb', str(prefix), str(prefix / 'lib/libpinyin.so'), '--cases', name]
            workers = [pin, subject]
            output = io.StringIO()
            with patch.object(driver.sys, 'argv', argv), \
                    patch.object(driver, 'run_worker', side_effect=lambda *a, **k: workers.pop(0)), \
                    patch.object(driver, 'directory_reads_as_a_chunk', return_value=reads), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
                status = driver.main()
        rows = [row for row in (json.loads(line) for line in output.getvalue().splitlines()
                                if line.startswith('{')) if 'case' in row]
        return status, rows[0]['verdict'], rows[0]['expected_ok']

    def test_registered_cases(self):
        self.assertEqual({name: spec['dir_chunk'] for name, spec in driver.CASES.items() if spec['dir_chunk']}, {
            'table-conf-abort-system-file-without-chunk': 'system',
            'table-conf-abort-system-file-without-chunk-zhuyin': 'system',
            'table-conf-abort-load-addon-without-chunk': 'system',
            'table-conf-abort-system-file-without-user-file': 'user',
            'table-conf-abort-user-file-without-user-file': 'user'})

    def test_probe_repeats_the_pin_size_check(self):
        for sought, reads in ((OSError(22, 'Invalid argument'), False), (0, False), (7, False), (8, True),
                              (2 ** 63 - 1, True)):
            with self.subTest(sought=sought):
                driver.directory_reads_as_a_chunk.cache_clear()
                with patch.object(driver.os, 'lseek', side_effect=[sought]):
                    self.assertIs(driver.directory_reads_as_a_chunk(), reads)

    def test_condition_absent_keeps_the_abort_matcher(self):
        for name in (self.USER, self.SYSTEM):
            with self.subTest(name=name):
                self.assertEqual(self.decide(name, self.aborted(), self.answered(), True), 'MATCH')
        # Survivals and the tmpfs SIGSEGV are not the abort the host predicts.
        self.assertEqual(self.decide(self.USER, self.answered(True, ()), self.answered(True, ()), True), 'DIFFER')
        line = self.answered(stderr=self.FRESH + self.UNMAPPABLE)
        self.assertEqual(self.decide(self.SYSTEM, self.segv(), line, True), 'DIFFER')
        self.assertEqual(self.run_driver(self.SYSTEM, self.segv(), line, True), (1, 'DIFFER', False))

    def test_condition_present_user_dir_compares_both_survivors(self):
        survivor = self.answered(True, ())
        self.assertEqual(self.decide(self.USER, survivor, copy.deepcopy(survivor), False), 'MATCH')
        self.assertEqual(self.run_driver(self.USER, survivor, copy.deepcopy(survivor), False), (0, 'MATCH', True))
        # The abort answer, another stderr or another profile stays a DIFFER.
        self.assertEqual(self.decide(self.USER, survivor, self.answered(), False), 'DIFFER')
        self.assertEqual(self.decide(self.USER, survivor, self.answered(True, (), stderr=''), False), 'DIFFER')
        other_profile = self.answered(True, ())
        other_profile['userfiles'] = {}
        self.assertEqual(self.decide(self.USER, survivor, other_profile, False), 'DIFFER')
        self.assertEqual(self.decide(self.USER, self.aborted(), self.answered(), False), 'DIFFER')
        self.assertEqual(self.run_driver(self.USER, self.aborted(), self.answered(), False), (1, 'DIFFER', False))

    def test_condition_present_system_dir_segv_with_the_same_stderr_matches(self):
        line = self.answered(stderr=self.FRESH + self.UNMAPPABLE)
        self.assertEqual(self.decide(self.SYSTEM, self.segv(), line, False), 'MATCH')
        self.assertEqual(self.run_driver(self.SYSTEM, self.segv(), line, False), (0, 'MATCH', True))

    def test_condition_present_other_system_differences_stay_differ(self):
        line = self.FRESH + self.UNMAPPABLE
        other_profile = self.answered(stderr=line)
        other_profile['userfiles'] = {}
        for label, pin, subject in (
                ('subject without the line', self.segv(), self.answered()),
                ('subject with another line', self.segv(), self.answered(stderr=self.FRESH + 'mmap SCRATCH/x failed!\n')),
                ('subject writes it twice', self.segv(), self.answered(stderr=line + self.UNMAPPABLE)),
                ('pin without the line', self.segv(self.FRESH), self.answered(stderr=line)),
                ('abort', self.aborted(), self.answered(stderr=line)),
                ('pin survived', self.answered(True, (), stderr=line), self.answered(stderr=line)),
                ('subject loaded', self.segv(), self.answered(True, (), stderr=line)),
                ('two warnings', self.segv(), self.answered(logs=(('libpinyin', 16),) * 2, stderr=line)),
                ('other profile', self.segv(), other_profile)):
            with self.subTest(label):
                self.assertEqual(self.run_driver(self.SYSTEM, pin, subject, False), (1, 'DIFFER', False))
                self.assertNotEqual(self.decide(self.SYSTEM, pin, subject, False), 'MATCH')

    def test_other_cases_ignore_the_probe(self):
        spec = driver.CASES['table-conf-abort-default-dictionary']
        with patch.object(driver, 'directory_reads_as_a_chunk', side_effect=AssertionError('probed')):
            self.assertEqual(driver.verdict_for(spec, self.aborted(), self.answered(), False)[0], 'MATCH')


class StrjoinvOverReadTests(DriverFixtureCase):
    """`strjoinv_over_read`: register row 1's unterminated join is held to
    the class (b) fault, or compared exactly where the pin completes."""

    NAMES = ('user-gram-unsorted-export', 'user-gram-duplicate-export')
    STDERR = 'open SCRATCH/user-X/user.conf failed.\n'

    def steps(self, reading="ni'da'da"):
        return {'steps': [[True, '你龘龘', reading, 90], [True, '你号', "ni'hao", 80], 'no-next', 'no-next']}

    def completed(self, reading="ni'da'da"):
        return observation(result=self.steps(reading), stderr=self.STDERR)

    def decide(self, pin, subject):
        return [driver.verdict_for(driver.CASES[name], pin, subject, False)[0] for name in self.NAMES]

    def test_registered_cases(self):
        self.assertEqual(sorted(name for name, spec in driver.CASES.items() if spec['strjoinv_over_read']),
                         sorted(self.NAMES))

    def test_fault_after_the_same_stderr_is_held(self):
        pin = observation(exit=-signal.SIGSEGV, stderr=self.STDERR)
        self.assertEqual(self.decide(pin, self.completed()), ['MATCH', 'MATCH'])

    def test_fault_after_different_stderr_is_invalid(self):
        for stderr in ('', self.STDERR + 'more\n'):
            with self.subTest(stderr=stderr):
                pin = observation(exit=-signal.SIGSEGV, stderr=stderr)
                self.assertEqual(self.decide(pin, self.completed()), ['INVALID', 'INVALID'])

    def test_other_termination_is_invalid(self):
        for exit in (-signal.SIGABRT, -signal.SIGBUS, -signal.SIGTRAP, 1):
            with self.subTest(exit=exit):
                pin = observation(exit=exit, stderr=self.STDERR)
                self.assertEqual(self.decide(pin, self.completed()), ['INVALID', 'INVALID'])

    def test_subject_must_complete(self):
        pin = observation(exit=-signal.SIGSEGV, stderr=self.STDERR)
        self.assertEqual(self.decide(pin, observation(exit=-signal.SIGSEGV, stderr=self.STDERR)),
                         ['INVALID', 'INVALID'])
        self.assertEqual(self.decide(self.completed(), observation(exit=0, stderr=self.STDERR)),
                         ['INVALID', 'INVALID'])

    def test_exact_completion_matches(self):
        self.assertEqual(self.decide(self.completed(), self.completed()), ['MATCH', 'MATCH'])

    def test_completed_reading_is_compared_exactly(self):
        for reading in ("ni'da'da'junk", "ni'junk'da'da", "ni'da'da'", "ni'da", "ni'da'du", "nii'da'da",
                        "ni'dada", None):
            with self.subTest(reading=reading):
                self.assertEqual(self.decide(self.completed(reading), self.completed()), ['DIFFER', 'DIFFER'])

    def test_dropped_trailing_syllable_differs(self):
        pin, subject = self.completed(), self.completed()
        pin['result']['steps'][1][2], subject['result']['steps'][1][2] = "ni'hao'ma", "ni'hao"
        self.assertEqual(self.decide(pin, subject), ['DIFFER', 'DIFFER'])

    def test_other_fields_stay_exact(self):
        for change in (lambda s: s[0].__setitem__(1, '你龘'), lambda s: s[0].__setitem__(3, 91),
                       lambda s: s[0].__setitem__(0, False), lambda s: s.pop(), lambda s: s.insert(0, 'no-next')):
            pin = self.completed()
            change(pin['result']['steps'])
            self.assertEqual(self.decide(pin, self.completed()), ['DIFFER', 'DIFFER'])

    def test_other_cases_keep_their_matcher(self):
        spec = driver.CASES['default-option-parse']
        self.assertFalse(spec['strjoinv_over_read'])
        pin = observation(exit=-signal.SIGSEGV, stderr='')
        self.assertEqual(driver.verdict_for(spec, pin, observation(result={'ret': 1}), False)[0], 'INVALID')
        self.assertFalse(driver.matches(spec, observation(result={'r': "ni'hao'x"}),
                                        observation(result={'r': "ni'hao"})))


class WorkerEntryTests(DriverFixtureCase):
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
        self.assertEqual(self.runtime_guard.call_args.args[3], '/so')
        self.assertEqual(self.runtime_guard.call_args.kwargs['env'], run.call_args.kwargs['env'])

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


class SelectionTests(DriverFixtureCase):
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


class TimingTests(DriverFixtureCase):
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


class DeadlineTests(DriverFixtureCase):
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


class DriverVerdictTests(DriverFixtureCase):
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


class CancellationTests(DriverFixtureCase):
    """Cancel a real driver while its worker and a descendant ignore TERM."""

    def children(self, pid):
        children_path = Path(f'/proc/{pid}/task/{pid}/children')
        try:
            return [int(item) for item in children_path.read_text().split()]
        except OSError as children_error:
            # CONFIG_PROC_CHILDREN is optional; still observe the entire tree.
            try:
                children, scanned = [], False
                for process in Path('/proc').iterdir():
                    if not process.name.isdecimal():
                        continue
                    try:
                        parent = int((process / 'stat').read_text().rsplit(') ', 1)[1].split()[1])
                    except OSError:
                        continue  # An individual process can exit or be hidden.
                    scanned = True
                    if parent == pid:
                        children.append(int(process.name))
                if not scanned:
                    raise FileNotFoundError('no readable /proc/*/stat records')
                return children
            except (OSError, ValueError, IndexError) as stat_error:
                self.fail(f'descendant observation unavailable: {children_path}: {children_error}; '
                          f'/proc/*/stat scan: {stat_error}')

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

    def cancel(self, signum, through_runner=False, leader_exits=False, per_call=False):
        # Fail before spawning if neither process-observation source is available.
        self.children(os.getpid())
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
            setup = "module.WORKER = pathlib.Path(sys.argv.pop(2)); "
            if per_call:
                # Keep this process-group test independent of compiler and profiler tools.
                setup = ("module.__file__ = sys.argv.pop(2); "
                         "module.per_call_helper = lambda scratch: pathlib.Path('/unused'); "
                         "run = module.per_call_run; "
                         "module.per_call_run = lambda *a, **kw: run(*(*a[:5], 'wall', *a[6:]), **kw); ")
            code = ("import importlib.util, pathlib, sys; "
                    "spec = importlib.util.spec_from_file_location('contract', sys.argv[1]); "
                    "module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module); "
                    + setup + "module.check_tkrzw = lambda *a, **kw: None; sys.argv.pop(0); "
                    "sys.exit(module.main())")
            command = [sys.executable, '-c', code, str(Path(driver.__file__).resolve()),
                       str(worker), 'bdb', str(prefix), str(prefix / 'lib/libpinyin.so'),
                       *(['--per-call', 'lookup-only', '--per-call-facades', 'pinyin',
                          '--unordered-arm', 'subject'] if per_call else
                         ['--cases', 'default-option-parse'])]
            env = os.environ.copy()
            if through_runner:
                # Exercise real slot admission and cancellation without host resources
                # or caller settings delaying the worker past the startup deadline.
                env.update(OXPINYIN_LOCK_DIR=str(prefix / 'locks'), OXPINYIN_HEAVY_SLOTS='1',
                           OXPINYIN_RESOURCE_WAIT_SECONDS='0', CARGO_BUILD_JOBS='1',
                           RUST_TEST_THREADS='1')
                env.pop('OXPINYIN_HEAVY_SLOT_HELD', None)
                runner_code = (
                    "import importlib.util, sys; "
                    "spec = importlib.util.spec_from_file_location('run_all', sys.argv.pop(1)); "
                    "module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module); "
                    "module.resource_snapshot = lambda: dict(available_mib=4096, cpus=1, cargo_cpus=1, "
                    "notes=['synthetic cancellation-test resources']); "
                    "sys.exit(module.main())")
                command = [sys.executable, '-c', runner_code,
                           str(Path(__file__).with_name('run-all.py').resolve()),
                           '--heavy-slot', '--', *command]
            proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    start_new_session=True, env=env)
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
                # Also clean up groups created before startup detection succeeds.
                for pid in reversed(self.tree(proc.pid)):
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(pid, signal.SIGKILL)
                if worker_pid is not None:
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(worker_pid, signal.SIGKILL)
                if proc.poll() is None:
                    proc.kill()
                proc.communicate()

    @unittest.skipUnless(sys.platform.startswith('linux'),
                         'requires Linux /proc process observation')
    def test_sigint(self):
        self.cancel(signal.SIGINT)

    @unittest.skipUnless(sys.platform.startswith('linux'),
                         'requires Linux /proc process observation')
    def test_sigterm(self):
        for leader_exits in (False, True):
            with self.subTest(leader_exits=leader_exits):
                self.cancel(signal.SIGTERM, leader_exits=leader_exits)

    @unittest.skipUnless(sys.platform.startswith('linux'),
                         'requires Linux /proc process observation')
    def test_sighup(self):
        self.cancel(signal.SIGHUP)

    @unittest.skipUnless(sys.platform.startswith('linux'),
                         'requires Linux /proc process observation')
    def test_sigterm_through_run_all(self):
        self.cancel(signal.SIGTERM, through_runner=True)

    @unittest.skipUnless(sys.platform.startswith('linux'),
                         'requires Linux /proc process observation')
    def test_per_call_sigterm(self):
        for leader_exits in (False, True):
            with self.subTest(leader_exits=leader_exits):
                self.cancel(signal.SIGTERM, leader_exits=leader_exits, per_call=True)

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


class ProcessObservationTests(DriverFixtureCase):
    def test_children_file_observes_direct_children(self):
        with patch.object(Path, 'read_text', return_value='11 12\n'), \
                patch.object(Path, 'iterdir', side_effect=AssertionError('unexpected stat scan')):
            self.assertEqual(CancellationTests().children(10), [11, 12])

    def test_stat_scan_observes_worker_and_descendant_without_children_file(self):
        records = {'10': '10 (driver) S 1 0', '11': '11 (worker ) with spaces) S 10 0',
                   '12': '12 (descendant) S 11 0', '99': '99 (unrelated) S 1 0'}
        def read(path):
            if path.name == 'children' or path.parent.name == '98':
                raise FileNotFoundError(path)
            return records[path.parent.name]
        entries = [Path('/proc') / name for name in (*records, '98', 'self')]
        with patch.object(Path, 'read_text', read), \
                patch.object(Path, 'iterdir', side_effect=lambda: iter(entries)):
            self.assertEqual(CancellationTests().tree(10), [10, 11, 12])

    def test_unavailable_proc_sources_fail_explicitly(self):
        for error in (FileNotFoundError('/proc missing'), PermissionError('/proc unreadable')):
            with self.subTest(error=error), \
                    patch.object(Path, 'read_text', side_effect=FileNotFoundError('children missing')), \
                    patch.object(Path, 'iterdir', side_effect=error), \
                    self.assertRaisesRegex(AssertionError, r'/proc/10/task/10/children.* /proc/\*/stat scan:'):
                CancellationTests().children(10)

    def test_stat_scan_skips_unreadable_unrelated_entries(self):
        records = {'10': '10 (driver) S 1 0', '11': '11 (worker) S 10 0',
                   '12': '12 (descendant) S 11 0'}
        entries = [Path('/proc') / name for name in ('99', *records)]
        for error in (PermissionError('hidden'), ProcessLookupError('exited'), OSError('unreadable')):
            def read(path):
                if path.name == 'children':
                    raise FileNotFoundError(path)
                if path.parent.name == '99':
                    raise error
                return records[path.parent.name]
            with self.subTest(error=error), patch.object(Path, 'read_text', read), \
                    patch.object(Path, 'iterdir', side_effect=lambda: iter(entries)):
                self.assertEqual(CancellationTests().tree(10), [10, 11, 12])

    def test_all_unreadable_stat_records_fail_explicitly(self):
        errors = {'99': PermissionError('hidden'), '98': ProcessLookupError('exited'),
                  '97': OSError('unreadable')}
        def read(path):
            if path.name == 'children':
                raise FileNotFoundError(path)
            raise errors[path.parent.name]
        entries = [Path('/proc') / name for name in errors]
        with patch.object(Path, 'read_text', read), \
                patch.object(Path, 'iterdir', side_effect=lambda: iter(entries)), \
                self.assertRaisesRegex(AssertionError,
                                      r'/proc/10/task/10/children.* /proc/\*/stat scan: no readable /proc/\*/stat records'):
            CancellationTests().children(10)

    def test_unavailable_stat_records_fail_explicitly(self):
        for error in (FileNotFoundError('stat missing'), PermissionError('stat unreadable')):
            def read(path):
                if path.name == 'children':
                    raise FileNotFoundError('children missing')
                raise error
            with self.subTest(error=error), patch.object(Path, 'read_text', read), \
                    patch.object(Path, 'iterdir', side_effect=lambda: iter([Path('/proc/10')])), \
                    self.assertRaisesRegex(AssertionError, r'/proc/10/task/10/children.* /proc/\*/stat scan:'):
                CancellationTests().children(10)

    def test_unavailable_observation_fails_before_spawning(self):
        with patch.object(Path, 'read_text', side_effect=FileNotFoundError('children missing')), \
                patch.object(Path, 'iterdir', return_value=iter(())), \
                patch.object(subprocess, 'Popen') as spawn, \
                self.assertRaisesRegex(AssertionError, r'no readable /proc/\*/stat records'):
            CancellationTests().cancel(signal.SIGTERM, through_runner=True)
        spawn.assert_not_called()


class PerCallTests(DriverFixtureCase):
    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux measurement controller')
    def test_unordered_subject_is_refused_before_helper_or_measurements(self):
        from argparse import Namespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lib').mkdir()
            (root / 'lib/libpinyin.so').write_bytes(b'not an ordered ELF')
            args = Namespace(cases='', issue=[], family=[], expect_parent=False, observations=None,
                             plan_only=False, regenerate_sentence_ties=None, axis_c_replacement=None,
                             axis_c_mutation=None, axis_c_snapshot_check=False, per_call='lookup-only',
                             per_call_facades='pinyin', per_call_operations=None,
                             per_call_attribution_limit=25, prefix=root, pinyin_so=root / 'lib/libpinyin.so',
                             zhuyin_so=None, base_pinyin_so=None, base_zhuyin_so=None)
            with patch.object(driver, 'per_call_helper') as helper:
                with self.assertRaisesRegex(ValueError, 'function ordering: arm subject/pinyin refused'):
                    driver.run_per_call(args, None)
                helper.assert_not_called()

    def test_wait_errors_stop_and_reap_the_per_call_group(self):
        for error in (KeyboardInterrupt(), RuntimeError('wait failed')):
            with self.subTest(error=type(error).__name__), tempfile.TemporaryDirectory() as scratch:
                process = worker_process()
                process.__enter__.return_value = process
                process.communicate.side_effect = error
                with patch.object(driver, 'CANCELLED', False), \
                        patch.object(driver.subprocess, 'Popen', return_value=process), \
                        patch.object(driver.os, 'killpg') as kill, self.assertRaises(type(error)):
                    driver.per_call_run('pinyin', '/so', '/data', 'case', '/helper',
                                        'wall', 'count', scratch, 0)
                self.assertEqual([call.args for call in kill.call_args_list],
                                 [(process.pid, signal.SIGTERM), (process.pid, signal.SIGKILL)])
                self.assertEqual(process.wait.call_count, 2)

    def test_deadline_stops_and_reaps_the_per_call_group(self):
        process = worker_process()
        process.__enter__.return_value = process
        with tempfile.TemporaryDirectory() as scratch, patch.object(driver, 'CANCELLED', False), \
                patch.object(driver.time, 'monotonic', side_effect=[0, 901]), \
                patch.object(driver.subprocess, 'Popen', return_value=process), \
                patch.object(driver.os, 'killpg') as kill, self.assertRaises(subprocess.TimeoutExpired):
            driver.per_call_run('pinyin', '/so', '/data', 'case', '/helper',
                                'wall', 'count', scratch, 0)
        self.assertEqual([call.args for call in kill.call_args_list],
                         [(process.pid, signal.SIGTERM), (process.pid, signal.SIGKILL)])
        self.assertEqual(process.wait.call_count, 2)

    def test_ir_parser_requires_one_nonzero_ir_total(self):
        self.assertEqual(contract['per_call_ir']('noise\nevents: Ir\ntotals: 24000\n'), 24000)
        for text in ('', 'events: Ir\ntotals: 0', 'events: Dr\ntotals: 25',
                     'events: Ir Dr\ntotals: 25 1', 'events: Ir\ntotals: 25\ntotals: 26'):
            with self.subTest(text=text), self.assertRaises(ValueError):
                contract['per_call_ir'](text)

    def test_calibration_subtracts_only_three_instruction_callee(self):
        for total, expected in ((25000, 22), (24000, 21), (31000, 28)):
            self.assertEqual(contract['per_call_calibration'](total), expected)
        for total in (0, 3000, 24001):
            with self.assertRaises(ValueError):
                contract['per_call_calibration'](total)
        with self.assertRaises(ValueError):
            contract['per_call_calibration'](24000, 999)

    def sample(self, result=None, calls=7):
        return dict(observation=observation(result=result or {'ret': True, 'logs': []}),
                    stats={op: dict(calls=calls, ns=70) for op in contract['PER_CALL_OPS']}, ir=3500)

    def test_samples_keep_strict_verdict_and_attempt_counts(self):
        spec = contract['CASES']['wrap-256th-reading']
        good = self.sample()
        contract['per_call_checked'](spec, good, good, ('count',))
        for sample in (self.sample({'ret': False}), self.sample(calls=8), self.sample(calls=0)):
            with self.assertRaises(ValueError):
                contract['per_call_checked'](spec, good, sample, ('count',))
        dead = self.sample()
        dead['observation']['exit'] = -signal.SIGSEGV
        with self.assertRaisesRegex(ValueError, 'INVALID'):
            contract['per_call_checked'](spec, dead, dead, ('count',))

    def test_default_step_scope_has_no_side_effect(self):
        kit = object.__new__(contract['Kit'])
        with kit.per_call_step():
            pass
        self.assertEqual(vars(kit), {})

    def test_measurement_scope_resets_after_error(self):
        kit = object.__new__(contract['PerCallKit'])
        kit.active = False
        from unittest.mock import Mock
        kit.native = Mock()
        with self.assertRaises(RuntimeError):
            with kit.per_call_step():
                self.assertTrue(kit.active)
                raise RuntimeError('fixture failed')
        self.assertFalse(kit.active)
        kit.native.pc_start.assert_called_once_with()

    def test_profile_keeps_exclusive_costs_apart_from_call_edges(self):
        profile = contract['per_call_profile']("""events: Ir
positions: instr line
ob=(1) /object.so
fl=(1) caller.rs
fn=(1) add
0x10 10 10
cfi=(2) runtime.rs
cfn=(2) oxpinyin_runtime::RuntimeDict::token_introspection
calls=5 0x20 20
* * 50
fn=(2)
fi=(2)
0x20 20 20
cob=(1)
cfl=(1)
cfn=(3) decode
calls=5 0x30 30
+1 +1 30
fn=(3)
0x30 30 30
totals: 60
""")
        self.assertEqual(profile['total'], 60)
        self.assertEqual(sum(profile['exclusive'].values()), 60)
        self.assertEqual(sum(value[0] for value in profile['edges'].values()), 80)
        shown = contract['per_call_attribution'](profile, 5, 1)
        self.assertEqual(shown['exclusive_sum_ir_per_call'], 12)
        self.assertEqual(shown['functions'][0]['ir_per_call'], 6)
        self.assertEqual(shown['omitted_functions'], 2)
        self.assertEqual(shown['phase_partition'], 'not_established')
        self.assertEqual(shown['named_phase_candidates']['validation']['status'], 'named_roots_only')
        self.assertEqual(shown['named_phase_candidates']['validation']['inclusive_edges'][0]['inclusive_ir_per_call'], 10)
        self.assertEqual(shown['named_phase_candidates']['lookup']['status'], 'unresolved')
        self.assertEqual(len(contract['per_call_attribution'](profile, 5, 0)['functions']), 3)

    def test_profile_sums_repeated_costs_and_edges(self):
        text = """events: Ir
fn=(1) caller
1 2
cfn=(2) target
calls=1 2
1 3
cfn=(2)
calls=2 2
1 6
fn=(2)
2 4
+1 5
totals: 11
"""
        profile = contract['per_call_profile'](text)
        self.assertEqual(profile['exclusive']['???', 'target'], 9)
        self.assertEqual(profile['edges'][('???', 'caller'), ('???', 'target')], (9, 3))

    def test_callee_object_applies_only_to_next_function(self):
        profile = contract['per_call_profile']("""events: Ir
ob=(1) /subject.so
fn=caller
1 5
cob=(2) /libc.so
cfn=external
calls=1 2
1 7
calls=2 2
1 14
cfn=internal
calls=1 3
1 9
totals: 5
""")
        edges = profile['edges']
        self.assertEqual(edges[(('/subject.so', 'caller'), ('/libc.so', 'external'))], (21, 3))
        self.assertEqual(edges[(('/subject.so', 'caller'), ('/subject.so', 'internal'))], (9, 1))

    def test_profile_refuses_unresolved_ids_and_unreconciled_costs(self):
        for text in ('events: Ir\nfn=(7)\n1 9\ntotals: 9',
                     'events: Ir\nfn=x\n1 8\ntotals: 9',
                     'events: Ir\nfn=x\ncfn=y\ncalls=1 1\ntotals: 9',
                     'events: Ir\nfn=(1) x\nfn=(1) y\n1 9\ntotals: 9'):
            with self.subTest(text=text), self.assertRaises(ValueError):
                contract['per_call_profile'](text)

    def test_only_calls_inside_step_use_native_wrapper(self):
        from unittest.mock import Mock
        kit = object.__new__(contract['PerCallKit'])
        kit.native, kit.active, kit.operations = Mock(), False, ('count',)
        original = contract['C'].CFUNCTYPE(contract['B'], contract['P'], contract['U'],
                                         contract['C'].POINTER(contract['U']))(lambda *args: True)
        with patch.object(contract['Kit'], 'fn', return_value=original):
            counted = kit.fn('token_get_n_pronunciation', contract['B'])
        num = contract['U'](123)
        # Preparation and item reads use the original function even when the
        # callable was bound before a measured step.
        self.assertTrue(counted(None, 7, contract['C'].byref(num)))
        kit.native.pc_count.assert_not_called()
        with kit.per_call_step():
            counted(None, 7, contract['C'].byref(num))
        kit.native.pc_count.assert_called_once()
        counted(None, 7, contract['C'].byref(num))
        kit.native.pc_count.assert_called_once()

    @unittest.skipUnless(sys.platform.startswith('linux'),
                         'requires Linux CPU affinity')
    def test_native_rounds_interleave_and_profile_each_side(self):
        from argparse import Namespace
        globals_ = contract['run_per_call'].__globals__
        calls = []
        def run(mode, so, data, name, helper, metric, operations, scratch, cpu, **kwargs):
            calls.append((so.name, metric, name, operations))
            sample = self.sample()
            if name == 'calibrate':
                sample['ir'] = 24000
                sample['stats'][operations]['calls'] = 1000
            return sample
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'lib').mkdir()
            for name in ('lib/libpinyin.so', 'base.so', 'subject.so'):
                (root / name).touch()
            args = Namespace(cases='', issue=[], family=[], expect_parent=False, observations=None,
                             plan_only=False, regenerate_sentence_ties=None, axis_c_replacement=None,
                             axis_c_mutation=None, axis_c_snapshot_check=False, per_call='wrap',
                             per_call_facades='pinyin', per_call_cpu=None, prefix=root,
                             per_call_operations=None, per_call_attribution=False, per_call_attribution_limit=25,
                             pinyin_so=root / 'subject.so', zhuyin_so=None,
                             base_pinyin_so=root / 'base.so', base_zhuyin_so=None,
                             unordered_arm=['base', 'subject'])
            import contextlib, io
            with patch.dict(globals_, per_call_helper=lambda scratch: root / 'helper.so', per_call_run=run), \
                 contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(contract['run_per_call'](args, None), 0)
        native = [name for name, metric, _, _ in calls if metric == 'wall']
        self.assertEqual(native, ['libpinyin.so', 'base.so', 'subject.so',
                                  'base.so', 'subject.so', 'libpinyin.so',
                                  'subject.so', 'libpinyin.so', 'base.so'])
        profiled = [(name, op) for name, metric, case, op in calls if metric == 'ir' and case != 'calibrate']
        self.assertEqual(profiled, [(side, op) for side in ('libpinyin.so', 'base.so', 'subject.so')
                                    for op in ('add', 'count', 'lookup')])


if __name__ == '__main__':
    unittest.main()
