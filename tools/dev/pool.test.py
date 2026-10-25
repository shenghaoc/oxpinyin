#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for tools/dev/pool.py: the plan arithmetic, the admission callback, the pool loop with a
fake launcher, and a real SIGTERM against the CLI. No platform is skipped."""
import argparse
import io
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host, pool


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


class PlanTests(unittest.TestCase):
    def test_workers_times_jobs_never_exceed_the_cpus(self):
        for tasks in range(1, 7):
            for cpus in range(1, 33):
                for memory in (None, 0, 400, 1000, 4000, 10**6):
                    with self.subTest(tasks=tasks, cpus=cpus, memory=memory):
                        chosen = pool.plan(tasks, cpus, memory)
                        self.assertGreaterEqual(chosen.workers, 1)
                        self.assertGreaterEqual(chosen.jobs, 1)
                        self.assertLessEqual(chosen.workers, tasks)
                        self.assertLessEqual(chosen.workers * chosen.jobs, max(cpus, chosen.jobs))

    def test_overlapped_builds_keep_two_make_jobs(self):
        for tasks in range(2, 7):
            for cpus in range(1, 33):
                chosen = pool.plan(tasks, cpus, 10**6)
                if chosen.workers > 1:
                    self.assertGreaterEqual(chosen.jobs, pool.MIN_OVERLAP_JOBS, (tasks, cpus))

    def test_ample_resources(self):
        self.assertEqual(pool.plan(3, 12, 10**6), (3, 4))
        self.assertEqual(pool.plan(3, 6, 10**6), (3, 2))
        self.assertEqual(pool.plan(3, 4, 10**6), (2, 2))
        self.assertEqual(pool.plan(3, 3, 10**6), (1, 3))
        self.assertEqual(pool.plan(3, 2, 10**6), (1, 2))
        self.assertEqual(pool.plan(3, 1, 10**6), (1, 1))
        self.assertEqual(pool.plan(1, 8, 10**6), (1, 8))

    def test_memory_counts_every_workers_make_jobs(self):
        def need(workers, jobs):
            return workers * (pool.BUILD_BASE_MIB + jobs * pool.BUILD_JOB_MIB)
        self.assertEqual(pool.plan(3, 12, need(3, 4)), (3, 4))
        self.assertEqual(pool.plan(3, 12, need(3, 4) - 1)[0], 2)
        for memory in range(0, 8000, 50):
            chosen = pool.plan(4, 16, memory)
            if chosen.workers > 1:
                self.assertLessEqual(need(*chosen), memory, memory)

    def test_short_memory_holds_a_single_build_below_the_cpu_count(self):
        self.assertEqual(pool.plan(3, 8, pool.BUILD_BASE_MIB + 2 * pool.BUILD_JOB_MIB), (1, 2))
        self.assertEqual(pool.plan(3, 8, 100), (1, 1))

    def test_unknown_memory_admits_no_overlap(self):
        self.assertEqual(pool.plan(3, 8, None), (1, 8))

    def test_the_estimate_is_every_workers_fixed_part_and_make_jobs(self):
        self.assertEqual(pool.estimate_mib(pool.Plan(2, 3)), 2 * (300 + 3 * 275))


def resources(available, cpus=4, total=16384, limit=None):
    return dict(available_mib=available, total_mib=total, cpus=cpus, cargo_cpus=cpus,
                memory_limit_mib=limit, notes=[])


class AdmissionPreparerTests(unittest.TestCase):
    def prepare(self, snapshot, tasks=3, cap=None, env=None):
        return pool.admission_preparer(tasks, cap)(dict(env or {}), snapshot)

    def test_the_plan_comes_from_the_admitted_room_after_headroom(self):
        headroom = host.desktop_headroom_mib(16384)
        child, chosen, estimate = self.prepare(resources(headroom + 10_000, cpus=12))
        self.assertEqual((child[pool.POOL_WORKERS], child[pool.POOL_JOBS]), ('3', '4'))
        self.assertEqual(chosen[pool.POOL_WORKERS][0], 3)
        self.assertEqual(estimate, pool.estimate_mib(pool.Plan(3, 4)))

    def test_the_reservation_never_exceeds_the_admitted_room(self):
        headroom = host.desktop_headroom_mib(16384)
        for room in range(0, 9000, 100):
            for cpus in (1, 2, 4, 8, 16):
                with self.subTest(room=room, cpus=cpus):
                    _, chosen, estimate = self.prepare(resources(headroom + room, cpus=cpus))
                    workers = chosen[pool.POOL_WORKERS][0]
                    if workers > 1:
                        self.assertLessEqual(estimate, room)

    def test_the_cpu_cap_limits_the_budget(self):
        child, _, _ = self.prepare(resources(10**6, cpus=16), cap=4)
        self.assertEqual((child[pool.POOL_WORKERS], child[pool.POOL_JOBS]), ('2', '2'))
        child, _, _ = self.prepare(resources(10**6, cpus=2), cap=16)
        self.assertEqual((child[pool.POOL_WORKERS], child[pool.POOL_JOBS]), ('1', '2'))

    def test_unknown_memory_plans_one_build_and_reserves_it(self):
        child, _, estimate = self.prepare(resources(None, cpus=8))
        self.assertEqual((child[pool.POOL_WORKERS], child[pool.POOL_JOBS]), ('1', '8'))
        self.assertEqual(estimate, pool.estimate_mib(pool.Plan(1, 8)))

    def test_a_hard_limited_compatibility_guest_keeps_no_headroom(self):
        child, _, _ = self.prepare(resources(2560, cpus=4, limit=2560),
                                   env={'OXPINYIN_COMPAT_CONTAINER': '1'})
        self.assertEqual((child[pool.POOL_WORKERS], child[pool.POOL_JOBS]), ('2', '2'))

    def test_the_callers_environment_is_kept(self):
        child, _, _ = self.prepare(resources(10**6), env={'KEEP': 'yes'})
        self.assertEqual(child['KEEP'], 'yes')


class FakeProcess:
    """A task whose exit the test scripts; records how the pool stopped it."""

    def __init__(self, name, status, runs, log):
        self.name, self.status, self.runs, self.log = name, status, runs, log
        self.terminated = self.killed = False
        self.waited = False

    def poll(self):
        if self.terminated or self.killed:
            return -signal.SIGTERM
        if self.runs > 0:
            self.runs -= 1
            return None
        return self.status

    def terminate(self):
        self.terminated = True

    def kill(self):
        self.killed = True

    def wait(self, timeout=None):
        self.waited = True
        return -signal.SIGTERM


class FakeLauncher:
    def __init__(self, statuses=None, runs=2, runs_for=None):
        self.statuses, self.runs, self.runs_for = statuses or {}, runs, runs_for or {}
        self.launched, self.live, self.peak = [], 0, 0
        self.processes = {}

    def launch(self, task):
        process = FakeProcess(task.name, self.statuses.get(task.name, 0),
                              self.runs_for.get(task.name, self.runs), task.log)
        self.launched.append(task.name)
        self.processes[task.name] = process
        return process


def tasks_of(*names, cleanup=True, directory=None):
    return [pool.Task(name, ['run', name], log=None if directory is None else directory / f'{name}.log',
                      cleanup=['cleanup', name] if cleanup else None) for name in names]


class PoolLoopTests(unittest.TestCase):
    def run_pool(self, tasks, workers, launcher, cancelled=lambda: False, out=None):
        cleaned = []
        result = pool.run_pool(tasks, workers, launcher, cancelled=cancelled, run_cleanup=cleaned.append,
                               poll=0, grace=0, out=out or io.StringIO(), sleep=lambda _: None)
        return result, cleaned

    def test_every_task_runs_in_submission_order_and_succeeds(self):
        launcher = FakeLauncher()
        result, cleaned = self.run_pool(tasks_of('a', 'b', 'c', 'd'), 2, launcher)
        self.assertTrue(result.ok)
        self.assertEqual(launcher.launched, ['a', 'b', 'c', 'd'])
        self.assertEqual(result.started, ['a', 'b', 'c', 'd'])
        self.assertEqual(cleaned, [])

    def test_no_more_than_the_workers_run_at_once(self):
        launcher = FakeLauncher(runs=3)
        peak = 0
        original = launcher.launch

        def launch(task):
            nonlocal peak
            process = original(task)
            peak = max(peak, sum(1 for p in launcher.processes.values() if p.runs > 0 or p is process))
            return process
        launcher.launch = launch
        result, _ = self.run_pool(tasks_of('a', 'b', 'c', 'd', 'e'), 2, launcher)
        self.assertTrue(result.ok)
        self.assertLessEqual(peak, 2)

    def test_one_worker_runs_one_task_at_a_time(self):
        launcher = FakeLauncher(runs=1)
        order = []
        original = launcher.launch

        def launch(task):
            order.append([name for name, p in launcher.processes.items() if p.runs > 0])
            return original(task)
        launcher.launch = launch
        self.run_pool(tasks_of('a', 'b', 'c'), 1, launcher)
        self.assertTrue(all(not live for live in order), order)

    def test_a_failure_starts_nothing_more_stops_the_running_and_cleans_up_started_tasks(self):
        launcher = FakeLauncher({'bad': 7}, runs=50, runs_for={'bad': 1})
        result, cleaned = self.run_pool(tasks_of('good', 'bad', 'later', 'last'), 2, launcher)
        self.assertFalse(result.ok)
        self.assertEqual(result.failed, {'bad': 7})
        self.assertEqual(launcher.launched, ['good', 'bad'])
        self.assertTrue(launcher.processes['good'].terminated)
        self.assertTrue(launcher.processes['good'].waited)
        self.assertEqual(sorted(map(tuple, cleaned)), [('cleanup', 'bad'), ('cleanup', 'good')])

    def test_cancellation_stops_queued_work_and_reaps_the_running(self):
        launcher = FakeLauncher(runs=10)
        calls = {'n': 0}

        def cancelled():
            calls['n'] += 1
            return calls['n'] > 3
        result, cleaned = self.run_pool(tasks_of('a', 'b', 'c', 'd'), 2, launcher, cancelled)
        self.assertTrue(result.cancelled)
        self.assertFalse(result.ok)
        self.assertEqual(launcher.launched, ['a', 'b'])
        for name in ('a', 'b'):
            self.assertTrue(launcher.processes[name].terminated)
            self.assertTrue(launcher.processes[name].waited)
        self.assertEqual(sorted(map(tuple, cleaned)), [('cleanup', 'a'), ('cleanup', 'b')])

    def test_a_task_that_ignores_sigterm_is_killed_after_the_grace(self):
        launcher = FakeLauncher({'bad': 1}, runs=50, runs_for={'bad': 1})

        class Stubborn(FakeProcess):
            def wait(self, timeout=None):
                self.waited = True
                return None if not self.killed else -signal.SIGKILL
        original = launcher.launch

        def launch(task):
            process = original(task)
            stubborn = Stubborn(task.name, process.status, process.runs, task.log)
            launcher.processes[task.name] = stubborn
            return stubborn
        launcher.launch = launch
        self.run_pool(tasks_of('stubborn', 'bad'), 2, launcher)
        self.assertTrue(launcher.processes['stubborn'].killed)

    def test_the_result_does_not_depend_on_completion_order(self):
        for runs in (0, 1, 3):
            launcher = FakeLauncher({'b': 2}, runs=runs)
            result, _ = self.run_pool(tasks_of('a', 'b', 'c'), 3, launcher)
            self.assertEqual(result.failed, {'b': 2}, runs)

    def test_a_failed_task_prints_the_tail_of_its_log(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            (directory / 'bad.log').write_text(''.join(f'line {n}\n' for n in range(100)))
            out = io.StringIO()
            launcher = FakeLauncher({'bad': 3}, runs=0)
            self.run_pool(tasks_of('bad', directory=directory), 1, launcher, out=out)
            text = out.getvalue()
            self.assertIn('bad: failed with status 3', text)
            self.assertIn('line 99', text)
            self.assertNotIn('line 59\n', text)

    def test_a_successful_task_removes_its_work_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory) / 'work'
            work.mkdir()
            (work / 'file').write_text('x')
            task = pool.Task('a', ['run'], remove=work)
            result, _ = self.run_pool([task], 1, FakeLauncher(runs=0))
            self.assertTrue(result.ok)
            self.assertFalse(work.exists())


class CommandTemplateTests(unittest.TestCase):
    def test_placeholders_are_filled_per_item_and_other_braces_are_kept(self):
        args = argparse.Namespace(
            items=['bdb', 'kc'], log='/logs/{item}.log', cleanup='docker rm -f name-{item}',
            remove='/build/{item}', command=['build', '--dbm', '{item}', '--jobs', '{jobs}', '--format', '{{.ID}}'])
        tasks = pool.build_tasks(args, pool.Plan(2, 3))
        self.assertEqual([task.argv for task in tasks],
                         [['build', '--dbm', 'bdb', '--jobs', '3', '--format', '{{.ID}}'],
                          ['build', '--dbm', 'kc', '--jobs', '3', '--format', '{{.ID}}']])
        self.assertEqual(tasks[1].log, Path('/logs/kc.log'))
        self.assertEqual(tasks[1].cleanup, ['docker', 'rm', '-f', 'name-kc'])
        self.assertEqual(tasks[1].remove, Path('/build/kc'))


class CliTests(unittest.TestCase):
    def run_cli(self, *args, **kwargs):
        return subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev.pool', *args],
                              cwd=ROOT, capture_output=True, text=True, timeout=60, **kwargs)

    def test_items_run_with_their_planned_jobs_and_log_files(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_cli('run', '--label', 'demo', '--items', 'a,b', '--admission', 'none',
                                  '--cpus', '4', '--log', f'{directory}/{{item}}.log',
                                  '--', sys.executable, '-c', 'import sys; print(sys.argv[1:])', '{item}', '{jobs}')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('demo: a b with', result.stderr)
            self.assertRegex((Path(directory) / 'a.log').read_text(), r"\['a', '\d+'\]")

    def test_a_failing_item_gives_status_one_and_its_log_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_cli('run', '--items', 'a', '--admission', 'none', '--log', f'{directory}/{{item}}.log',
                                  '--', sys.executable, '-c', 'print("boom"); raise SystemExit(5)')
            self.assertEqual(result.returncode, 1)
            self.assertIn('a: failed with status 5', result.stderr)
            self.assertIn('boom', result.stderr)

    def test_an_item_name_expands_in_every_word_of_the_command(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_cli('run', '--items', 'x', '--admission', 'none', '--log', f'{directory}/{{item}}.log',
                                  '--', sys.executable, '-c', 'import sys; assert sys.argv[1:] == ["x", "x/x"]',
                                  '{item}', '{item}/{item}')
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_sigterm_stops_queued_work_and_reaps_every_child(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            script = ('import os, sys, time; '
                      'open(sys.argv[1] + "/" + sys.argv[2], "w").write(str(os.getpid())); time.sleep(60)')
            process = subprocess.Popen(
                [sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev.pool', 'run', '--items', 'a,b,c,d',
                 '--admission', 'none', '--cpus', '4',
                 '--', sys.executable, '-c', script, str(directory), '{item}'],
                cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline and not list(directory.glob('?')):
                time.sleep(0.05)
            time.sleep(0.5)    # let any further worker the host's plan allows start
            started = sorted(path.name for path in directory.glob('?'))
            self.assertTrue(0 < len(started) < 4, started)    # at most two workers for four CPUs: items stay queued
            pids = [int(path.read_text()) for path in directory.glob('?')]
            process.send_signal(signal.SIGTERM)
            _, stderr = process.communicate(timeout=30)
            self.assertEqual(process.returncode, pool.CANCELLED_STATUS, stderr)
            self.assertEqual(sorted(path.name for path in directory.glob('?')), started)
            for pid in pids:
                self.assertFalse(running(pid), f'{pid} still running')

if __name__ == '__main__':
    unittest.main()
