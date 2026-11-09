#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the shared measurement services: bench.py, identity.py and reports.py.

The backend identity, CPU pinning, the order of the runtime guard, the worker's
environment and argv, the launchers, the in-process reports and the summary.
The tools are replaced by fakes, so the file runs on macOS and Linux. The real
taskset class is defined on Linux only. Real children are plain Python.
"""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host, pool, slot, test_support
from tools.dev.measure import bench, identity, reports
from tools.dev.runners import Finished


def stopped(function, *arguments, **options):
    """Run FUNCTION and return (value, status, stdout, stderr); status is None unless it raised Finished."""
    out, err = io.StringIO(), io.StringIO()
    value, status = None, None
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            value = function(*arguments, **options)
        except Finished as stop:
            status = stop.status
    return value, status, out.getvalue(), err.getvalue()


class Stream(io.TextIOWrapper):
    """A text stream with a byte buffer behind it, as the console has: runners.write needs .buffer."""

    def __init__(self):
        super().__init__(io.BytesIO(), encoding='utf-8', write_through=True)

    def text(self) -> str:
        self.flush()
        return self.buffer.getvalue().decode()


def completed(status=0, stdout=b'', stderr=b''):
    return pool.Completed(status, stdout, stderr)


class Scratch(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix='bench-test-'))).resolve()


class BackendIdentityTests(unittest.TestCase):
    def test_exactly_one_backend_is_required_from_the_linked_library(self):
        for library, expected in [('libdb-5.3.so', 'bdb'), ('libdb.so.6', 'bdb'),
                                  ('libkyotocabinet.so.16', 'kc'), ('libtkrzw.so.1', 'tkrzw')]:
            with self.subTest(library=library):
                output = f'(NEEDED) [{library}]\n(NEEDED) [libc.so.6]'.encode()
                self.assertEqual(identity.library_backend('/subject', run=lambda _: bench.pool.Completed(0, output, b'')), expected)
        for output in (b'(NEEDED) [libc.so.6]', b'(NEEDED) [libdb.so]\n(NEEDED) [libtkrzw.so.1]'):
            with self.assertRaisesRegex(ValueError, 'expected exactly one backend'):
                identity.library_backend('/subject', run=lambda _: bench.pool.Completed(0, output, b''))
        with self.assertRaisesRegex(ValueError, 'cannot read library backend'):
            identity.library_backend('/subject', run=lambda _: bench.pool.Completed(1, b'', b'invalid ELF'))


class PathTests(Scratch):
    def test_pinning_is_required_or_only_when_taskset_exists(self):
        self.assertEqual(bench.pinned(['w', '--perf'], 3, required=True, which=lambda name: None),
                         ['taskset', '-c', '3', 'w', '--perf'])
        self.assertEqual(bench.pinned(['w'], '0', required=False, which=lambda name: '/usr/bin/taskset'),
                         ['taskset', '-c', '0', 'w'])
        self.assertEqual(bench.pinned(['w', Path('/x')], 0, required=False, which=lambda name: None),
                         ['w', '/x'])


class RuntimeGuardTests(Scratch):
    """The libtkrzw check runs on the object first, then on the native executable that opens it."""

    def setUp(self):
        super().setUp()
        self.calls = []
        patcher = mock.patch.object(bench, 'check', lambda cell, runner, side, obj, **kw: self.calls.append(
            (cell, runner, side, str(obj), kw['cwd'])))
        patcher.start()
        self.addCleanup(patcher.stop)
        self.so = self.root / 'lib.so'
        self.so.write_bytes(b'\x7fELF')
        self.worker = self.root / 'worker'
        self.worker.write_bytes(b'\x7fELF')
        self.worker.chmod(0o755)

    def test_the_object_then_a_native_executable_are_checked(self):
        bench.guard('run-x.sh', 'auto', 'oxpinyin', self.so, [str(self.worker), '--perf'], env={'PATH': ''})
        self.assertEqual([call[3] for call in self.calls], [str(self.so), str(self.worker)])
        self.assertEqual({call[:3] for call in self.calls}, {('auto', 'run-x.sh', 'oxpinyin')})

    def test_a_refusal_is_a_failed_run_with_the_resolvers_words(self):
        with mock.patch.object(bench, 'check', side_effect=bench.RuntimeRefused('run-x.sh: pin: libtkrzw resolved to /usr')):
            _, status, _, err = stopped(bench.runtime_check, 'run-x.sh', 'auto', 'pin', self.so, env={})
        self.assertEqual((status, err), (1, 'REFUSED: run-x.sh: pin: libtkrzw resolved to /usr\n'))


class BenchTests(Scratch):
    def setUp(self):
        super().setUp()
        self.events = []
        for name, replacement in (('guard', lambda *a, **k: self.events.append(('guard', a[:4], k['env']))),
                                  ('capture', self.fake_capture)):
            patcher = mock.patch.object(bench, name, replacement)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.status = 0
        self.arm = bench.Arm('oxpinyin', Path('/lib/libpinyin.so'), Path('/data'), Path('/lib'))

    def fake_capture(self, command, *, env=None, **kw):
        self.events.append(('capture', list(command), dict(env)))
        return completed(self.status, b'{"a":1}\n', b'warn\n')

    def bench(self, **options):
        defaults = dict(label='run-x.sh', env={'PATH': '/bin', 'LD_LIBRARY_PATH': '/inherited'},
                        harness=Path('/h/bisect'), cpu='2')
        return bench.Bench(**{**defaults, **options})

    def test_environment_puts_the_library_first_and_sets_the_perf_variables(self):
        env = self.bench().environment(self.arm, 'speed', 8, repeats=3, extra={'PERF_HW': 'instructions'})
        self.assertEqual({key: env[key] for key in env if key.startswith(('PERF_', 'LD_'))},
                         {'LD_LIBRARY_PATH': '/lib:/inherited', 'PERF_BACKEND': 'oxpinyin', 'PERF_MODE': 'speed',
                          'PERF_CYCLES': '8', 'PERF_REPEATS': '3', 'PERF_HW': 'instructions'})
        self.assertEqual(env['PATH'], '/bin')

    def test_launch_checks_then_pins_the_harness_on_the_data(self):
        done = self.bench(required=True).launch(self.arm, 'speed', 8, repeats=1)
        self.assertEqual(done.stdout, b'{"a":1}\n')
        kinds = [event[0] for event in self.events]
        self.assertEqual(kinds, ['guard', 'capture'])
        self.assertEqual(self.events[0][1], ('run-x.sh', 'auto', 'oxpinyin', Path('/lib/libpinyin.so')))
        self.assertEqual(self.events[1][1], ['taskset', '-c', '2', '/h/bisect', '--perf', '/lib/libpinyin.so', '/data'])

    def test_sample_appends_stdout_and_stderr_and_a_failure_ends_the_run_after_the_append(self):
        out, err = self.root / 'speed.jsonl', self.root / 'x.err'
        out.write_bytes(b'before\n')
        self.bench().sample(self.arm, 'speed', out, err, 8)
        self.bench().sample(self.arm, 'speed', out, err, 8)
        self.assertEqual(out.read_bytes(), b'before\n{"a":1}\n{"a":1}\n')
        self.assertEqual(err.read_bytes(), b'warn\nwarn\n')
        self.status = 3
        with self.assertRaises(Finished) as stop:
            self.bench().sample(self.arm, 'speed', out, err, 8)
        self.assertEqual(stop.exception.status, 3)
        self.assertEqual(out.read_bytes().count(b'{"a":1}'), 3)

    def test_a_refusal_at_launch_goes_to_the_workers_stderr_file_not_the_console(self):
        # The shell's redirection covered the worker's whole chain, its libtkrzw check included.
        out, err = self.root / 'o', self.root / 'e'
        console = Stream()
        refuse = lambda *a, **k: bench.fail('REFUSED: run-x.sh: oxpinyin: libtkrzw resolved to /usr/lib')
        with mock.patch.object(bench, 'guard', refuse), contextlib.redirect_stderr(console):
            with self.assertRaises(Finished) as stop:
                self.bench().sample(self.arm, 'speed', out, err, 8)
        self.assertEqual(stop.exception.status, 1)
        self.assertEqual(console.text(), '')
        self.assertEqual(err.read_text(), 'REFUSED: run-x.sh: oxpinyin: libtkrzw resolved to /usr/lib\n')
        self.assertEqual(out.read_bytes(), b'')

    def test_standalone_checks_the_object_in_its_loader_environment(self):
        seen = []
        with mock.patch.object(bench, 'check', lambda cell, runner, side, obj, **kw: seen.append(
                (cell, runner, side, str(obj), kw['env']['LD_LIBRARY_PATH']))):
            self.bench(cell='auto').standalone('pin', self.arm)
            self.bench(cell='auto').standalone('subject', self.arm, 'tkrzw')
        self.assertEqual(seen, [('auto', 'run-x.sh', 'pin', '/lib/libpinyin.so', '/lib:/inherited'),
                                ('tkrzw', 'run-x.sh', 'subject', '/lib/libpinyin.so', '/lib:/inherited')])


class ProcessTests(Scratch):
    """Real children, all plain Python, through the launchers the runners use."""

    def test_capture_returns_the_status_and_both_streams(self):
        done = bench.capture([sys.executable, '-c', 'import sys; sys.stdout.write("out"); '
                              'sys.stderr.write("err"); sys.exit(3)'])
        self.assertEqual((done.status, done.stdout, done.stderr), (3, b'out', b'err'))

    def test_a_killed_child_has_the_shell_status(self):
        done = bench.capture([sys.executable, '-c', 'import os, signal; os.kill(os.getpid(), signal.SIGKILL)'])
        self.assertEqual(done.status, 128 + 9)

    def test_capture_runs_in_the_given_environment_and_directory(self):
        done = bench.capture([sys.executable, '-c', 'import os; print(os.getcwd(), os.environ["X"])'],
                             env={'X': 'seen', 'PATH': os.environ.get('PATH', '')}, cwd=self.root)
        self.assertEqual(done.stdout.decode().split(), [str(self.root), 'seen'])

    def test_a_missing_command_is_file_not_found_for_the_shared_ladder(self):
        with self.assertRaises(FileNotFoundError):
            bench.capture(['no-such-command-for-bench-test'])

    def test_run_returns_or_enforces_the_status(self):
        self.assertEqual(bench.run([sys.executable, '-c', 'raise SystemExit(5)'], check_status=False), 5)
        with self.assertRaises(Finished) as stop:
            bench.run([sys.executable, '-c', 'raise SystemExit(5)'])
        self.assertEqual(stop.exception.status, 5)
        self.assertEqual(bench.run([sys.executable, '-c', 'pass']), 0)

    def test_run_forwards_stdin_and_can_merge_stderr_for_interchange(self):
        source = self.root / 'input'
        target = self.root / 'output'
        source.write_bytes(b'interchange')
        with source.open('rb') as stdin, target.open('wb') as stdout:
            bench.run([sys.executable, '-c',
                       'import sys; sys.stdout.buffer.write(sys.stdin.buffer.read()); sys.stdout.flush(); sys.stderr.write("diagnostic")'],
                      stdin=stdin, stdout=stdout, stderr=subprocess.STDOUT)
        self.assertEqual(target.read_bytes(), b'interchangediagnostic')

    def cancelled_while_running(self, launch, expected):
        """Cancel LAUNCH while its child and the child's own child sleep; both must be gone when it raises."""
        pids = self.root / 'pids'
        sleeper = (
            'import os, subprocess, sys, time\n'
            'grandchild = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])\n'
            f'open({str(pids)!r} + ".tmp", "w").write(f"{{os.getpid()}} {{grandchild.pid}}")\n'
            f'os.rename({str(pids)!r} + ".tmp", {str(pids)!r})\n'
            'time.sleep(60)\n')

        def cancel_when_running():
            deadline = time.monotonic() + 30
            while not pids.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            slot.CANCELLED.set()
        canceller = threading.Thread(target=cancel_when_running)
        self.addCleanup(slot.CANCELLED.clear)
        canceller.start()
        started = time.monotonic()
        try:
            with self.assertRaises(expected):
                launch([sys.executable, '-c', sleeper])
        finally:
            canceller.join()
        self.assertLess(time.monotonic() - started, 30)
        for pid in map(int, pids.read_text().split()):
            deadline = time.monotonic() + 5
            while not self.is_dead(pid) and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertTrue(self.is_dead(pid), f'process {pid} survived the cancellation')

    @staticmethod
    def is_dead(pid):
        if sys.platform == 'win32':
            return not test_support.process_running(pid)    # os.kill(pid, 0) is no liveness probe there
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        try:
            # A zombie is dead but waits for its adopter to reap it, and a container's init may never.
            return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] == 'Z'
        except OSError:
            return False

    def test_a_cancellation_reaps_a_captured_child_and_its_descendants(self):
        self.cancelled_while_running(bench.capture, (slot.Cancelled, pool.UnitStopped, pool.UnitsCancelled))

    def test_a_cancellation_reaps_a_console_child_and_its_descendants(self):
        self.cancelled_while_running(bench.run, slot.Cancelled)

    def test_heavy_runs_supervised_and_returns_the_shell_status(self):
        with mock.patch.object(bench.slot, 'run_command', return_value=-15) as run_command:
            _, status, _, _ = stopped(bench.heavy, ['cargo', Path('x')], env={'A': 'b'})
        self.assertEqual(status, 128 + 15)
        self.assertEqual(run_command.call_args.args, (['cargo', 'x'],))
        test_support.assert_calls(self, run_command, [mock.call(['cargo', 'x'], env={'A': 'b'}, stdout=None)])
        with mock.patch.object(bench.slot, 'run_command', return_value=7):
            self.assertEqual(bench.heavy(['true'], check_status=False), 7)


class ReportTests(Scratch):
    def setUp(self):
        super().setUp()
        self.scripts = 0
        bench.script.cache_clear()
        self.addCleanup(bench.script.cache_clear)

    def script(self, body):
        self.scripts += 1
        path = self.root / f'tool{self.scripts}.py'
        path.write_text(body)
        return str(path)

    def test_report_returns_the_status_and_what_the_script_printed(self):
        path = self.script('def main(argv):\n    print("rows", *argv)\n    return 0\n')
        self.assertEqual(bench.report(path, ['a', 'b']), (0, 'rows a b\n'))

    def test_a_system_exit_message_goes_to_stderr_and_is_status_1(self):
        path = self.script('def main(argv):\n    print("partial")\n    raise SystemExit("no rows")\n')
        (code, text), err = (None, None), io.StringIO()
        with contextlib.redirect_stderr(err):
            code, text = bench.report(path, [])
        self.assertEqual((code, text, err.getvalue()), (1, 'partial\n', 'no rows\n'))

    def test_a_crashing_script_prints_its_traceback_and_keeps_what_it_had_written(self):
        path = self.script('def main(argv):\n    print("rows so far")\n    {}["missing"]\n')
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code, text = bench.report(path, [])
        self.assertEqual((code, text), (1, 'rows so far\n'))
        self.assertTrue(err.getvalue().startswith('Traceback (most recent call last):\n'))
        self.assertTrue(err.getvalue().endswith("KeyError: 'missing'\n"))

    def test_a_cancellation_inside_the_script_is_not_a_crash(self):
        path = self.script('from tools.dev import slot\ndef main(argv):\n    raise slot.Cancelled("stop")\n')
        with self.assertRaises(slot.Cancelled):
            bench.report(path, [])

    def test_argparse_errors_name_the_script_and_the_process_is_put_back(self):
        path = self.script('import argparse\ndef main(argv):\n    argparse.ArgumentParser().parse_args(argv)\n')
        argv, main = sys.argv, sys.modules['__main__']
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code, _ = bench.report(path, ['--bogus'])
        self.assertEqual(code, 2)
        self.assertTrue(err.getvalue().startswith(f'usage: {Path(path).name} [-h]\n'), err.getvalue())
        self.assertIn(f'{Path(path).name}: error: unrecognized arguments: --bogus', err.getvalue())
        self.assertIs(sys.argv, argv)
        self.assertIs(sys.modules['__main__'], main)


LINUX = sys.platform.startswith('linux')


if LINUX:
    class TasksetTests(Scratch):
        def test_a_pinned_child_runs_on_the_chosen_cpu_and_reports_its_status(self):
            cpu = min(host.allowed_cpus())
            command = bench.pinned([sys.executable, '-c', 'import os, sys; sys.exit(7 if os.sched_getaffinity(0) == {%d} else 1)' % cpu], cpu)
            self.assertEqual(command[:3], ['taskset', '-c', str(cpu)])
            self.assertEqual(bench.capture(command).status, 7)


class SummaryTests(Scratch):
    def write(self, name, rows):
        path = self.root / name
        path.write_text(''.join(json.dumps(row) + '\n' for row in rows) + '\n')
        return path

    def test_hardware_instructions_reports_medians_and_the_ratio(self):
        capture = self.write('hw.jsonl', [
            {'backend': 'libpinyin', 'steady_hw_instructions': 1000, 'steady_hw_cycles': 4},
            {'backend': 'libpinyin', 'steady_hw_instructions': 3000, 'steady_hw_cycles': 4},
            {'backend': 'oxpinyin', 'steady_hw_instructions': 1500, 'steady_hw_cycles': 4}])
        text = reports.hardware_instructions(capture)
        self.assertEqual(text.splitlines()[:3], [
            'libpinyin: n=2 steady_cycles=[4] median=2000 per-cycle=500',
            'oxpinyin: n=1 steady_cycles=[4] median=1500 per-cycle=375',
            'hardware instruction ratio (oxpinyin / libpinyin): 0.7500'])
        self.assertEqual(len(text.splitlines()), 6)

    def test_a_run_with_no_counter_turns_the_summary_into_the_note(self):
        capture = self.write('hw.jsonl', [{'backend': 'x', 'hw_errno': 1}, {'backend': 'y', 'steady_hw_instructions': 5}])
        text = reports.hardware_instructions(capture)
        self.assertTrue(text.startswith('NO HARDWARE COUNTERS: a run reported none (hw_errno=1).\n'))
        self.assertIn('Distortion remains UNQUANTIFIED either way', text)
        self.assertNotIn('median', text)


if __name__ == '__main__':
    unittest.main()
