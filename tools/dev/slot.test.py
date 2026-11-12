#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Unit tests for tools.dev.slot."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import slot, test_support


def running(pid):
    """True while PID is a live process; a killed child awaiting an init that does not reap it is not."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    try:
        return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] not in ('Z', 'X')
    except OSError:
        return True    # no /proc (macOS): an orphan there is reaped at once, so it is really running


class GroupTerminationTests(unittest.TestCase):
    if sys.platform != 'win32':
        # POSIX process groups (os.killpg); Windows has neither, and terminate_group's Windows path is below.
        def test_darwin_exiting_group_permission_error_retries_final_kill(self):
            process = mock.Mock(pid=123)
            with mock.patch.object(sys, 'platform', 'darwin'), \
                    mock.patch.object(slot.os, 'killpg', side_effect=[PermissionError(), PermissionError(), ProcessLookupError()]) as kill, \
                    mock.patch.object(slot.time, 'monotonic', side_effect=[0, .01]), \
                    mock.patch.object(slot.time, 'sleep') as sleep:
                slot.terminate_group(process)
            self.assertEqual(kill.call_args_list, [mock.call(123, signal.SIGTERM),
                                                  mock.call(123, signal.SIGKILL), mock.call(123, signal.SIGKILL)])
            sleep.assert_called_once_with(.01)
            self.assertEqual(process.wait.call_args_list, [mock.call(timeout=5), mock.call()])

    def windows_ctrl_c(self, *, signum, exits):
        """terminate_group on Windows after a cancellation by SIGNUM; the child EXITS within the grace or not."""
        process = mock.Mock()
        process.wait.side_effect = [0] if exits else [subprocess.TimeoutExpired('child', 5), 0]
        saved = slot.CANCELLED.is_set(), slot.CANCELLED_SIGNAL
        self.addCleanup(lambda: (slot.CANCELLED.set() if saved[0] else slot.CANCELLED.clear(),
                                 setattr(slot, 'CANCELLED_SIGNAL', saved[1])))
        slot.CANCELLED.set()
        slot.CANCELLED_SIGNAL = signum
        with mock.patch.object(sys, 'platform', 'win32'):
            slot.terminate_group(process)
        return process

    def test_a_windows_ctrl_c_lets_the_child_finish_its_own_cleanup(self):
        # The console Ctrl-C reached the child too (the pool removing its containers): no terminate.
        process = self.windows_ctrl_c(signum=signal.SIGINT, exits=True)
        process.terminate.assert_not_called()
        self.assertEqual(process.wait.call_args_list, [mock.call(timeout=5)])

    def test_a_windows_child_that_outlasts_the_grace_is_terminated(self):
        process = self.windows_ctrl_c(signum=signal.SIGINT, exits=False)
        process.terminate.assert_called_once_with()
        self.assertEqual(process.wait.call_args_list, [mock.call(timeout=5), mock.call(timeout=5)])

    def test_other_windows_cancellations_terminate_at_once(self):
        process = self.windows_ctrl_c(signum=signal.SIGTERM, exits=True)
        process.terminate.assert_called_once_with()
        self.assertEqual(process.wait.call_args_list, [mock.call(timeout=5)])
        slot.CANCELLED.clear()
        process = mock.Mock()
        with mock.patch.object(sys, 'platform', 'win32'):
            slot.terminate_group(process)    # a timeout or another failure: no cancellation at all
        process.terminate.assert_called_once_with()

    def windows_job(self, *, empty=True):
        """A process that spawn put in a Job Object, and that job, for terminate_group on Windows."""
        job = mock.Mock()
        job.wait_empty.return_value = empty
        process = mock.Mock(pid=123, job=job)
        process.wait.return_value = 1
        return process, job

    def test_a_windows_group_ends_the_whole_job_and_waits_until_it_is_empty(self):
        process, job = self.windows_job()
        with mock.patch.object(sys, 'platform', 'win32'):
            slot.terminate_group(process)
        job.terminate.assert_called_once_with()
        job.wait_empty.assert_called_once_with(5)
        process.terminate.assert_called_once_with()

    def test_a_windows_job_that_keeps_a_member_is_not_reported_as_ended(self):
        process, job = self.windows_job(empty=False)
        with mock.patch.object(sys, 'platform', 'win32'), self.assertRaises(RuntimeError):
            slot.terminate_group(process)

    def test_a_windows_leader_that_leaves_in_the_grace_still_has_its_job_ended(self):
        process, job = self.windows_job()
        process.wait.side_effect = [0]
        saved = slot.CANCELLED.is_set(), slot.CANCELLED_SIGNAL
        self.addCleanup(lambda: (slot.CANCELLED.set() if saved[0] else slot.CANCELLED.clear(),
                                 setattr(slot, 'CANCELLED_SIGNAL', saved[1])))
        slot.CANCELLED.set()
        slot.CANCELLED_SIGNAL = signal.SIGINT
        with mock.patch.object(sys, 'platform', 'win32'):
            slot.terminate_group(process)
        process.terminate.assert_not_called()
        job.terminate.assert_called_once_with()

    if sys.platform != 'win32':
        # The group's PermissionError comes from os.killpg, which only POSIX has.
        def test_group_permission_errors_are_not_silenced(self):
            for platform in ('darwin', 'linux'):
                with self.subTest(platform=platform), mock.patch.object(sys, 'platform', platform), \
                        mock.patch.object(slot.os, 'killpg', side_effect=[None, PermissionError()]), \
                        mock.patch.object(slot.time, 'monotonic', side_effect=[0, 6]), \
                        mock.patch.object(slot.time, 'sleep') as sleep:
                    with self.assertRaises(PermissionError):
                        slot.terminate_group(mock.Mock(pid=123))
                    sleep.assert_not_called()


class RunCommandTests(unittest.TestCase):
    """run_command supervises a child and can redirect its stdout."""

    if sys.platform != 'win32':
        def test_stdout_goes_where_it_is_sent_and_the_status_returns(self):
            with tempfile.TemporaryFile() as target:
                status = slot.run_command([sys.executable, '-c', 'print("hi"); raise SystemExit(3)'], stdout=target)
                target.seek(0)
                self.assertEqual(target.read(), b'hi\n')
            self.assertEqual(status, 3)

        def test_a_signalled_child_reports_the_negative_status_it_returned(self):
            status = slot.run_command([sys.executable, '-c', 'import os, signal; os.kill(os.getpid(), signal.SIGTERM)'])
            self.assertEqual(status, -signal.SIGTERM)

        def test_the_child_runs_in_its_own_session(self):
            with tempfile.TemporaryFile() as target:
                code = 'import os; print(os.getsid(0) == os.getpid())'
                slot.run_command([sys.executable, '-c', code], stdout=target)
                target.seek(0)
                self.assertEqual(target.read(), b'True\n')

        def test_a_cancellation_signal_terminates_the_child_and_raises(self):
            timer = threading.Timer(.5, os.kill, (os.getpid(), signal.SIGTERM))
            timer.start()
            try:
                with self.assertRaises(slot.Cancelled):
                    slot.run_command([sys.executable, '-c', 'import time; time.sleep(60)'])
            finally:
                timer.cancel()
            self.assertEqual(slot.cancellation_status(), 128 + signal.SIGTERM)

        def test_a_spawn_failure_propagates(self):
            with self.assertRaises(OSError):
                slot.run_command(['/nonexistent/oxpinyin-command'])

        def test_a_cancellation_pending_before_the_call_starts_nothing(self):
            with slot.cancellation_handlers(), mock.patch.object(slot.subprocess, 'Popen') as launch:
                slot.CANCELLED.set()
                with self.assertRaises(slot.Cancelled):
                    slot.run_command([sys.executable, '-c', 'pass'])
            launch.assert_not_called()


class RunCommandWindowsTests(unittest.TestCase):
    """The Job Object branch takes cwd and stdout as the POSIX branch does."""

    def test_cwd_and_stdout_reach_the_job_object_launch(self):
        with tempfile.TemporaryFile() as target, test_support.windows_launch(status=3) as (launch, jobs, ntdll):
            status = slot.run_command(['cargo', 'x'], env={}, cwd='work', stdout=target)
            self.assertIs(launch.call_args.kwargs['stdout'], target)
        self.assertEqual(status, 3)
        test_support.assert_windows_launches(self, launch, jobs, ntdll, cwd='work')


class SpawnWindowsTests(unittest.TestCase):
    """spawn starts a Windows child suspended in a Job Object and keeps the job on the process."""

    def test_the_child_joins_its_job_before_it_runs_and_keeps_it(self):
        with test_support.windows_launch() as (launch, jobs, ntdll):
            process = slot.spawn(['child'], cwd='work')
            test_support.assert_windows_launches(self, launch, jobs, ntdll, cwd='work')
            self.assertIs(process.job, jobs[0])
            slot.release(process)
        self.assertTrue(jobs[0].closed)

    def test_a_child_that_cannot_resume_is_ended_and_its_job_closed(self):
        with test_support.windows_launch() as (launch, jobs, ntdll):
            ntdll.NtResumeProcess.return_value = 1
            with self.assertRaises(OSError):
                slot.spawn(['child'])
        self.assertTrue(jobs[0].closed)

    def test_a_spawn_failure_closes_the_job(self):
        with test_support.windows_launch() as (launch, jobs, ntdll):
            launch.side_effect = FileNotFoundError('child')
            with self.assertRaises(FileNotFoundError):
                slot.spawn(['child'])
        self.assertTrue(jobs[0].closed)


class CancellationTests(unittest.TestCase):
    def test_handlers_clear_state_on_entry_and_restore_the_previous_handlers_on_exit(self):
        slot.CANCELLED.set()
        previous = signal.getsignal(signal.SIGTERM)
        with slot.cancellation_handlers():
            self.assertFalse(slot.CANCELLED.is_set())
            self.assertIsNot(signal.getsignal(signal.SIGTERM), previous)
        self.assertEqual(signal.getsignal(signal.SIGTERM), previous)

    def test_a_nested_context_keeps_the_outer_cancellation(self):
        with slot.cancellation_handlers():
            slot.CANCELLED.set()
            with slot.cancellation_handlers():
                self.assertTrue(slot.CANCELLED.is_set())

    def test_status_is_130_without_a_signal_and_128_plus_the_signal_after_one(self):
        with mock.patch.object(slot, 'CANCELLED_SIGNAL', None):
            self.assertEqual(slot.cancellation_status(), 130)
        # SIGHUP exists only on POSIX; on Windows the handlers record SIGTERM (or SIGINT, which is 130).
        recorded, expected = (signal.SIGTERM, 143) if sys.platform == 'win32' else (signal.SIGHUP, 128 + signal.SIGHUP)
        with mock.patch.object(slot, 'CANCELLED_SIGNAL', recorded):
            self.assertEqual(slot.cancellation_status(), expected)


class WaitChildTests(unittest.TestCase):
    if sys.platform != 'win32':
        def start(self, code):
            return subprocess.Popen([sys.executable, '-c', code], start_new_session=True)

        def test_a_timeout_terminates_and_reaps_the_group(self):
            with slot.cancellation_handlers(), self.start('import time; time.sleep(60)') as process:
                with self.assertRaises(subprocess.TimeoutExpired):
                    slot.wait_child(process, timeout=.3)
                self.assertIsNotNone(process.poll())

        def test_descendants_in_the_group_are_killed(self):
            code = ('import subprocess, sys, time\n'
                    'child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])\n'
                    'print(child.pid, flush=True); time.sleep(60)')
            with slot.cancellation_handlers(), subprocess.Popen([sys.executable, '-c', code], stdout=subprocess.PIPE,
                                                                start_new_session=True) as process:
                grandchild = int(process.stdout.readline())
                with self.assertRaises(subprocess.TimeoutExpired):
                    slot.wait_child(process, timeout=.3)
                deadline = time.monotonic() + 5
                while running(grandchild):
                    if time.monotonic() >= deadline:
                        self.fail('the grandchild survived the group kill')
                    time.sleep(.05)


if __name__ == '__main__':
    unittest.main()
