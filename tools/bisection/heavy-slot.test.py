#!/usr/bin/env python3
"""Regressions for run-all.py's host-wide heavy slot."""
import io
import fcntl
import errno
import select
import signal
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
RUN_ALL = HERE / 'run-all.py'
run_all = runpy.run_path(str(RUN_ALL))


class HeavySlotTests(unittest.TestCase):
    def setUp(self):
        self.environment = mock.patch.dict(os.environ)
        self.environment.start()
        os.environ.pop('OXPINYIN_HEAVY_SLOT_HELD', None)
        os.environ.pop('OXPINYIN_HEAVY_SLOTS', None)
        self.lock_dir = tempfile.TemporaryDirectory()
        self.env = dict(os.environ, OXPINYIN_LOCK_DIR=self.lock_dir.name)
        os.environ['OXPINYIN_LOCK_DIR'] = self.lock_dir.name
        os.environ.pop('OXPINYIN_HEAVY_SLOTS', None)

    def tearDown(self):
        os.environ.pop('OXPINYIN_LOCK_DIR', None)
        self.lock_dir.cleanup()
        self.environment.stop()

    def heavy(self, seconds):
        return subprocess.Popen([sys.executable, str(RUN_ALL), '--heavy-slot', '--', sys.executable, '-c',
                                 f'import time; time.sleep({seconds})'], env=self.env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def test_two_concurrent_heavy_requests_serialise_and_report_the_wait(self):
        started = time.monotonic()
        first = self.heavy(1.2)
        time.sleep(0.3)
        second = self.heavy(0.1)
        outputs = [p.communicate()[1] for p in (first, second)]
        elapsed = time.monotonic() - started
        self.assertEqual([first.returncode, second.returncode], [0, 0])
        self.assertGreaterEqual(elapsed, 1.3)
        waits = [float(line.split('slot_wait_seconds ')[1].split(',')[0]) for out in outputs
                 for line in out.splitlines() if 'heavy slot 0: slot_wait_seconds' in line]
        self.assertEqual(len(waits), 2)
        self.assertLess(min(waits), 0.5)
        self.assertGreaterEqual(max(waits), 0.8)
        waiting = [line for out in outputs for line in out.splitlines() if line.startswith('waiting for a heavy slot')]
        self.assertEqual(len(waiting), 1)
        self.assertIn('pid ', waiting[0])
        self.assertIn('time.sleep(1.2)', waiting[0])

    def test_nested_wrapper_inherits_the_slot_without_waiting(self):
        code = (f"import runpy, sys; m = runpy.run_path({str(RUN_ALL)!r}); "
                "sys.exit(m['run_in_heavy_slot']([sys.executable, '-c', 'pass']))")
        result = subprocess.run([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                 sys.executable, '-c', code], env=self.env,
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr.count('inherited from ancestor'), 1)
        self.assertIn('heavy slot 0 inherited', result.stderr)
        self.assertEqual(result.stderr.count('slot_wait_seconds 0.0'), 2)
        self.assertNotIn('waiting for a heavy slot', result.stderr)

    def test_relative_lock_directory_stops_before_starting_a_child(self):
        result = subprocess.run([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                 sys.executable, '-c', 'print("started")'],
                                env=dict(self.env, OXPINYIN_LOCK_DIR='relative'),
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 1)
        self.assertIn('OXPINYIN_LOCK_DIR must be an absolute path', result.stderr)
        self.assertNotIn('started', result.stdout)

    def test_nested_call_in_another_directory_inherits_resolved_default(self):
        with tempfile.TemporaryDirectory() as initial, tempfile.TemporaryDirectory() as other:
            expected = str(Path(initial) / 'cache/oxpinyin/locks')
            code = ("import json, os, runpy, sys; "
                    f"assert os.environ['OXPINYIN_LOCK_DIR'] == {expected!r}; "
                    f"os.chdir({other!r}); m = runpy.run_path({str(RUN_ALL)!r}); "
                    "sys.exit(m['run_in_heavy_slot']([sys.executable, '-c', 'pass']))")
            env = dict(self.env, XDG_CACHE_HOME='cache')
            env.pop('OXPINYIN_LOCK_DIR')
            result = subprocess.run([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                     sys.executable, '-c', code], cwd=initial, env=env,
                                    capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('inherited from ancestor', result.stderr)
        self.assertNotIn('waiting for a heavy slot', result.stderr)

    def test_missing_fcntl_names_the_portable_slot_stage(self):
        code = ("import runpy, sys; sys.modules['fcntl'] = None; "
                f"m = runpy.run_path({str(RUN_ALL)!r}); "
                "m['main']()")
        result = subprocess.run([sys.executable, '-c', code], env=self.env,
                                capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Windows support requires #895 Stage 2 (`just heavy`)', result.stderr)
        self.assertNotIn('ImportError', result.stderr)

    def test_cancellation_reaps_heavy_child_before_releasing_slot_and_checkout_lock(self):
        aggregate = r'''import json, runpy, sys
from pathlib import Path
from unittest import mock
script, command, resource = sys.argv[1:]
m = runpy.run_path(script)
original = Path.read_text
row = next(r for r in json.loads(original(Path(script).with_name('runners.json'))) if r['name']=='pc-version')
row = dict(row, name='tiny-heavy', heavy=True, command=[sys.executable, '-c', command],
           exclusive_resources=[resource])
def read(path, *args, **kwargs):
    return json.dumps([row]) if path.name=='runners.json' else original(path, *args, **kwargs)
with mock.patch.object(Path, 'read_text', read), mock.patch.object(sys, 'argv',
        [script, '--runners', 'tiny-heavy', '--no-build', '--cells', 'bdb']):
    sys.exit(m['main']())
'''
        for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            with self.subTest(signal=signum), tempfile.TemporaryDirectory() as directory:
                ready = Path(directory) / 'ready'
                stopping = Path(directory) / 'stopping'
                resource = f'cancellation-{os.getpid()}-{signum}'
                command = ("import os, signal, sys, time; from pathlib import Path\n"
                           "def stop(*args):\n"
                           f" Path({str(stopping)!r}).touch()\n"
                           " time.sleep(0.7)\n sys.exit(0)\n"
                           "signal.signal(signal.SIGTERM, stop)\n"
                           "signal.signal(signal.SIGINT, stop)\n"
                           "signal.signal(signal.SIGHUP, stop)\n"
                           f"Path({str(ready)!r}).write_text(str(os.getpid()))\n"
                           "time.sleep(30)\n")
                parent = subprocess.Popen([sys.executable, '-c', aggregate, str(RUN_ALL), command, resource],
                                          env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                contender = None
                try:
                    deadline = time.monotonic() + 5
                    while (not ready.exists() or not ready.read_text()) and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(ready.exists())
                    child_pid = int(ready.read_text())
                    check = (f'import os; pid={child_pid}\n'
                             'try: os.kill(pid, 0)\n'
                             'except ProcessLookupError: print("reaped", flush=True)\n'
                             'else: raise AssertionError("heavy child is still alive")')
                    contender = subprocess.Popen([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                                  sys.executable, '-c', check], env=self.env,
                                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                    parent.send_signal(signum)
                    deadline = time.monotonic() + 3
                    while not stopping.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(stopping.exists())
                    lock_path = HERE.parent.parent / 'target/runner-locks' / resource
                    with lock_path.open('a') as probe:
                        with self.assertRaises(BlockingIOError):
                            fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    with self.assertRaises(subprocess.TimeoutExpired):
                        contender.communicate(timeout=0.2)
                    _, error = parent.communicate(timeout=8)
                    self.assertNotEqual(parent.returncode, 0, error)
                    self.assertFalse(run_all['pid_alive'](child_pid))
                    output, error = contender.communicate(timeout=5)
                    self.assertEqual(contender.returncode, 0, error)
                    self.assertIn('reaped', output)
                    with lock_path.open('a') as probe:
                        fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
                finally:
                    for process in (parent, contender):
                        if process is not None:
                            if process.poll() is None:
                                process.kill()
                            process.communicate(timeout=8)
                    lock_path = HERE.parent.parent / 'target/runner-locks' / resource
                    lock_path.unlink(missing_ok=True)

    def test_marker_for_another_directory_does_not_bypass_the_slot(self):
        with tempfile.TemporaryDirectory() as other:
            env = dict(self.env, OXPINYIN_HEAVY_SLOT_HELD=json.dumps(
                dict(lock_dir=other, slot=0, pid=os.getpid())))
            with run_all['HeavySlot']('test holder', out=io.StringIO()):
                child = subprocess.Popen([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                          sys.executable, '-c', 'pass'], env=env,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                try:
                    with self.assertRaises(subprocess.TimeoutExpired):
                        child.communicate(timeout=0.8)
                finally:
                    if child.poll() is not None:
                        self.fail('The child bypassed the held slot.')
            try:
                _, stderr = child.communicate(timeout=5)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.communicate()
            self.assertEqual(child.returncode, 0, stderr)
            self.assertIn('waiting for a heavy slot', stderr)
            self.assertNotIn('inherited', stderr)

    def test_invalid_marker_fails_with_a_clear_message(self):
        for marker in ('bad JSON', '{}', '[]', '{"lock_dir":"relative","slot":0}',
                       '{"lock_dir":"/tmp","slot":-1}',
                       '{"lock_dir":"/tmp","slot":true}'):
            with self.subTest(marker=marker):
                result = subprocess.run([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                         sys.executable, '-c', 'pass'],
                                        env=dict(self.env, OXPINYIN_HEAVY_SLOT_HELD=marker),
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 1)
                self.assertIn('FAIL: Invalid OXPINYIN_HEAVY_SLOT_HELD', result.stderr)

    def test_parallel_heavy_jobs_keep_their_markers_private(self):
        code = r'''import fcntl, json, os, runpy, subprocess, sys, time
from pathlib import Path
from unittest import mock
script = sys.argv[1]
m = runpy.run_path(script)
original_read = Path.read_text
row = next(r for r in json.loads(original_read(Path(sys.argv[1]).with_name('runners.json')))
           if r['name'] == 'pc-version')
rows = [dict(row, name=name, heavy=True) for name in ('heavy-a', 'heavy-b')]
seen = []
def read(path, *args, **kwargs):
    if path.name == 'runners.json':
        return json.dumps(rows)
    return original_read(path, *args, **kwargs)
def execute(command, env, cwd, timeout, output, slot=None):
    assert 'OXPINYIN_HEAVY_SLOT_HELD' not in os.environ
    marker = json.loads(env['OXPINYIN_HEAVY_SLOT_HELD'])
    assert marker['lock_dir'] == str(Path(os.environ['OXPINYIN_LOCK_DIR']).resolve())
    assert marker['slot'] == 0 and marker['pid'] == os.getpid()
    assert marker['fd'] in slot.pass_fds()
    with (Path(marker['lock_dir']) / 'heavy-slot-0').open('a+') as probe:
        try:
            fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            pass
        else:
            raise AssertionError('The runner did not hold its own slot.')
    start = time.monotonic()
    nested = subprocess.run([sys.executable, script, '--heavy-slot', '--',
                             sys.executable, '-c', 'pass'], env=env,
                            capture_output=True, text=True, timeout=3)
    assert nested.returncode == 0, nested.stderr
    assert 'inherited from ancestor' in nested.stderr
    assert 'slot_wait_seconds 0.0' in nested.stderr
    time.sleep(0.7)
    seen.append((start, time.monotonic()))
    return 0, 0.7, '', False
with mock.patch.object(Path, 'read_text', read), mock.patch.dict(m['main'].__globals__, execute=execute):
    with mock.patch.object(sys, 'argv', [sys.argv[1], '--runners', 'heavy-a,heavy-b',
                                       '--cells', 'bdb', '--no-build', '--jobs', '2']):
        assert m['main']() == 0
assert len(seen) == 2
seen.sort()
assert seen[1][0] >= seen[0][1]
assert 'OXPINYIN_HEAVY_SLOT_HELD' not in os.environ
'''
        result = subprocess.run([sys.executable, '-c', code, str(RUN_ALL)], env=self.env,
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('inherited from ancestor', result.stderr)

    def test_light_row_finishes_before_two_heavy_rows_waiting_for_a_slot(self):
        code = r'''import io, json, runpy, sys
from pathlib import Path
from unittest import mock
script = sys.argv[1]
m = runpy.run_path(script)
original_read = Path.read_text
row = next(r for r in json.loads(original_read(Path(script).with_name('runners.json')))
           if r['name'] == 'pc-version')
rows = [dict(row, name=name, heavy=heavy, command=[name])
        for name, heavy in [('heavy-a', True), ('heavy-b', True), ('light', False)]]
finished = []
holder = m['HeavySlot']('external holder', out=io.StringIO())
holder.__enter__()
def read(path, *args, **kwargs):
    return json.dumps(rows) if path.name == 'runners.json' else original_read(path, *args, **kwargs)
def execute(command, env, cwd, timeout, output, slot=None):
    if command == ['light']:
        assert 'OXPINYIN_HEAVY_SLOT_HELD' not in env
        finished.append('light')
        holder.__exit__()
    else:
        assert finished[0] == 'light'
        assert 'OXPINYIN_HEAVY_SLOT_HELD' in env
        finished.append(command[0])
    return 0, 0.0, '', False
try:
    with mock.patch.object(Path, 'read_text', read), mock.patch.dict(m['main'].__globals__, execute=execute):
        with mock.patch.object(sys, 'argv', [script, '--runners', 'heavy-a,heavy-b,light',
                                           '--cells', 'bdb', '--no-build', '--jobs', '2']):
            assert m['main']() == 0
    assert finished[0] == 'light' and len(finished) == 3
finally:
    holder.__exit__()
'''
        result = subprocess.run([sys.executable, '-c', code, str(RUN_ALL)], env=self.env,
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('waiting for a heavy slot', result.stderr)

    def test_wrapper_cancellation_reaps_child_that_ignores_sigterm(self):
        code = ('import os, signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); '
                'print(os.getpid(), flush=True); time.sleep(30)')
        parent = subprocess.Popen([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                   sys.executable, '-c', code], env=self.env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertTrue(select.select([parent.stdout], [], [], 5)[0])
            child_pid = int(parent.stdout.readline())
            parent.terminate()
            _, error = parent.communicate(timeout=6)
            self.assertNotEqual(parent.returncode, 0, error)
            self.assertFalse(run_all['pid_alive'](child_pid))
            with run_all['HeavySlot']('probe', out=io.StringIO()):
                pass
        finally:
            if parent.poll() is None:
                parent.kill()
            parent.communicate(timeout=6)

    def test_child_keeps_slot_after_wrapper_death(self):
        code = 'import os,time; print(os.getpid(), flush=True); time.sleep(1.4)'
        wrapper = subprocess.Popen([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                    sys.executable, '-c', code], env=self.env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertTrue(select.select([wrapper.stdout], [], [], 3)[0])
            child_pid = int(wrapper.stdout.readline())
            wrapper.kill()
            wrapper.wait(timeout=3)
            contender = self.heavy(0)
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    contender.communicate(timeout=0.3)
                os.kill(child_pid, 0)
                _, stderr = contender.communicate(timeout=5)
                self.assertEqual(contender.returncode, 0, stderr)
                self.assertIn('waiting for a heavy slot', stderr)
            finally:
                if contender.poll() is None:
                    contender.kill()
                contender.communicate(timeout=5)
        finally:
            if wrapper.poll() is None:
                wrapper.kill()
            wrapper.communicate(timeout=5)

    def test_child_keeps_slot_after_aggregate_death(self):
        code = r'''import json, runpy, sys
from pathlib import Path
from unittest import mock
script=sys.argv[1]
m=runpy.run_path(script)
original=Path.read_text
row=next(r for r in json.loads(original(Path(script).with_name('runners.json'))) if r['name']=='pc-version')
row=dict(row, name='tiny-heavy', heavy=True, command=[sys.executable, '-c', 'import time; time.sleep(1.4)'])
def read(path,*args,**kwargs):
    return json.dumps([row]) if path.name=='runners.json' else original(path,*args,**kwargs)
with mock.patch.object(Path,'read_text',read), mock.patch.object(sys,'argv',[script,'--runners','tiny-heavy','--no-build','--cells','bdb']):
    sys.exit(m['main']())
'''
        parent = subprocess.Popen([sys.executable, '-c', code, str(RUN_ALL)], env=self.env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        path = Path(self.lock_dir.name) / 'heavy-slot-0'
        try:
            deadline = time.monotonic() + 3
            holder = None
            while time.monotonic() < deadline:
                holder = run_all['read_holder'](path)
                if holder and holder['pid'] != parent.pid:
                    break
                time.sleep(0.01)
            self.assertTrue(holder and holder['pid'] != parent.pid)
            parent.kill()
            parent.wait(timeout=3)
            contender = self.heavy(0)
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    contender.communicate(timeout=0.3)
                _, stderr = contender.communicate(timeout=5)
                self.assertEqual(contender.returncode, 0, stderr)
                self.assertIn('waiting for a heavy slot', stderr)
            finally:
                if contender.poll() is None:
                    contender.kill()
                contender.communicate(timeout=5)
        finally:
            if parent.poll() is None:
                parent.kill()
            parent.communicate(timeout=5)

    def test_normal_parent_exit_does_not_unlock_a_descendant(self):
        code = ('import os,time; pid=os.fork(); '
                'print(pid,flush=True) if pid else time.sleep(1.2)')
        parent = subprocess.Popen([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                   sys.executable, '-c', code], env=self.env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertTrue(select.select([parent.stdout], [], [], 3)[0])
            self.assertGreater(int(parent.stdout.readline()), 0)
            self.assertEqual(parent.wait(timeout=3), 0)
            contender = self.heavy(0)
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    contender.communicate(timeout=0.3)
                _, stderr = contender.communicate(timeout=5)
                self.assertEqual(contender.returncode, 0, stderr)
                self.assertIn('waiting for a heavy slot', stderr)
            finally:
                if contender.poll() is None:
                    contender.kill()
                contender.communicate(timeout=5)
        finally:
            if parent.poll() is None:
                parent.kill()
            parent.communicate(timeout=5)

    def test_dead_unrelated_and_replaced_markers_do_not_bypass_the_slot(self):
        dead = subprocess.Popen([sys.executable, '-c', 'pass'])
        dead.wait()
        sibling = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(5)'])
        try:
            for label, marker_pid, record_pid in [('dead', dead.pid, dead.pid),
                                                  ('unrelated', sibling.pid, sibling.pid),
                                                  ('replaced', os.getpid(), sibling.pid)]:
                with self.subTest(label=label):
                    with run_all['HeavySlot']('holder', out=io.StringIO()) as slot:
                        path = Path(self.lock_dir.name) / 'heavy-slot-0'
                        record = json.loads(path.read_text())
                        record['pid'] = record_pid
                        run_all['write_holder'](slot._file, record)
                        env = dict(self.env, OXPINYIN_HEAVY_SLOT_HELD=json.dumps(
                            dict(lock_dir=self.lock_dir.name, slot=0, pid=marker_pid)))
                        child = subprocess.Popen([sys.executable, str(RUN_ALL), '--heavy-slot', '--',
                                                  sys.executable, '-c', 'pass'], env=env,
                                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                        with self.assertRaises(subprocess.TimeoutExpired):
                            child.communicate(timeout=0.2)
                    try:
                        _, stderr = child.communicate(timeout=5)
                        self.assertEqual(child.returncode, 0, stderr)
                        self.assertIn('stale OXPINYIN_HEAVY_SLOT_HELD', stderr)
                        self.assertNotIn('inherited from ancestor', stderr)
                    finally:
                        if child.poll() is None:
                            child.kill()
                            child.communicate(timeout=3)
        finally:
            sibling.terminate()
            sibling.wait(timeout=3)

    def test_unsupported_flock_fails_with_the_error_and_path(self):
        with mock.patch.object(run_all['fcntl'], 'flock', side_effect=OSError(errno.EOPNOTSUPP, 'Operation not supported')) as locked:
            with self.assertRaises(OSError) as raised:
                with run_all['HeavySlot']('unsupported', out=io.StringIO()):
                    self.fail('The unsupported lock was acquired.')
        self.assertEqual(raised.exception.errno, errno.EOPNOTSUPP)
        self.assertIn('Operation not supported', str(raised.exception))
        self.assertIn(str(Path(self.lock_dir.name) / 'heavy-slot-0'), str(raised.exception))
        self.assertEqual(locked.call_count, 1)

    def test_interrupted_flock_is_retried(self):
        original = run_all['fcntl'].flock
        attempts = []
        def lock(file, operation):
            attempts.append(operation)
            if len(attempts) == 1:
                raise InterruptedError(errno.EINTR, 'Interrupted system call')
            return original(file, operation)
        with mock.patch.object(run_all['fcntl'], 'flock', lock):
            with run_all['HeavySlot']('interrupted', out=io.StringIO()):
                pass
        self.assertEqual(len(attempts), 2)

    def test_light_work_proceeds_while_the_slot_is_held(self):
        with run_all['HeavySlot']('test holder', out=io.StringIO()):
            started = time.monotonic()
            result = subprocess.run([sys.executable, str(RUN_ALL), '--runners', 'pc-version', '--no-build',
                                     '--cells', 'bdb'], env=self.env, capture_output=True, text=True, timeout=60)
            elapsed = time.monotonic() - started
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertLess(elapsed, 20)
        rows = [line for line in result.stdout.splitlines() if line.startswith('pc-version |')]
        self.assertEqual(len(rows), 1)
        fields = [f.strip() for f in rows[0].split('|')]
        self.assertEqual(fields[2], 'PASS')
        # lock_wait_seconds, slot_wait_seconds, build_slot_wait_seconds: a light run waits for nothing
        self.assertEqual((fields[5], fields[6], fields[7]), ('0.0', '0.0', '0.0'))
        self.assertIn('lock_wait_seconds | slot_wait_seconds | build_slot_wait_seconds', result.stdout)
        self.assertFalse(run_all['is_heavy']({'name': 'pc-version'}))

    def test_heavy_rows_are_the_registered_ones(self):
        rows = json.loads((HERE / 'runners.json').read_text())
        heavy = sorted(r['name'] for r in rows if run_all['is_heavy'](r))
        self.assertEqual(heavy, ['bisect', 'contract-diff', 'open-counter-diff', 'transformed-options-sweep'])

    def test_a_stale_slot_is_reclaimed_with_a_message(self):
        dead = subprocess.Popen([sys.executable, '-c', 'pass'])
        dead.wait()
        slot = Path(self.lock_dir.name) / 'heavy-slot-0'
        slot.write_text(json.dumps(dict(pid=dead.pid, worktree='/elsewhere', command='old campaign',
                                        since='2026-10-19T00:00:00Z')))
        out = io.StringIO()
        with run_all['HeavySlot']('new work', out=out) as held:
            self.assertEqual(held.reclaimed['pid'], dead.pid)
            self.assertEqual(json.loads(slot.read_text())['command'], 'new work')
        self.assertIn(f'reclaimed stale heavy slot 0: pid {dead.pid} in /elsewhere', out.getvalue())
        self.assertEqual(slot.read_text(), '')

    def test_slot_count_comes_from_the_environment(self):
        os.environ['OXPINYIN_HEAVY_SLOTS'] = '2'
        try:
            out = io.StringIO()
            with run_all['HeavySlot']('a', out=out) as a, run_all['HeavySlot']('b', out=out) as b:
                self.assertEqual({a.slot, b.slot}, {0, 1})
                self.assertLess(b.waited, 0.5)
            self.assertEqual(out.getvalue(), '')
        finally:
            os.environ.pop('OXPINYIN_HEAVY_SLOTS', None)
        self.assertEqual(run_all['heavy_slot_count'](), 1)


if __name__ == '__main__':
    unittest.main()
