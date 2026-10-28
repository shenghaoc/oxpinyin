#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for the contract driver's worker-process and run-output helpers (#1175)."""

import argparse
import contextlib
import hashlib
import io
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

import contract_execution as execution
from tools.dev import host


class ProcessGroupTests(unittest.TestCase):
    def leader(self, script):
        process = subprocess.Popen([sys.executable, '-c', script], stdout=subprocess.PIPE, text=True,
                                   start_new_session=True)
        self.addCleanup(process.stdout.close)
        self.addCleanup(lambda: process.poll() is None and process.kill())
        return process, int(process.stdout.readline())

    def assert_stopped(self, group):
        # kill(pid, 0) still succeeds for a zombie. Its adopter may not reap it
        # (notably PID 1 in a container), but it cannot execute or keep files open.
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if host.process_group_live(group) is False:
                return
            time.sleep(.02)
        with contextlib.suppress(ProcessLookupError):
            os.killpg(group, signal.SIGKILL)
        self.fail('process group %d still has an executing member or its state is unknown' % group)

    def test_an_exited_unreaped_process_does_not_count_as_a_live_group(self):
        process = subprocess.Popen([sys.executable, '-c', 'pass'], stdout=subprocess.PIPE,
                                   start_new_session=True)
        self.addCleanup(process.wait)
        self.addCleanup(process.stdout.close)
        process.stdout.read()
        self.assert_stopped(process.pid)
        # Do not poll/wait first: the PID must still exist to exercise the bug.
        os.kill(process.pid, 0)

    def test_a_live_or_unknown_group_is_still_a_failure(self):
        for observation in (True, None):
            with self.subTest(observation=observation), \
                    patch.object(host, 'process_group_live', return_value=observation), \
                    patch.object(time, 'monotonic', side_effect=(0, 0, 5)), \
                    patch.object(time, 'sleep'), patch.object(os, 'killpg') as kill:
                with self.assertRaisesRegex(AssertionError, 'executing member or its state is unknown'):
                    self.assert_stopped(4321)
                kill.assert_called_once_with(4321, signal.SIGKILL)

    def test_a_leader_that_ignores_sigterm_is_killed_with_its_descendant_after_the_grace_period(self):
        process, grandchild = self.leader(
            'import os, signal, time\n'
            'signal.signal(signal.SIGTERM, signal.SIG_IGN)\n'
            'child = os.fork()\n'
            'if child == 0:\n    time.sleep(60)\n    os._exit(0)\n'
            'print(child, flush=True)\n'
            'time.sleep(60)\n')
        self.assertEqual(os.getpgid(grandchild), process.pid)
        started = time.monotonic()
        execution.terminate_group(process)
        self.assertGreaterEqual(time.monotonic() - started, 0.4, 'the grace period is half a second')
        self.assertEqual(process.returncode, -signal.SIGKILL)
        self.assert_stopped(process.pid)

    def test_a_descendant_survives_the_leaders_exit_only_until_the_group_is_killed(self):
        process, grandchild = self.leader(
            'import os, signal, time\n'
            'child = os.fork()\n'
            'if child == 0:\n'
            '    signal.signal(signal.SIGTERM, signal.SIG_IGN)\n'
            '    print(os.getpid(), flush=True)\n'
            '    time.sleep(60)\n'
            '    os._exit(0)\n'
            'time.sleep(60)\n')
        self.assertEqual(os.getpgid(grandchild), process.pid)
        execution.terminate_group(process)
        self.assertEqual(process.returncode, -signal.SIGTERM)
        self.assert_stopped(process.pid)

    def test_a_group_that_is_already_gone_is_not_an_error(self):
        process = subprocess.Popen([sys.executable, '-c', 'pass'], start_new_session=True)
        process.wait()
        execution.terminate_group(process)
        self.assertEqual(process.returncode, 0)


class UserDirectoryListingTests(unittest.TestCase):
    def test_new_user_directories_are_listed_by_file_kind_and_a_later_directory_overwrites_a_name(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            for stale in ('user-old', 'sys-ccc'):
                (root / stale).mkdir()
                (root / stale / 'user.conf').write_text('kept')
            (root / 'keep.txt').write_text('x')
            before = set(os.listdir(scratch))
            first, second = root / 'user-aaa', root / 'user-bbb'
            first.mkdir()
            second.mkdir()
            (first / 'user.conf').write_text('a:1\nb:2\n')
            (first / 'gb_char.dbin').write_bytes(b'first log')
            (first / 'x.tmp').write_bytes(b'partial')
            (first / 'user_phrase_index.bin').write_bytes(b'container')
            (first / 'notes').write_bytes(b'12345')
            (first / 'sub').mkdir()
            (second / 'gb_char.dbin').write_bytes(b'second log')
            listing = execution.user_dir_listing(scratch, before)
        self.assertEqual(listing, {
            'user.conf': 'a:1|b:2|',
            'gb_char.dbin': [10, hashlib.sha256(b'second log').hexdigest()],
            'x.tmp': [7, hashlib.sha256(b'partial').hexdigest()],
            'user_phrase_index.bin': [9],
            'notes': [5],
            'sub': None,
        })

    def test_a_user_conf_directory_is_listed_as_a_non_file(self):
        with tempfile.TemporaryDirectory() as scratch:
            user = Path(scratch) / 'user-dir-conf'
            user.mkdir()
            (user / 'user.conf').mkdir()
            listing = execution.user_dir_listing(scratch, set())
        self.assertEqual(listing, {'user.conf': None})

    def test_the_directories_it_lists_are_removed_and_the_others_stay(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'user-new').mkdir()
            (root / 'user-old').mkdir()
            (root / 'sys-new').mkdir()
            (root / 'user-file').write_text('not a directory')
            execution.user_dir_listing(scratch, {'user-old'})
            self.assertEqual(sorted(os.listdir(scratch)), ['sys-new', 'user-file', 'user-old'])


class JobCountTests(unittest.TestCase):
    def parser(self):
        return argparse.ArgumentParser(prog='driver')

    def test_a_job_count_is_a_positive_integer(self):
        self.assertEqual(execution.positive_jobs('3'), 3)
        for value in ('0', '-1'):
            with self.subTest(value=value), self.assertRaisesRegex(argparse.ArgumentTypeError, 'positive integer'):
                execution.positive_jobs(value)
        with self.assertRaises(ValueError):
            execution.positive_jobs('many')

    def test_the_flag_wins_the_environment_next_and_serial_is_the_default(self):
        with patch.dict(os.environ, OXPINYIN_CONTRACT_JOBS='bad'):
            self.assertEqual(execution.ordinary_jobs(self.parser(), 5), 5)
        environment = {name: value for name, value in os.environ.items() if name != 'OXPINYIN_CONTRACT_JOBS'}
        with patch.dict(os.environ, environment, clear=True):
            self.assertEqual(execution.ordinary_jobs(self.parser(), None), 1)
        with patch.dict(os.environ, OXPINYIN_CONTRACT_JOBS='4'):
            self.assertEqual(execution.ordinary_jobs(self.parser(), None), 4)

    def test_a_bad_environment_value_is_a_usage_error_naming_the_variable(self):
        for value in ('0', '-2', 'x', ''):
            with self.subTest(value=value), patch.dict(os.environ, OXPINYIN_CONTRACT_JOBS=value), \
                    contextlib.redirect_stderr(io.StringIO()) as errors, self.assertRaises(SystemExit) as caught:
                execution.ordinary_jobs(self.parser(), None)
            self.assertEqual(caught.exception.code, 2)
            self.assertIn('Invalid OXPINYIN_CONTRACT_JOBS: use a positive integer', errors.getvalue())


class TimingTests(unittest.TestCase):
    def test_phases_are_summed_and_execution_is_split_by_side(self):
        timings = [dict(startup=1, load=2, fixture=3, execute=4, teardown=5),
                   dict(startup=1, load=2, fixture=3, execute=6, teardown=5)]
        summary = execution.timing_summary(timings, ['a'], 0.5, 30.0)
        self.assertEqual(summary, dict(cases=1, workers=2, seconds=dict(
            startup=2, load=4, fixture=6, pin_execute=4, subject_execute=6, teardown=10, compare=0.5, total=30.0)))

    def test_a_recorded_side_overrides_the_order_after_a_pin_timeout(self):
        timings = [dict(execute=1, side='pin'), dict(execute=2, side='pin'), dict(execute=3, side='subject')]
        seconds = execution.timing_summary(timings, ['timed-out', 'next'], 0, 6)['seconds']
        self.assertEqual((seconds['pin_execute'], seconds['subject_execute']), (3, 3))

    def test_no_workers_is_all_zero_and_rounding_is_to_four_places(self):
        summary = execution.timing_summary([], [], 0.123456, 1.987654)
        self.assertEqual(summary['workers'], 0)
        self.assertEqual(summary['seconds']['compare'], 0.1235)
        self.assertEqual(summary['seconds']['total'], 1.9877)
        self.assertEqual(summary['seconds']['startup'], 0.0)


if __name__ == '__main__':
    unittest.main()
