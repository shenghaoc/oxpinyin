#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
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

    def test_group_permission_errors_are_not_silenced(self):
        for platform in ('darwin', 'linux'):
            with self.subTest(platform=platform), mock.patch.object(sys, 'platform', platform), \
                    mock.patch.object(slot.os, 'killpg', side_effect=[None, PermissionError()]), \
                    mock.patch.object(slot.time, 'monotonic', side_effect=[0, 6]), \
                    mock.patch.object(slot.time, 'sleep') as sleep:
                with self.assertRaises(PermissionError):
                    slot.terminate_group(mock.Mock(pid=123))
                sleep.assert_not_called()


@unittest.skipIf(sys.platform == 'win32', 'the child is started in its own POSIX session')
class RunCommandTests(unittest.TestCase):
    """run_command supervises a child and can redirect its stdout and working directory."""

    def test_stdout_goes_where_it_is_sent_and_the_status_returns(self):
        with tempfile.TemporaryFile() as target:
            status = slot.run_command([sys.executable, '-c', 'print("hi"); raise SystemExit(3)'], stdout=target)
            target.seek(0)
            self.assertEqual(target.read(), b'hi\n')
        self.assertEqual(status, 3)

    def test_a_signalled_child_reports_the_negative_status_it_returned(self):
        status = slot.run_command([sys.executable, '-c', 'import os, signal; os.kill(os.getpid(), signal.SIGTERM)'])
        self.assertEqual(status, -signal.SIGTERM)

    def test_env_and_cwd_reach_the_child(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryFile() as target:
            code = 'import os; print(os.environ["PROBE"], os.getcwd())'
            self.assertEqual(slot.run_command([sys.executable, '-c', code], env=dict(os.environ, PROBE='x'),
                                              cwd=directory, stdout=target), 0)
            target.seek(0)
            self.assertEqual(target.read().decode(), f'x {os.path.realpath(directory)}\n')

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


class CancellationTests(unittest.TestCase):
    def test_check_cancelled_raises_only_while_the_event_is_set(self):
        with slot.cancellation_handlers():
            slot.check_cancelled()
            slot.CANCELLED.set()
            with self.assertRaises(slot.Cancelled):
                slot.check_cancelled()

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
        with mock.patch.object(slot, 'CANCELLED_SIGNAL', signal.SIGHUP):
            self.assertEqual(slot.cancellation_status(), 128 + signal.SIGHUP)


@unittest.skipIf(sys.platform == 'win32', 'POSIX process groups')
class WaitChildTests(unittest.TestCase):
    def start(self, code):
        return subprocess.Popen([sys.executable, '-c', code], start_new_session=True)

    def test_returns_the_status(self):
        with slot.cancellation_handlers(), self.start('raise SystemExit(5)') as process:
            self.assertEqual(slot.wait_child(process), 5)

    def test_a_timeout_terminates_and_reaps_the_group(self):
        with slot.cancellation_handlers(), self.start('import time; time.sleep(60)') as process:
            with self.assertRaises(subprocess.TimeoutExpired):
                slot.wait_child(process, timeout=.3)
            self.assertIsNotNone(process.poll())

    def test_cancellation_terminates_and_reaps_the_group(self):
        with slot.cancellation_handlers(), self.start('import time; time.sleep(60)') as process:
            slot.CANCELLED.set()
            with self.assertRaises(slot.Cancelled):
                slot.wait_child(process)
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
