#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the shared measurement services: bench.py, identity.py and reports.py.

Counts, the option grammar, the order of the runtime guard, the worker's
environment and argv, the pool that builds, and the records the runners write.
Nothing here needs taskset, binutils or a proc filesystem: the host readers take a
fixture root and the tools are replaced by fakes, so the file runs on macOS and Linux.
Real children are plain Python.
"""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
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


class CountTests(unittest.TestCase):
    def test_a_plain_decimal_is_a_count_and_prints_as_written(self):
        for text, number in (('8', 8), ('08', 8), ('010', 10), ('0', 0), ('20', 20)):
            with self.subTest(text=text):
                value = bench.decimal_count('run-x.sh', 'PERF_RUNS', text)
                self.assertEqual((int(value), str(value), f'{value}'), (number, text, text))
                self.assertEqual(list(range(value)), list(range(number)))

    def test_anything_else_exits_2_naming_the_variable_and_the_value(self):
        for text in ('2+3', '-1', '', ' 5', '5 ', '5\n', '٣', '0x10', '1e3', '$((1))'):
            with self.subTest(text=text):
                _, status, out, err = stopped(bench.decimal_count, 'run-x.sh', 'PERF_RUNS', text)
                self.assertEqual(status, 2)
                self.assertEqual(out, '')
                self.assertEqual(err, f"run-x.sh: PERF_RUNS must be a decimal count (got '{text}')\n")

    def test_an_unset_or_empty_variable_takes_the_default(self):
        for env in ({}, {'PERF_RUNS': ''}):
            with self.subTest(env=env):
                self.assertEqual(int(bench.count('run-x.sh', env, 'PERF_RUNS', '20')), 20)
        self.assertEqual(int(bench.count('run-x.sh', {'PERF_RUNS': '08'}, 'PERF_RUNS', '20')), 8)
        self.assertEqual(bench.setting({'A': ''}, 'A', 'x'), 'x')
        self.assertEqual(bench.setting({'A': '0'}, 'A', 'x'), '0')


class FunctionOrderArgumentTests(unittest.TestCase):
    def test_only_the_two_options_with_values_are_accepted(self):
        argv = ['--function-order', 'pr/pinyin=/p.order', '--unordered-arm', 'pr', '--function-order', 'a=b']
        self.assertEqual(bench.function_order_arguments(argv), argv)
        self.assertEqual(bench.function_order_arguments([]), [])

    def test_a_missing_or_empty_value_stops_silently_with_status_2(self):
        for argv in (['--function-order'], ['--unordered-arm', ''], ['--unordered-arm', 'x', '--function-order']):
            with self.subTest(argv=argv):
                _, status, out, err = stopped(bench.function_order_arguments, argv)
                self.assertEqual((status, out, err), (2, '', ''))

    def test_anything_else_names_itself(self):
        _, status, _, err = stopped(bench.function_order_arguments, ['--unordered-arm', 'x', '--bogus'])
        self.assertEqual((status, err), (2, 'unknown function-order argument: --bogus\n'))


class PathTests(Scratch):
    def test_the_objects_directory_comes_first_on_the_loader_path(self):
        self.assertEqual(bench.loader_path(Path('/a/lib'), None), '/a/lib')
        self.assertEqual(bench.loader_path(Path('/a/lib'), ''), '/a/lib')
        self.assertEqual(bench.loader_path('/a/lib', '/x:/y'), '/a/lib:/x:/y')

    def test_pinning_is_required_or_only_when_taskset_exists(self):
        self.assertEqual(bench.pinned(['w', '--perf'], 3, required=True, which=lambda name: None),
                         ['taskset', '-c', '3', 'w', '--perf'])
        self.assertEqual(bench.pinned(['w'], '0', required=False, which=lambda name: '/usr/bin/taskset'),
                         ['taskset', '-c', '0', 'w'])
        self.assertEqual(bench.pinned(['w', Path('/x')], 0, required=False, which=lambda name: None),
                         ['w', '/x'])

    def test_find_named_walks_in_sorted_pre_order_and_accepts_links(self):
        (self.root / 'b/lib').mkdir(parents=True)
        (self.root / 'a/deep').mkdir(parents=True)
        (self.root / 'b/lib/libpinyin.so').write_bytes(b'')
        (self.root / 'a/deep/libpinyin.so').symlink_to('/nonexistent')
        (self.root / 'a/libpinyin.so.15').write_bytes(b'')
        self.assertEqual(bench.find_named(self.root, 'libpinyin.so'), self.root / 'a/deep/libpinyin.so')
        self.assertIsNone(bench.find_named(self.root, 'absent'))
        self.assertIsNone(bench.find_named(self.root / 'missing', 'libpinyin.so'))

    def test_find_named_needs_a_file_or_link_unless_any_type(self):
        (self.root / 'libpinyin.pc').mkdir()
        self.assertIsNone(bench.find_named(self.root, 'libpinyin.pc'))
        self.assertEqual(bench.find_named(self.root, 'libpinyin.pc', any_type=True), self.root / 'libpinyin.pc')

    def test_find_library_stops_with_the_runners_message(self):
        _, status, _, err = stopped(bench.find_library, self.root)
        self.assertEqual((status, err), (1, f'fatal: libpinyin.so not found under {self.root}\n'))

    def test_working_directory_restores_the_caller(self):
        before = os.getcwd()
        with bench.working_directory(self.root):
            self.assertEqual(Path(os.getcwd()).resolve(), self.root)
        self.assertEqual(os.getcwd(), before)

    def test_copy_file_and_tee(self):
        source, target = self.root / 'a', self.root / 'b'
        source.write_bytes(b'x')
        bench.copy_file(source, target)
        self.assertEqual(target.read_bytes(), b'x')
        _, status, _, err = stopped(bench.copy_file, self.root / 'nope', target)
        self.assertEqual((status, err), (1, f"cp: cannot stat '{self.root}/nope': No such file or directory\n"))
        _, status, _, err = stopped(bench.copy_file, source, self.root / 'missing-directory' / 'file')
        self.assertEqual((status, err), (1, f"cp: cannot create regular file '{self.root}/missing-directory/file': "
                                            'No such file or directory\n'))
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            bench.tee(self.root / 'table.txt', 'one\ntwo\n')
        self.assertEqual((out.getvalue(), (self.root / 'table.txt').read_text()), ('one\ntwo\n',) * 2)


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

    def test_a_script_wrapper_or_the_object_itself_is_not_checked_again(self):
        script = self.root / 'wrapper'
        script.write_text('#!/bin/sh\n')
        script.chmod(0o755)
        bench.guard('run-x.sh', 'auto', 'pin', self.so, [str(script)], env={'PATH': ''})
        bench.guard('run-x.sh', 'auto', 'pin', self.so, [str(self.so)], env={'PATH': ''})
        self.assertEqual([call[3] for call in self.calls], [str(self.so)] * 2)

    def test_mach_o_counts_as_native_and_a_missing_file_does_not(self):
        macho = self.root / 'macho'
        macho.write_bytes(b'\xcf\xfa\xed\xfe rest')
        self.assertTrue(bench.native(macho))
        self.assertTrue(bench.native(self.so))
        self.assertFalse(bench.native(self.root / 'missing'))

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

    def test_no_libdir_leaves_the_loader_path_and_no_repeats_leaves_the_variable_unset(self):
        arm = bench.Arm('L', Path('/x.so'), Path('/d'))
        env = self.bench().environment(arm, 'rss-diag', '8')
        self.assertEqual(env['LD_LIBRARY_PATH'], '/inherited')
        self.assertNotIn('PERF_REPEATS', env)

    def test_launch_checks_then_pins_the_harness_on_the_data(self):
        done = self.bench(required=True).launch(self.arm, 'speed', 8, repeats=1)
        self.assertEqual(done.stdout, b'{"a":1}\n')
        kinds = [event[0] for event in self.events]
        self.assertEqual(kinds, ['guard', 'capture'])
        self.assertEqual(self.events[0][1], ('run-x.sh', 'auto', 'oxpinyin', Path('/lib/libpinyin.so')))
        self.assertEqual(self.events[1][1], ['taskset', '-c', '2', '/h/bisect', '--perf', '/lib/libpinyin.so', '/data'])

    def test_a_profiler_prefix_comes_before_the_harness_and_is_what_the_guard_sees(self):
        self.bench(required=True).launch(self.arm, 'speed', 4, prefix=['valgrind', '--tool=callgrind'])
        self.assertEqual(self.events[1][1][3:5], ['valgrind', '--tool=callgrind'])
        self.assertEqual(self.events[1][1][-4:], ['/h/bisect', '--perf', '/lib/libpinyin.so', '/data'])

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

    def test_both_files_exist_before_the_check_refuses(self):
        out, err = self.root / 'o', self.root / 'e'
        with mock.patch.object(bench, 'guard', side_effect=Finished(1)):
            with self.assertRaises(Finished):
                self.bench().sample(self.arm, 'speed', out, err, 8)
        self.assertEqual((out.read_bytes(), err.read_bytes()), (b'', b''))

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

    def test_round_robin_runs_every_entry_once_per_round_in_order(self):
        a, b = bench.Arm('a', Path('a'), Path('d')), bench.Arm('b', Path('b'), Path('d'))
        calls = []
        bench.round_robin(2, [(a, 'ram-init'), (b, 'ram-init'), (a, 'ram-cycle')],
                          lambda arm, mode: calls.append((arm.label, mode)))
        self.assertEqual(calls, [('a', 'ram-init'), ('b', 'ram-init'), ('a', 'ram-cycle')] * 2)
        bench.round_robin(0, [(a, 'speed')], lambda *args: calls.append('never'))
        self.assertNotIn('never', calls)


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

    def test_run_can_send_the_childs_stdout_elsewhere(self):
        with open(self.root / 'o.txt', 'wb') as target:
            bench.run([sys.executable, '-c', 'print("hello")'], stdout=target)
        self.assertEqual((self.root / 'o.txt').read_text(), 'hello\n')

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

    def test_heavy_runs_under_the_slot_with_the_trailer_and_returns_the_shell_status(self):
        with mock.patch.object(bench.slot, 'run_command', return_value=-15) as run_command:
            _, status, _, _ = stopped(bench.heavy, ['cargo', Path('x')], env={'A': 'b'})
        self.assertEqual(status, 128 + 15)
        self.assertEqual(run_command.call_args.args, (['cargo', 'x'],))
        test_support.assert_calls(self, run_command, [mock.call(['cargo', 'x'], env={'A': 'b'}, stdout=None, report=True)])
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

    def test_an_integer_exit_or_return_is_the_status(self):
        self.assertEqual(bench.report(self.script('def main(argv):\n    raise SystemExit(4)\n'), [])[0], 4)
        self.assertEqual(bench.report(self.script('def main(argv):\n    return 2\n'), [])[0], 2)
        self.assertEqual(bench.report(self.script('def main(argv):\n    raise SystemExit\n'), [])[0], 0)

    def test_a_script_that_cannot_be_opened_is_the_interpreters_complaint_and_status_2(self):
        missing = self.root / 'ctl/bisection/perf-ratio.py'
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            result = bench.report(str(missing), ['x'])
        self.assertEqual((result, err.getvalue()),
                         ((2, ''), f"{sys.executable}: can't open file '{missing}': [Errno 2] No such file or directory\n"))

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


class ExportedTests(unittest.TestCase):
    def test_the_environment_is_swapped_for_the_body_and_restored_after(self):
        before = dict(os.environ)
        with bench.exported({'ONLY': 'this'}):
            self.assertEqual(dict(os.environ), {'ONLY': 'this'})
        self.assertEqual(dict(os.environ), before)

    def test_it_is_restored_when_the_body_raises(self):
        before = dict(os.environ)
        with self.assertRaises(RuntimeError):
            with bench.exported({'ONLY': 'this'}):
                raise RuntimeError
        self.assertEqual(dict(os.environ), before)

    def test_no_variables_leave_the_environment_alone(self):
        before = dict(os.environ)
        with bench.exported(None):
            self.assertEqual(dict(os.environ), before)
        self.assertEqual(dict(os.environ), before)

    def test_the_function_order_tool_runs_in_the_environment_it_was_given(self):
        seen = []

        class Tool:
            @staticmethod
            def main(argv):
                seen.append((list(argv), os.environ.get('PKG_CONFIG_PATH')))

        with mock.patch.object(bench, 'script', return_value=Tool):
            bench.check_function_order([('ox/pinyin', Path('/a.so'))], ['--unordered-arm', 'ox'],
                                       references=['pin'], env={'PKG_CONFIG_PATH': '/stage'})
            bench.check_function_order([('ox/pinyin', Path('/a.so'))], [])
        self.assertEqual(seen, [(['--library', 'ox/pinyin=/a.so', '--reference-arm', 'pin', '--unordered-arm', 'ox'],
                                 '/stage'),
                                (['--library', 'ox/pinyin=/a.so'], os.environ.get('PKG_CONFIG_PATH'))])


class FakeAdmission:
    """HeavyAdmission stand-in: admits a fixed worker and job count and records its lifecycle."""
    workers = 4
    jobs = 2
    events = None

    def __init__(self, label, env, out=None, runner=False, prepare_heavy_env=None):
        self.env = {pool.POOL_WORKERS: str(self.workers), pool.POOL_JOBS: str(self.jobs)}
        self.slot = type('Slot', (), {'pass_fds': staticmethod(lambda: ()), 'handoff': staticmethod(lambda: None)})()
        self.events.append('admission')

    def try_acquire(self):
        return True

    def close(self):
        self.events.append('close')


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.events = []
        FakeAdmission.events, FakeAdmission.workers, FakeAdmission.jobs = self.events, 4, 2
        for patcher in (mock.patch.object(slot, 'HeavyAdmission', FakeAdmission),
                        mock.patch.object(host, 'allowed_cpus', lambda: {0, 1, 2, 3}),
                        mock.patch.object(slot.CANCELLED, 'is_set', lambda: False)):
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_builds_get_the_planned_jobs_and_replay_their_output_in_build_order(self):
        seen, lock = {}, threading.Lock()
        release = threading.Event()

        def build(name, wait):
            def run(context, jobs):
                with lock:
                    seen[name] = jobs
                if wait:
                    release.wait(5)
                else:
                    release.set()
                return f'{name} output\n'.encode()
            return run
        err = io.BytesIO()
        with mock.patch.object(bench, 'write', lambda data, error=False: err.write(data)):
            with contextlib.redirect_stderr(io.StringIO()):
                bench.build_all([('first', build('first', True)), ('second', build('second', False))],
                                'builds', env={})
        self.assertEqual(seen, {'first': 2, 'second': 2})
        self.assertEqual(err.getvalue(), b'first output\nsecond output\n')
        self.assertEqual(self.events, ['admission', 'close'])

    def test_the_earliest_failure_ends_the_run_with_its_status(self):
        def failing(context, jobs):
            raise Finished(101)
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(Finished) as stop:
                bench.build_all([('a', lambda context, jobs: b''), ('b', failing)], 'builds', env={})
        self.assertEqual(stop.exception.status, 101)
        self.assertEqual(self.events[-1], 'close')

    def test_cargo_builds_with_its_share_unless_the_caller_chose(self):
        launched = []

        class Context:
            environment = {'A': 'b'}

            def launch(self, command, *, env, merge_stderr):
                launched.append((command, env, merge_stderr))
                return completed(0, b'built\n')
        self.assertEqual(bench.cargo(Context(), ['cargo', Path('x')], 2), b'built\n')
        self.assertEqual(launched[0], (['cargo', 'x'], {'A': 'b', 'CARGO_BUILD_JOBS': '2'}, True))
        Context.environment = {'CARGO_BUILD_JOBS': '7'}
        bench.cargo(Context(), ['cargo'], 2)
        self.assertEqual(launched[1][1]['CARGO_BUILD_JOBS'], '7')

    def test_a_failed_cargo_replays_its_output_and_ends_the_run(self):
        class Context:
            environment = {}

            def launch(self, command, *, env, merge_stderr):
                return completed(101, b'error[E0000]\n')
        err = io.BytesIO()
        with mock.patch.object(bench, 'write', lambda data, error=False: err.write(data)):
            with self.assertRaises(Finished) as stop:
                bench.cargo(Context(), ['cargo'], 1)
        self.assertEqual((stop.exception.status, err.getvalue()), (101, b'error[E0000]\n'))


class AdmittedBuildTests(Scratch):
    """build_all against the real slot admission on a fake host: the plan comes from the admitted CPUs and memory."""

    def setUp(self):
        super().setUp()
        locks = self.root / 'locks'
        locks.mkdir()
        environment = {name: value for name, value in os.environ.items()
                       if name not in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', slot.HEAVY_SLOT_HELD)}
        environment.update(OXPINYIN_HEAVY_SLOTS='1', OXPINYIN_RESOURCE_WAIT_SECONDS='0')
        for patcher in (mock.patch.dict(os.environ, environment, clear=True),
                        mock.patch.object(host, 'host_lock_dir', return_value=locks),
                        mock.patch.object(slot.CANCELLED, 'is_set', lambda: False)):
            patcher.start()
            self.addCleanup(patcher.stop)

    def fake_host(self, available, cpus, total=4096):
        """A host with AVAILABLE MiB free of TOTAL and CPUS CPUs, all of them allowed."""
        snapshot = dict(available_mib=available, total_mib=total, memory_limit_mib=None, cpus=cpus,
                        cargo_cpus=cpus, notes=[f'synthetic {available} MiB, {cpus} CPUs'])
        for patcher in (mock.patch.object(host, 'allowed_cpus', lambda: set(range(cpus))),
                        mock.patch.object(host, 'resource_snapshot', lambda *args, **kwargs: dict(snapshot))):
            patcher.start()
            self.addCleanup(patcher.stop)

    def build(self, count, expected=0, **options):
        """Run COUNT builds that record the jobs they got; return (jobs by name, most running at once).

        With EXPECTED, each build waits until that many run together, so a narrower pool cannot pass.
        """
        lock, together = threading.Lock(), threading.Event()
        seen, running, peak = {}, [0], [0]

        def build(name):
            def run(context, jobs):
                with lock:
                    seen[name] = jobs
                    running[0] += 1
                    peak[0] = max(peak[0], running[0])
                    if running[0] == expected:
                        together.set()
                if expected:
                    self.assertTrue(together.wait(5), 'the builds did not overlap')
                time.sleep(0.02)
                with lock:
                    running[0] -= 1
            return run
        self.started = seen
        with contextlib.redirect_stderr(io.StringIO()):
            bench.build_all([(str(index), build(str(index))) for index in range(count)], 'builds', **options)
        return seen, peak[0]

    def test_a_host_with_many_cpus_and_little_memory_builds_with_fewer_jobs(self):
        # 32 CPUs and 4 GiB, two builds: a fixed share of 16 jobs a build is 9,600 MiB for even one
        # worker, which this host never admits. The plan holds the build to the jobs the memory carries.
        self.fake_host(available=4096, cpus=32)
        seen, peak = self.build(2)
        self.assertEqual(sorted(seen), ['0', '1'])
        jobs = set(seen.values())
        self.assertEqual(len(jobs), 1, jobs)
        (jobs,) = jobs
        self.assertTrue(1 <= jobs < 16, jobs)
        self.assertLessEqual(peak * jobs * host.RUSTC_PEAK_MIB, 4096 - host.desktop_headroom_mib(4096))

    def test_a_build_is_admitted_on_any_cpu_count_while_memory_holds_a_rustc(self):
        room = 4096 - host.desktop_headroom_mib(4096)
        for cpus in (1, 2, 3, 4, 8, 16, 32, 64, 128, 256):
            with self.subTest(cpus=cpus):
                self.fake_host(available=4096, cpus=cpus)
                seen, peak = self.build(4)
                self.assertEqual(len(seen), 4)
                self.assertLessEqual(peak * max(seen.values()) * host.RUSTC_PEAK_MIB, room)
                self.assertLessEqual(peak * max(seen.values()), cpus)

    def test_a_roomy_host_overlaps_builds_within_its_cpus(self):
        self.fake_host(available=65536, cpus=8, total=65536)
        seen, peak = self.build(4, expected=4)
        self.assertEqual((set(seen.values()), peak), ({2}, 4))

    def test_an_explicit_job_count_is_kept_and_charged_in_full(self):
        # 3 jobs at 600 MiB is 1,800 MiB a build: two fit the 3,687 MiB the host admits.
        self.fake_host(available=4096, cpus=32)
        seen, peak = self.build(4, expected=2, env=dict(os.environ, CARGO_BUILD_JOBS='3'))
        self.assertEqual((len(seen), set(seen.values()), peak), (4, {3}, 2))

    def test_an_explicit_job_count_that_does_not_fit_is_refused_and_not_reduced(self):
        self.fake_host(available=4096, cpus=32)
        with self.assertRaises(host.ResourceBlocked):
            self.build(2, env=dict(os.environ, CARGO_BUILD_JOBS='8'))    # 4,800 MiB a build
        self.assertEqual(self.started, {})

    def test_default_and_negative_job_counts_are_resolved_against_the_cpus(self):
        self.fake_host(available=65536, cpus=8, total=65536)
        for setting, jobs in (('default', 8), ('-1', 7), ('-20', 1), ('+4', 4)):
            with self.subTest(setting=setting):
                seen, _ = self.build(2, env=dict(os.environ, CARGO_BUILD_JOBS=setting))
                self.assertEqual(set(seen.values()), {jobs})


class IdentityTests(Scratch):
    def proc(self):
        proc = self.root / 'proc'
        (proc / 'sys/kernel').mkdir(parents=True)
        (proc / 'cpuinfo').write_text('processor\t: 0\nmodel name\t: Fancy CPU: v2 @ 3GHz\nmodel name\t: other\n')
        (proc / 'meminfo').write_text('MemFree:  1 kB\nMemTotal:       16384000 kB\nMemTotalX: 2\n')
        (proc / 'loadavg').write_text('0.10 0.20 0.30 1/234 5678\n')
        (proc / 'sys/kernel/perf_event_paranoid').write_text('2\n')
        return proc

    def test_cpu_memory_and_load_come_from_the_proc_files(self):
        proc = self.proc()
        self.assertEqual(identity.cpu_model(proc), 'Fancy CPU: v2 @ 3GHz')
        self.assertEqual(identity.memory_total(proc), 'MemTotal:       16384000 kB')
        self.assertEqual(identity.loadavg(proc), '0.10 0.20 0.30 1/234 5678')
        self.assertEqual(identity.loadavg(proc, fields=3), '0.10 0.20 0.30')
        self.assertEqual(identity.perf_event_paranoid(proc), '2')

    def test_a_host_without_those_files_reports_what_the_shell_did(self):
        empty = self.root / 'empty'
        empty.mkdir()
        self.assertEqual((identity.cpu_model(empty), identity.memory_total(empty)), ('', ''))
        self.assertEqual(identity.perf_event_paranoid(empty), 'unknown')
        (empty / 'cpuinfo').write_text('processor\t: 0\nCPU part\t: 0xd0c\n')
        self.assertEqual(identity.cpu_model(empty), '')

    def test_processors_are_the_snapshots_rounded_up_count(self):
        with mock.patch.object(host, 'resource_snapshot', return_value=dict(cpus=2, cargo_cpus=3)) as snapshot:
            self.assertEqual(identity.processors(self.root / 'proc', self.root / 'cgroup'), 3)
        snapshot.assert_called_once_with(self.root / 'proc', self.root / 'cgroup')
        self.assertGreaterEqual(identity.processors(), 1)

    def test_processors_differ_from_gnu_nproc_on_a_fractional_quota_and_the_omp_variables(self):
        # The rows are cpu.max of the process's cgroup and this count, on a fake Linux host with four CPUs in
        # the affinity set. GNU nproc 9.10 printed the same count for every row but the last three, where it
        # printed 1, 2 and 3 (docs/plans/tooling-python.md, U2).
        proc = self.proc()
        (proc / 'self').mkdir()
        (proc / 'self/cgroup').write_text('0::/\n')
        cgroup = self.root / 'cgroup'
        cgroup.mkdir()
        for quota, count in (('max 100000', 4), ('100000 100000', 1), ('200000 100000', 2), ('800000 100000', 4),
                             ('40000 100000', 1), ('150000 100000', 2), ('350000 100000', 4),
                             ('125000 100000', 2), ('240000 100000', 3), ('349000 100000', 4)):
            with self.subTest(quota=quota):
                (cgroup / 'cpu.max').write_text(quota + '\n')
                with mock.patch.object(sys, 'platform', 'linux'), \
                        mock.patch.object(host, 'allowed_cpus', return_value={0, 1, 2, 3}):
                    self.assertEqual(identity.processors(proc, cgroup), count)
        # Under a quota of 4 CPUs GNU nproc printed 2 for OMP_NUM_THREADS=2 and 3 for OMP_THREAD_LIMIT=3.
        (cgroup / 'cpu.max').write_text('400000 100000\n')
        with mock.patch.object(sys, 'platform', 'linux'), \
                mock.patch.object(host, 'allowed_cpus', return_value={0, 1, 2, 3, 4}), \
                mock.patch.dict(os.environ, {'OMP_NUM_THREADS': '2', 'OMP_THREAD_LIMIT': '3'}):
            self.assertEqual(identity.processors(proc, cgroup), 4)

    def test_uname_fields_follow_the_flags(self):
        names = os.uname()
        self.assertEqual(identity.uname('m'), names.machine)
        self.assertEqual(identity.uname('srvm'), f'{names.sysname} {names.release} {names.version} {names.machine}')

    def test_first_line_version_and_tool_failures(self):
        run = lambda command: completed(0, b'gcc (Debian 15) 15.2\nCopyright\n')
        self.assertEqual(identity.first_output_line(['gcc', '--version'], run=run), 'gcc (Debian 15) 15.2')

        def missing(command):
            raise FileNotFoundError(command[0])
        self.assertEqual(identity.first_output_line(['gcc'], run=missing), '')
        self.assertEqual(identity.package_version('tkrzw', run=missing), 'unknown')
        self.assertEqual(identity.package_version('tkrzw', run=lambda command: completed(1)), 'unknown')
        self.assertEqual(identity.package_version('tkrzw', run=lambda command: completed(0, b'1.0.34\n')), '1.0.34')

    def test_pc_version_prints_the_version_lines_or_unknown(self):
        pc = self.root / 'libpinyin.pc'
        pc.write_text('Name: libpinyin\nVersion: 2.11.92\nRequires: glib-2.0\n')
        self.assertEqual(identity.pc_version(pc), 'Version: 2.11.92')
        pc.write_text('Name: x\n')
        self.assertEqual(identity.pc_version(pc), 'unknown')
        self.assertEqual(identity.pc_version(self.root / 'absent.pc'), 'unknown')

    def test_needed_and_fixture_hooks_parse_the_tools_output(self):
        readelf = (b' 0x0000000000000001 (NEEDED)             Shared library: [libtkrzw.so.1]\n'
                   b' 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n'
                   b' 0x000000000000000e (SONAME)             Library soname: [libpinyin.so.15]\n')
        self.assertEqual(identity.needed('x.so', run=lambda command: completed(0, readelf)),
                         ['libtkrzw.so.1', 'libc.so.6'])
        nm = b'0000 T pinyin_init\n0010 T oxpinyin_init_for_fixtures\n0020 T oxpinyin_test_set_user_bigram\n'
        self.assertEqual(identity.fixture_hooks('x.so', run=lambda command: completed(0, nm)), 2)
        self.assertEqual(identity.fixture_hooks('x.so', run=lambda command: completed(1, nm)), 0)

    def test_a_failing_readelf_ends_the_run_with_its_status_and_message(self):
        err = io.BytesIO()
        with mock.patch.object(identity, 'write', lambda data, error=False: err.write(data)):
            with self.assertRaises(Finished) as stop:
                identity.needed('x.so', run=lambda command: completed(1, b'', b'readelf: Error: Not an ELF file\n'))
        self.assertEqual((stop.exception.status, err.getvalue()), (1, b'readelf: Error: Not an ELF file\n'))

    def test_sha256_matches_hashlib_and_realpath_follows_links(self):
        import hashlib
        (self.root / 'f').write_bytes(b'abc' * 1000)
        (self.root / 'link').symlink_to(self.root / 'f')
        self.assertEqual(identity.sha256(self.root / 'f'), hashlib.sha256(b'abc' * 1000).hexdigest())
        self.assertEqual(identity.realpath(self.root / 'link'), str((self.root / 'f').resolve()))


LINUX = sys.platform.startswith('linux')


@unittest.skipUnless(LINUX and shutil.which('gcc') and shutil.which('readelf') and shutil.which('nm'),
                     'needs gcc and GNU binutils (readelf, nm) on Linux')
class BinutilsTests(Scratch):
    """The parsers read what the real readelf and nm print, from a shared object gcc builds here."""

    def test_needed_and_fixture_hooks_of_a_real_shared_object(self):
        source = self.root / 'lib.c'
        source.write_text('#include <math.h>\n'
                          'double oxpinyin_init_for_fixtures(double x) { return sin(x); }\n'
                          'int oxpinyin_test_set_user_bigram(void) { return 0; }\n'
                          'int plain(void) { return 1; }\n')
        library = self.root / 'lib.so'
        subprocess.run(['gcc', '-shared', '-fPIC', '-o', str(library), str(source), '-lm'], check=True)
        names = identity.needed(library)
        self.assertTrue(any(name.startswith('libm.so') for name in names), names)
        self.assertEqual(identity.fixture_hooks(library), 2)

    def test_a_file_that_is_not_an_elf_ends_the_run_with_readelfs_status(self):
        (self.root / 'text').write_text('not an object')
        with contextlib.redirect_stderr(Stream()), self.assertRaises(Finished) as stop:
            identity.needed(self.root / 'text')
        self.assertNotEqual(stop.exception.status, 0)


@unittest.skipUnless(shutil.which('taskset'), 'needs taskset (util-linux)')
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

    def test_the_same_data_summary_prints_medians_per_cell(self):
        speed = self.write('speed.jsonl', [
            {'backend': 'a', 'init_ns': 2_000_000, 'alloc_ns': 1_000_000, 'cycles_ns': [9_000_000, 3_000_000, 5_000_000]},
            {'backend': 'a', 'init_ns': 4_000_000, 'alloc_ns': 3_000_000, 'cycles_ns': [7_000_000, 4_000_000]},
            {'backend': 'b', 'init_ns': 1_000_000, 'alloc_ns': 1_000_000, 'cycles_ns': [2_000_000, 1_000_000]}])
        init = self.write('init.jsonl', [{'backend': 'a', 'mode': 'ram-init', 'after_init': {'rss_kib': 100, 'hwm_kib': 120}},
                                         {'backend': 'a', 'mode': 'ram-init', 'rss_kib': 300, 'hwm_kib': 330}])
        cycle = self.write('cycle.jsonl', [{'backend': 'a', 'mode': 'ram-cycle', 'after_last': {'rss_kib': 500, 'hwm_kib': 900}}])
        expected = (f"{'cell':<18}{'init ms':>10}{'alloc ms':>10}{'cold ms':>10}{'steady ms':>11}"
                    f"{'rss-init KiB':>14}{'hwm-init KiB':>14}{'rss-cycle KiB':>15}{'hwm-cycle KiB':>15}\n"
                    f"{'a':<18}{3.0:>10.3f}{2.0:>10.3f}{8.0:>10.3f}{4.0:>11.3f}{200:>14.0f}{225:>14.0f}{500:>15.0f}{900:>15.0f}\n"
                    f"{'b':<18}{1.0:>10.3f}{1.0:>10.3f}{2.0:>10.3f}{1.0:>11.3f}{'nan':>14}{'nan':>14}{'nan':>15}{'nan':>15}\n")
        self.assertEqual(reports.same_data_summary(speed, init, cycle), expected)

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

    def test_an_empty_capture_prints_nothing(self):
        self.assertEqual(reports.hardware_instructions(self.write('hw.jsonl', [])), '')


if __name__ == '__main__':
    unittest.main()
