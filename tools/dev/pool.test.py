#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for tools/dev/pool.py: the plan arithmetic, the pool loop with a
fake launcher, callable units under the same loop, and a real SIGTERM against the CLI. No platform is
skipped: /proc and CPU affinity are faked, and a unit's real children are plain Python."""
import argparse
import io
import json
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


CLEANED = []


def tasks_of(*names, cleanup=True, directory=None):
    return [pool.Task(name, ['run', name], log=None if directory is None else directory / f'{name}.log',
                      cleanup=(lambda name=name: CLEANED.append(['cleanup', name])) if cleanup else None)
            for name in names]


class PoolLoopTests(unittest.TestCase):
    def run_pool(self, tasks, workers, launcher, cancelled=lambda: False, out=None):
        CLEANED.clear()
        result = pool.run_pool(tasks, workers, launcher, cancelled=cancelled,
                               poll=0, grace=0, out=out or io.StringIO(), sleep=lambda _: None)
        return result, list(CLEANED)

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


class PlanTests(unittest.TestCase):
    def test_workers_times_jobs_never_exceed_the_cpus(self):
        for tasks in range(1, 7):
            for cpus in range(1, 33):
                with self.subTest(tasks=tasks, cpus=cpus):
                    chosen = pool.plan(tasks, cpus)
                    self.assertGreaterEqual(chosen.workers, 1)
                    self.assertGreaterEqual(chosen.jobs, 1)
                    self.assertLessEqual(chosen.workers, tasks)
                    self.assertLessEqual(chosen.workers * chosen.jobs, max(cpus, chosen.jobs))

    def test_overlapped_builds_keep_two_make_jobs(self):
        for tasks in range(2, 7):
            for cpus in range(1, 33):
                chosen = pool.plan(tasks, cpus)
                if chosen.workers > 1:
                    self.assertGreaterEqual(chosen.jobs, pool.MIN_OVERLAP_JOBS, (tasks, cpus))

    def test_plans(self):
        self.assertEqual(pool.plan(3, 12), (3, 4))
        self.assertEqual(pool.plan(3, 6), (3, 2))
        self.assertEqual(pool.plan(3, 4), (2, 2))
        self.assertEqual(pool.plan(3, 3), (1, 3))
        self.assertEqual(pool.plan(3, 2), (1, 2))
        self.assertEqual(pool.plan(3, 1), (1, 1))
        self.assertEqual(pool.plan(1, 8), (1, 8))


class CommandTemplateTests(unittest.TestCase):
    def test_placeholders_are_filled_per_item_and_other_braces_are_kept(self):
        args = argparse.Namespace(
            items=['bdb', 'kc'], log='/logs/{item}.log', container_runtime=None, container_label='l',
            remove='/build/{item}', command=['build', '--dbm', '{item}', '--jobs', '{jobs}', '--format', '{{.ID}}'])
        tasks = pool.build_tasks(args, pool.Plan(2, 3))
        self.assertEqual([task.argv for task in tasks],
                         [['build', '--dbm', 'bdb', '--jobs', '3', '--format', '{{.ID}}'],
                          ['build', '--dbm', 'kc', '--jobs', '3', '--format', '{{.ID}}']])
        self.assertEqual(tasks[1].log, Path('/logs/kc.log'))
        self.assertIsNone(tasks[1].cleanup)
        self.assertEqual(tasks[1].remove, Path('/build/kc'))


class CliTests(unittest.TestCase):
    def run_cli(self, *args, **kwargs):
        return subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev.pool', *args],
                              cwd=ROOT, capture_output=True, text=True, timeout=60, **kwargs)

    def test_items_run_with_their_planned_jobs_and_log_files(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_cli('run', '--label', 'demo', '--items', 'a,b',                                   '--cpus', '4', '--log', f'{directory}/{{item}}.log',
                                  '--', sys.executable, '-c', 'import sys; print(sys.argv[1:])', '{item}', '{jobs}')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('demo: a b with', result.stderr)
            self.assertRegex((Path(directory) / 'a.log').read_text(), r"\['a', '\d+'\]")

    def test_a_failing_item_gives_status_one_and_its_log_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_cli('run', '--items', 'a', '--log', f'{directory}/{{item}}.log',
                                  '--', sys.executable, '-c', 'print("boom"); raise SystemExit(5)')
            self.assertEqual(result.returncode, 1)
            self.assertIn('a: failed with status 5', result.stderr)
            self.assertIn('boom', result.stderr)

    def test_duplicate_items_are_rejected_before_anything_starts(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'ran'
            result = self.run_cli('run', '--items', 'a,b,a', '--',
                                  sys.executable, '-c', f'open({str(marker)!r}, "w")')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('distinct --items', result.stderr)
            self.assertFalse(marker.exists())

    def test_an_item_name_expands_in_every_word_of_the_command(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_cli('run', '--items', 'x', '--log', f'{directory}/{{item}}.log',
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
                 '--cpus', '4',
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

FAKE_RUNTIME = """
import json, os, secrets, subprocess, sys, time
state, args = sys.argv[1], sys.argv[2:]
# Invocations overlap: one `run` scans every record while another writes its own. A record is written
# under a temporary name beside it and renamed into place, as a real runtime does
# does (#1029), so a scan finds a whole record or none; it skips the temporaries. No record is read,
# changed and written back, so there is no lock.


def record(cid):
    return os.path.join(state, cid)


def containers():
    return {cid: json.load(open(record(cid))) for cid in sorted(os.listdir(state)) if not cid.endswith('.tmp')}


def write(target, text):
    temporary = f'{target}.{os.getpid()}.tmp'
    with open(temporary, 'w') as stream:
        hold()
        stream.write(text)
    os.replace(temporary, target)


def hold():
    # Test hook: with HOLD_RECORD set, stop once the new file exists and is empty, until release-record exists.
    if os.environ.get('HOLD_RECORD'):
        beside = os.path.dirname(state)
        open(os.path.join(beside, 'record-created'), 'w').close()
        while not os.path.exists(os.path.join(beside, 'release-record')):
            time.sleep(0.01)


if args[0] == 'run':
    options, rest, i = {}, [], 1
    while i < len(args):
        if args[i] in ('--name', '--label', '--cidfile'):
            options[args[i]] = args[i + 1]
            i += 2
        elif args[i] == '--':
            rest = args[i + 1:]
            break
        else:
            i += 1
    open(options['--cidfile'], 'w').close()
    if any(found['name'] == options['--name'] for found in containers().values()):
        print('name already in use', file=sys.stderr)
        sys.exit(125)
    cid = secrets.token_hex(32)
    key, _, value = options['--label'].partition('=')
    write(record(cid), json.dumps({'name': options['--name'], 'labels': {key: value}}))
    open(options['--cidfile'], 'w').write(cid)
    sys.exit(subprocess.call(rest))
elif args[:2] == ['container', 'inspect']:
    cid, template = args[-1], args[3]
    if not os.path.exists(record(cid)):
        sys.exit(1)
    key = template.split('"')[1]
    print(cid, containers()[cid]['labels'].get(key, ''))
elif args[:2] == ['container', 'rm']:
    os.remove(record(args[-1]))
"""


FAKE_RUNTIME = """
import json, os, secrets, subprocess, sys, time
state, args = sys.argv[1], sys.argv[2:]
# Invocations overlap: one `run` scans every record while another writes its own. A record is written
# under a temporary name beside it and renamed into place, as a real runtime does
# does (#1029), so a scan finds a whole record or none; it skips the temporaries. No record is read,
# changed and written back, so there is no lock.


def record(cid):
    return os.path.join(state, cid)


def containers():
    return {cid: json.load(open(record(cid))) for cid in sorted(os.listdir(state)) if not cid.endswith('.tmp')}


def write(target, text):
    temporary = f'{target}.{os.getpid()}.tmp'
    with open(temporary, 'w') as stream:
        hold()
        stream.write(text)
    os.replace(temporary, target)


def hold():
    # Test hook: with HOLD_RECORD set, stop once the new file exists and is empty, until release-record exists.
    if os.environ.get('HOLD_RECORD'):
        beside = os.path.dirname(state)
        open(os.path.join(beside, 'record-created'), 'w').close()
        while not os.path.exists(os.path.join(beside, 'release-record')):
            time.sleep(0.01)


if args[0] == 'run':
    options, rest, i = {}, [], 1
    while i < len(args):
        if args[i] in ('--name', '--label', '--cidfile'):
            options[args[i]] = args[i + 1]
            i += 2
        elif args[i] == '--':
            rest = args[i + 1:]
            break
        else:
            i += 1
    open(options['--cidfile'], 'w').close()
    if any(found['name'] == options['--name'] for found in containers().values()):
        print('name already in use', file=sys.stderr)
        sys.exit(125)
    cid = secrets.token_hex(32)
    key, _, value = options['--label'].partition('=')
    write(record(cid), json.dumps({'name': options['--name'], 'labels': {key: value}}))
    open(options['--cidfile'], 'w').write(cid)
    sys.exit(subprocess.call(rest))
elif args[:2] == ['container', 'inspect']:
    cid, template = args[-1], args[3]
    if not os.path.exists(record(cid)):
        sys.exit(1)
    key = template.split('"')[1]
    print(cid, containers()[cid]['labels'].get(key, ''))
elif args[:2] == ['container', 'rm']:
    os.remove(record(args[-1]))
"""


class ContainerOwnershipTests(unittest.TestCase):
    """The pool removes only containers it recorded and labelled, against a fake runtime that keeps state on disk."""

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.directory = Path(self.scratch.name)
        self.state = self.directory / 'state'
        self.state.mkdir()
        (self.directory / 'runtime.py').write_text(FAKE_RUNTIME)
        self.runtime = f'{sys.executable} {self.directory / "runtime.py"} {self.state}'
        self.bystander = 'b' * 64
        (self.state / self.bystander).write_text('{"name": "fixed-a", "labels": {"oxpinyin.test": "someone-else"}}')

    def left(self):
        """The container records; a runtime's unfinished temporary is not one."""
        return sorted(path.name for path in self.state.iterdir() if path.suffix != '.tmp')

    def command(self, name, script):
        return [*self.runtime.split(), 'run', '--name', name, '--label', 'oxpinyin.test={token}',
                '--cidfile', '{cidfile}', '--', sys.executable, '-c', script, '{item}']

    def arguments(self, items, name, script):
        return ['run', '--items', items, '--cpus', '4', '--container-runtime', self.runtime,
                '--container-label', 'oxpinyin.test', '--', *self.command(name, script)]

    def cli(self, *args):
        return [sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev.pool', *args]

    def test_a_failed_item_and_the_started_containers_are_removed_and_a_bystander_is_not(self):
        script = 'import sys, time; sys.exit(3) if sys.argv[1] == "a" else time.sleep(60)'
        result = subprocess.run(self.cli(*self.arguments('a,b', 'owned-{token}-{item}', script)), cwd=ROOT,
                                capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('a: failed with status 3', result.stderr)
        self.assertEqual(self.left(), [self.bystander])

    def test_a_name_collision_with_a_container_this_run_did_not_create_removes_nothing(self):
        script = 'import time; time.sleep(60)'
        result = subprocess.run(self.cli(*self.arguments('a', 'fixed-{item}', script)), cwd=ROOT,
                                capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('a: failed with status 125', result.stderr)
        self.assertEqual(self.left(), [self.bystander])

    def test_sigterm_removes_the_recorded_containers_and_leaves_the_bystander(self):
        script = 'import time; time.sleep(60)'
        process = subprocess.Popen(self.cli(*self.arguments('a,b,c,d', 'owned-{token}-{item}', script)), cwd=ROOT,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and len(self.left()) < 2:
            time.sleep(0.05)
        time.sleep(0.5)
        self.assertGreater(len(self.left()), 1, 'no container was created')
        process.send_signal(signal.SIGTERM)
        _, stderr = process.communicate(timeout=30)
        self.assertEqual(process.returncode, pool.CANCELLED_STATUS, stderr)
        self.assertEqual(self.left(), [self.bystander])

    def test_a_run_scanning_while_another_writes_its_record_never_reads_it_half_written(self):
        # The pool's workers start their `run` commands together, so one scans the records while another
        # writes its own. The first run here stops (HOLD_RECORD) once its record's file exists and is empty;
        # the second then scans and runs to its end; only then is the first released. With a plain
        # `json.dump(..., open(record, 'w'))` the second reads the empty file and exits 1 on a JSONDecodeError.
        def run(name, **environment):
            return subprocess.Popen(
                [*self.runtime.split(), 'run', '--name', name, '--label', 'oxpinyin.test=t',
                 '--cidfile', str(self.directory / f'{name}.cid'), '--', sys.executable, '-c', 'pass'],
                env=dict(os.environ, **environment), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        created, release = self.directory / 'record-created', self.directory / 'release-record'
        first = run('first', HOLD_RECORD='1')
        self.addCleanup(first.kill)
        reached, second, second_stderr = False, None, ''
        try:
            deadline = time.monotonic() + 30
            while not created.exists() and first.poll() is None and time.monotonic() < deadline:
                time.sleep(0.01)
            reached = created.exists()
            if reached:
                second = run('second')
                second_stderr = second.communicate(timeout=30)[1]
        finally:
            release.touch()
            first_stderr = first.communicate(timeout=30)[1]
        self.assertTrue(reached, f'the first run never reached its record: {first_stderr}')
        self.assertEqual(second.returncode, 0, second_stderr)
        self.assertEqual(first.returncode, 0, first_stderr)
        names = sorted(json.loads((self.state / record).read_text())['name']
                       for record in self.left() if record != self.bystander)
        self.assertEqual(names, ['first', 'second'])
        self.assertEqual(sorted(path.name for path in self.state.glob('*.tmp')), [])

    def owned(self, cid, token='mine'):
        cidfile = self.directory / 'cid'
        cidfile.write_text(cid)
        return pool.OwnedContainer(('rt',), cidfile, 'k', token)

    def runner(self, inspected):
        calls = []

        def run(argv):
            calls.append(list(argv))
            if argv[2] == 'inspect':
                return subprocess.CompletedProcess(argv, 0 if inspected is not None else 1, inspected, '')
            return subprocess.CompletedProcess(argv, 0, '', '')
        return run, calls

    def test_remove_owned_needs_the_recorded_id_and_this_runs_token(self):
        cid = 'a' * 64
        for inspected, removed in ((f'{cid} mine\n', True), (f'{cid} other\n', False), (f'{cid} \n', False),
                                   (f'{"c" * 64} mine\n', False), (None, False)):
            with self.subTest(inspected=inspected):
                run, calls = self.runner(inspected)
                self.assertEqual(pool.remove_owned(self.owned(cid), run, io.StringIO()), removed)
                self.assertEqual([call[2] for call in calls], ['inspect', 'rm'] if removed else ['inspect'])
                if removed:
                    self.assertEqual(calls[1], ['rt', 'container', 'rm', '-f', cid])

    def test_wslc_and_apple_owners_are_read_from_their_json(self):
        from tools.oracle import image
        patcher = mock.patch.object(image, 'host_system', return_value='Darwin')
        patcher.start()
        self.addCleanup(patcher.stop)
        cid = 'b' * 64
        records = {'wslc': [{'Id': cid, 'Config': {'Labels': {'k': 'mine'}}}],
                   'container': [{'configuration': {'id': cid, 'labels': {'k': 'mine'}}}]}
        removals = {'wslc': ['wslc', 'container', 'remove', '-f', cid], 'container': ['container', 'delete', '-f', cid]}
        inspects = {'wslc': ['wslc', 'container', 'inspect', cid], 'container': ['container', 'inspect', cid]}
        for runtime, record in records.items():
            with self.subTest(runtime=runtime):
                calls = []

                def run(argv):
                    calls.append(list(argv))
                    found = 'inspect' in argv
                    return subprocess.CompletedProcess(argv, 0, json.dumps(record) if found else '', '')
                owned = self.owned(cid)
                owned = pool.OwnedContainer((runtime,), owned.cidfile, 'k', 'mine')
                self.assertTrue(pool.remove_owned(owned, run, io.StringIO()))
                self.assertEqual(calls, [inspects[runtime], removals[runtime]])

    def test_windows_tasks_are_stopped_without_process_groups(self):
        process = mock.Mock()
        handle = pool.Handle(process)
        with mock.patch.object(pool.sys, 'platform', 'win32'), mock.patch.object(pool.os, 'killpg', create=True) as killpg:
            handle.terminate()
            handle.kill()
        process.terminate.assert_called_once_with()
        process.kill.assert_called_once_with()
        killpg.assert_not_called()

    def test_an_empty_or_missing_cidfile_means_no_container_and_no_runtime_call(self):
        run, calls = self.runner(f'{"a" * 64} mine\n')
        owned = self.owned('')
        self.assertFalse(pool.remove_owned(owned, run, io.StringIO()))
        owned.cidfile.unlink()
        self.assertFalse(pool.remove_owned(owned, run, io.StringIO()))
        self.assertEqual(calls, [])


class UnitRunnerTests(unittest.TestCase):
    """Callable units under the pool loop; the children are real."""
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='pool-units-')
        self.work = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        self.lock, self.events = threading.Lock(), []

    def log(self, event):
        with self.lock:
            self.events.append(event)

    def run_units(self, units, workers=2, cancelled=lambda: False, **kwargs):
        return pool.run_units(units, workers, cancelled=cancelled, poll=0.01, grace=3, out=io.StringIO(), **kwargs)

    def hold(self, name):
        """A unit that launches a real child (and a grandchild) and holds until the pool stops it."""
        def run(context):
            script = ('import os, subprocess, sys, time\n'
                      'g = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])\n'
                      f'open({str(self.work / (name + ".pids"))!r}, "w").write(f"{{os.getpid()}} {{g.pid}}")\n'
                      'time.sleep(60)')
            try:
                context.launch([sys.executable, '-c', script])
            finally:
                self.log('reaped-' + name)
        return name, run

    def pids(self, name):
        deadline = time.monotonic() + 10
        path = self.work / (name + '.pids')
        while time.monotonic() < deadline:
            if path.exists() and path.read_text().count(' '):
                return [int(value) for value in path.read_text().split()]
            time.sleep(0.01)
        self.fail('the child never started: ' + name)

    def test_no_more_than_the_workers_run_at_once_and_values_come_back_in_order(self):
        for workers in (1, 2, 3):
            with self.subTest(workers=workers):
                active, peak = [0], [0]

                def unit(name):
                    def run(context):
                        with self.lock:
                            active[0] += 1
                            peak[0] = max(peak[0], active[0])
                        time.sleep(0.03)
                        with self.lock:
                            active[0] -= 1
                        return name.upper()
                    return name, run
                names = ['a', 'b', 'c', 'd', 'e', 'f']
                self.assertEqual(self.run_units([unit(name) for name in names], workers), [n.upper() for n in names])
                self.assertEqual(peak[0], workers)

    def test_emission_follows_unit_order_in_the_calling_thread_whatever_the_completion_order(self):
        caller, emitted = threading.get_ident(), []
        finished = [threading.Event() for _ in range(4)]

        def unit(number):
            def run(context):
                # Unit n ends only after unit n+1 has: completion is the reverse of unit order.
                if number < 3:
                    finished[number + 1].wait(5)
                finished[number].set()
                return number
            return 'u%d' % number, run
        values = self.run_units([unit(number) for number in range(4)], 4,
                                emit=lambda name, value: emitted.append((name, value, threading.get_ident() == caller)))
        self.assertEqual(values, [0, 1, 2, 3])
        self.assertEqual(emitted, [('u%d' % number, number, True) for number in range(4)])

    def failing_after(self, name, hold):
        """A unit that fails once the held unit's child is running."""
        def run(context):
            self.pids(hold)
            raise ValueError('populate failed')
        return name, run

    def test_a_failure_starts_no_queued_unit_stops_the_running_and_reaps_their_children_first(self):
        started = []
        units = [self.hold('hold'), self.failing_after('bad', 'hold'),
                 *[(name, lambda c, name=name: started.append(name)) for name in ('q1', 'q2', 'q3')]]
        with self.assertRaisesRegex(ValueError, 'populate failed'):
            self.run_units(units, 2)
        self.assertEqual(started, [])
        self.assertIn('reaped-hold', self.events)
        self.assert_reaped_and_stopped(self.pids('hold'))

    def test_units_finished_before_a_failure_are_still_emitted_in_order(self):
        emitted = []
        units = [('good', lambda context: 'ok'), self.hold('hold'), self.failing_after('bad', 'hold')]
        with self.assertRaisesRegex(ValueError, 'populate failed'):
            self.run_units(units, 3, emit=lambda name, value: emitted.append((name, value)))
        self.assertEqual(emitted, [('good', 'ok')])

    def test_the_earliest_units_own_error_is_raised_whichever_failed_first(self):
        later_failed = threading.Event()

        def first(context):
            later_failed.wait(5)
            raise ValueError('first')

        def second(context):
            later_failed.set()
            raise KeyError('second')
        with self.assertRaisesRegex(ValueError, 'first'):
            self.run_units([('first', first), ('second', second)], 2)

    def test_cancellation_stops_queued_units_and_reaps_children_before_returning(self):
        class Cancelled(Exception):
            pass
        flag, started = threading.Event(), []
        units = [self.hold('a'), self.hold('b'),
                 *[(name, lambda c, name=name: started.append(name)) for name in ('q1', 'q2')]]
        threading.Thread(target=lambda: (self.pids('a'), self.pids('b'), flag.set())).start()
        with self.assertRaises(Cancelled):
            self.run_units(units, 2, cancelled=flag.is_set, cancel_error=Cancelled)
        self.assertEqual(started, [])
        self.assertEqual(sorted(event for event in self.events if str(event).startswith('reaped')),
                         ['reaped-a', 'reaped-b'])
        for name in ('a', 'b'):
            self.assert_reaped_and_stopped(self.pids(name))

    def assert_reaped_and_stopped(self, pids):
        """The direct child is reaped synchronously; descendants receive SIGKILL asynchronously."""
        child, *descendants = pids
        self.assertFalse(running(child))
        deadline = time.monotonic() + 5
        while any(running(pid) for pid in descendants) and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertFalse(any(running(pid) for pid in descendants))

    def test_cancellation_and_failure_kill_descendants_after_the_leader_exits(self):
        for reason in ('cancelled', 'sibling failed'):
            with self.subTest(reason=reason):
                ready = self.work / (reason + '.pids')
                # The intermediate parent stays outside the target group and reaps
                # the stubborn descendant, so even a container without a reaping
                # PID 1 can observe the target group's disappearance with killpg.
                child = ('import os, signal, sys, time\n'
                         'os.setpgid(0, int(sys.argv[1]))\n'
                         'signal.signal(signal.SIGTERM, signal.SIG_IGN)\n'
                         f'open({str(ready)!r}, "w").write(f"{{os.getpgrp()}} {{os.getppid()}}")\n'
                         'time.sleep(60)\n')
                reaper = ('import os, subprocess, sys\n'
                          'os.setpgrp()\n'
                          f'subprocess.Popen([sys.executable, "-c", {child!r}, sys.argv[1]]).wait()\n')
                leader = ('import os, signal, subprocess, sys, time\n'
                          'signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))\n'
                          f'subprocess.Popen([sys.executable, "-c", {reaper!r}, str(os.getpid())])\n'
                          'time.sleep(60)\n')
                flag, groups = threading.Event(), []

                def hold(context):
                    return context.launch([sys.executable, '-c', leader], poll=.01)

                def stop(context):
                    deadline = time.monotonic() + 10
                    while time.monotonic() < deadline:
                        if ready.exists() and ready.read_text().count(' '):
                            groups.extend(map(int, ready.read_text().split()))
                            break
                        context.check()
                        time.sleep(.01)
                    else:
                        raise AssertionError('descendant did not become ready')
                    os.killpg(groups[0], 0)
                    if reason == 'cancelled':
                        flag.set()
                    else:
                        raise ValueError(reason)

                try:
                    error = pool.UnitsCancelled if reason == 'cancelled' else ValueError
                    with self.assertRaisesRegex(error, reason):
                        self.run_units([('leader', hold), ('stop', stop)], cancelled=flag.is_set)
                    deadline = time.monotonic() + 10
                    while time.monotonic() < deadline:
                        try:
                            os.killpg(groups[0], 0)
                        except ProcessLookupError:
                            break
                        except PermissionError:
                            # A transient permission error is not proof that the
                            # group is gone; keep waiting for ProcessLookupError.
                            pass
                        time.sleep(.01)
                    with self.assertRaises(ProcessLookupError):
                        os.killpg(groups[0], 0)
                finally:
                    for group in groups:
                        try:
                            os.killpg(group, signal.SIGKILL)
                        except (ProcessLookupError, PermissionError):
                            pass

    def test_a_serial_run_sees_cancellation_at_its_checks_and_reaps_its_child(self):
        class Cancelled(Exception):
            pass
        flag, observed = threading.Event(), []
        def cancel_after_ready():
            try:
                observed.extend(self.pids('a'))
            finally:
                flag.set()
        stopper = threading.Thread(target=cancel_after_ready)
        stopper.start()
        units = [self.hold('a'), ('b', lambda context: self.log('b'))]
        try:
            with self.assertRaises(Cancelled):
                pool.run_leased(units, cancelled=flag.is_set, serial=True, cancel_error=Cancelled,
                                out=io.StringIO())
        finally:
            stopper.join(timeout=12)
        self.assertFalse(stopper.is_alive())
        self.assertEqual(self.events, ['reaped-a'])
        self.assertEqual(len(observed), 2)
        self.assert_reaped_and_stopped(observed)

    def test_a_pooled_run_cancelled_at_a_unit_check_raises_the_cancel_error(self):
        class Cancelled(Exception):
            pass
        flag = threading.Event()
        threading.Timer(0.2, flag.set).start()
        with self.assertRaises(Cancelled):
            pool.run_leased([self.hold('a'), self.hold('b')], cancelled=flag.is_set,
                            cancel_error=Cancelled, out=io.StringIO())
        self.assertEqual(sorted(self.events), ['reaped-a', 'reaped-b'])

    def test_the_stop_reaches_a_unit_only_at_its_checks(self):
        context = pool.UnitContext('u', threading.Event())
        context.check()
        context.stop.set()
        with self.assertRaises(pool.UnitStopped):
            context.check()

    def test_a_driver_view_adds_the_stop_to_the_drivers_own_check(self):
        driver = mock.Mock(check_cancelled=mock.Mock(), other='kept')
        context = pool.UnitContext('u', threading.Event())
        view = context.driver(driver)
        view.check_cancelled()
        driver.check_cancelled.assert_called_once_with()
        self.assertEqual(view.other, 'kept')
        driver.check_cancelled.side_effect = RuntimeError('driver cancelled')
        with self.assertRaisesRegex(RuntimeError, 'driver cancelled'):
            view.check_cancelled()
        driver.check_cancelled.side_effect = None
        context.stop.set()
        with self.assertRaises(pool.UnitStopped):
            view.check_cancelled()

    def test_launch_returns_status_and_output_with_the_units_environment(self):
        context = pool.UnitContext('u', threading.Event(), environment={'UNIT_PROBE': 'unit'})
        script = ('import os, sys\nprint(os.environ["UNIT_PROBE"], flush=True)\n'
                  'print("to stderr", file=sys.stderr)\nsys.exit(3)')
        done = context.launch([sys.executable, '-c', script])
        self.assertEqual((done.status, done.stdout, done.stderr), (3, b'unit\n', b'to stderr\n'))

    def test_launch_closes_stdin_and_honours_an_explicit_environment(self):
        context = pool.UnitContext('u', threading.Event(), environment={'UNIT_PROBE': 'unit'})
        done = context.launch([sys.executable, '-c', 'import os, sys; print(repr(sys.stdin.read()), os.environ.get("UNIT_PROBE"))'],
                              env={'ONLY': '1'})
        self.assertEqual(done.stdout, b"'' None\n")

    def test_launch_delivers_binary_input_across_communicate_timeouts(self):
        context = pool.UnitContext('u', threading.Event())
        data = bytes(range(256)) * 8192
        done = context.launch([sys.executable, '-c',
            'import sys,time; time.sleep(.1); sys.stdout.buffer.write(sys.stdin.buffer.read())'],
            input=data, poll=.01)
        self.assertEqual(done.status, 0)
        self.assertEqual(done.stdout, data)

    def test_each_running_unit_leases_its_own_cpu_and_returns_it(self):
        seen, together = [], threading.Barrier(3, timeout=5)

        def unit(name):
            def run(context):
                together.wait()
                seen.append(context.cpu)
            return name, run
        launcher = pool.ProcessLauncher(cpus=[5, 6, 7])
        self.run_units([unit(name) for name in 'abc'], 3, launcher=launcher)
        self.assertEqual(sorted(seen), [5, 6, 7])
        self.assertEqual(sorted(launcher.leases.get_nowait() for _ in range(3)), [5, 6, 7])

    def test_a_unit_without_a_lease_has_no_cpu(self):
        seen = []
        self.run_units([('a', lambda context: seen.append(context.cpu))], 1)
        self.assertEqual(seen, [None])

    def test_units_and_launched_processes_share_one_pool(self):
        path = self.work / 'process-ran'
        tasks = [pool.Task('process', [sys.executable, '-c', f'open({str(path)!r}, "w").write("x")']),
                 pool.Task('unit', [], run=lambda context: self.log('unit'))]
        launcher = pool.ProcessLauncher()
        result = pool.run_pool(tasks, 2, launcher, cancelled=lambda: False, poll=0.01, out=io.StringIO())
        self.assertTrue(result.ok)
        self.assertEqual(result.started, ['process', 'unit'])
        self.assertTrue(path.exists())
        self.assertEqual(self.events, ['unit'])

    def test_duplicate_unit_names_are_refused_before_anything_starts(self):
        started = []
        with self.assertRaisesRegex(ValueError, 'distinct'):
            self.run_units([('a', lambda c: started.append(1)), ('a', lambda c: started.append(2))])
        self.assertEqual(started, [])


class RunLeasedTests(unittest.TestCase):
    """run_leased: units in a pool sized by the unit count, the CPU list and the worker limit."""

    def setUp(self):
        cpus = mock.patch.object(host, 'allowed_cpus', return_value={2, 5, 7, 9})
        cpus.start()
        self.addCleanup(cpus.stop)

    def run_leased(self, units, **kwargs):
        kwargs.setdefault('cancelled', lambda: False)
        return pool.run_leased(units, out=io.StringIO(), **kwargs)

    def peak_of(self, expected, **kwargs):
        """Run four units that overlap up to EXPECTED at once; return the peak concurrency."""
        active, peak = 0, 0
        lock, together = threading.Lock(), threading.Event()

        def run(context):
            nonlocal active, peak
            with lock:
                active += 1
                peak = max(peak, active)
                if active == expected:
                    together.set()
            self.assertTrue(together.wait(5), 'the workers did not overlap')
            time.sleep(.02)
            with lock:
                active -= 1
            return context.name
        values = self.run_leased([(name, run) for name in 'abcd'], **kwargs)
        self.assertEqual(values, list('abcd'))
        return peak

    def test_the_pool_is_sized_by_the_unit_count_and_the_cpu_list(self):
        for cpus, expected in (([0, 1, 2, 3], 4), ([0, 1], 2), (None, 4), ([6], 1)):
            with self.subTest(cpus=cpus):
                self.assertEqual(self.peak_of(expected, cpus=cpus), expected)

    def test_fewer_units_than_cpus_run_one_each(self):
        self.assertEqual(self.peak_of(4, cpus=list(range(16))), 4)

    def test_worker_limit_caps_concurrency(self):
        for cpus, limit, expected in ((None, 2, 2), (None, 1, 1), ([5], 2, 1)):
            with self.subTest(cpus=cpus, limit=limit):
                self.assertEqual(self.peak_of(expected, cpus=cpus, max_workers=limit), expected)

    def test_serial_runs_one_unit_at_a_time_in_order_and_stops_at_the_first_failure(self):
        ran = []
        units = [(name, lambda c, name=name: ran.append(name) or name) for name in 'abc']
        self.assertEqual(self.run_leased(units, serial=True), list('abc'))
        self.assertEqual(ran, list('abc'))
        ran.clear()

        def bad(context):
            raise ValueError('first')
        with self.assertRaisesRegex(ValueError, 'first'):
            self.run_leased([('a', lambda c: ran.append('a')), ('b', bad), ('c', lambda c: ran.append('c'))],
                            serial=True)
        self.assertEqual(ran, ['a'])

    def test_nonpositive_worker_limit_and_empty_cpus_refuse_before_any_unit_starts(self):
        units = [('a', lambda c: self.fail('unit must not run')), ('b', lambda c: None)]
        for cap in (0, -1):
            with self.subTest(cap=cap), self.assertRaisesRegex(ValueError, 'max_workers must be positive'):
                self.run_leased(units, max_workers=cap)
        with self.assertRaisesRegex(ValueError, 'cpus must not be empty'):
            self.run_leased(units, cpus=[])

    def test_empty_work_returns_nothing(self):
        self.assertEqual(self.run_leased([]), [])

    def test_each_running_unit_leases_a_distinct_cpu_from_the_list(self):
        seen, together = [], threading.Barrier(3, timeout=5)

        def run(context):
            together.wait()
            seen.append(context.cpu)
        self.run_leased([(name, run) for name in 'abc'], cpus=[5, 6, 7])
        self.assertEqual(sorted(seen), [5, 6, 7])

    def test_the_default_cpu_list_is_the_allowed_set(self):
        seen = []
        self.run_leased([(name, lambda c: seen.append(c.cpu)) for name in 'abcdef'], serial=False)
        self.assertTrue(set(seen) <= {2, 5, 7, 9})

    def test_units_see_the_given_environment_or_the_process_environment(self):
        seen = []
        self.run_leased([('a', lambda c: seen.append(c.environment.get('LEASED_PROBE')))], env={'LEASED_PROBE': 'given'})
        with mock.patch.dict(os.environ, {'LEASED_PROBE': 'inherited'}):
            self.run_leased([('a', lambda c: seen.append(c.environment.get('LEASED_PROBE')))])
        self.assertEqual(seen, ['given', 'inherited'])

    def test_values_come_back_in_unit_order_and_emission_runs_in_the_calling_thread(self):
        caller, emitted = threading.get_ident(), []

        def slow(context):
            time.sleep(.1)
            return 'slow'
        values = self.run_leased([('a', slow), ('b', lambda c: 'fast')],
                                 emit=lambda name, value: emitted.append((name, value, threading.get_ident() == caller)))
        self.assertEqual(values, ['slow', 'fast'])
        self.assertEqual(emitted, [('a', 'slow', True), ('b', 'fast', True)])

    def test_cancellation_raises_the_callers_error(self):
        class Stop(Exception):
            pass
        flag = threading.Event()

        def run(context):
            flag.set()
            while True:
                context.check()
                time.sleep(.01)
        with self.assertRaises(Stop):
            self.run_leased([('a', run)], cancelled=flag.is_set, cancel_error=lambda message: Stop(message))


if __name__ == '__main__':
    unittest.main()
