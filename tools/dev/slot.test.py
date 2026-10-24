#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Unit tests for tools.dev.slot."""
import io
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host, slot


class SlotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        env = dict(os.environ, OXPINYIN_LOCK_DIR=self.temp.name, OXPINYIN_HEAVY_SLOTS='1')
        for name in ('OXPINYIN_HEAVY_SLOT_HELD', 'CARGO_BUILD_JOBS', 'RUST_TEST_THREADS'):
            env.pop(name, None)
        self.env = env
        self.patch = mock.patch.dict(os.environ, env, clear=True)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def run_python(self, code, *, wait=True, stdin=None):
        process = subprocess.Popen([sys.executable, '-c', code], cwd=ROOT, env=self.env,
                                   stdin=stdin, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        if wait:
            out, err = process.communicate(timeout=10)
            return process.returncode, out, err
        return process

    def test_two_concurrent_one_slot_sleeps_serialise(self):
        code = ("import sys, time\n"
                "from tools.dev.slot import HeavySlot\n"
                "with HeavySlot('sleep', out=sys.stderr):\n"
                "    print('started', flush=True)\n"
                "    time.sleep(0.8)\n")
        first = self.run_python(code, wait=False)
        try:
            self.assertEqual(first.stdout.readline().strip(), 'started')
            time.sleep(0.15)
            started = time.monotonic()
            second = self.run_python(code)
            elapsed = time.monotonic() - started
        finally:
            first.communicate(timeout=10)
        self.assertEqual(second[0], 0, second[2])
        self.assertIn('started', second[1])
        self.assertGreaterEqual(elapsed, 0.6)
        self.assertLess(elapsed, 5)

    def test_killed_holder_releases_only_after_its_workload_ends(self):
        wrapper = ("import os, sys, time\n"
                   "from tools.dev.slot import HeavySlot\n"
                   "held = HeavySlot('wrapper', out=sys.stderr)\n"
                   "held.__enter__()\n"
                   "pid = os.fork()\n"
                   "if pid == 0:\n"
                   "    time.sleep(0.9)\n"
                   "    os._exit(0)\n"
                   "print('ready', flush=True)\n")
        holder = self.run_python(wrapper, wait=False)
        try:
            self.assertEqual(holder.stdout.readline().strip(), 'ready')
            holder.kill()
            holder.wait(timeout=5)
            started = time.monotonic()
            contender = self.run_python(
                "import sys\n"
                "from tools.dev.slot import HeavySlot\n"
                "with HeavySlot('contender', out=sys.stderr):\n"
                "    print('acquired', flush=True)\n")
            elapsed = time.monotonic() - started
        finally:
            if holder.poll() is None:
                holder.kill()
            holder.wait(timeout=5)
        self.assertEqual(contender[0], 0, contender[2])
        self.assertIn('acquired', contender[1])
        self.assertGreaterEqual(elapsed, 0.4)
        self.assertIn('reclaimed stale heavy slot', contender[2])

    def test_stale_marker_is_reported_and_the_caller_acquires_normally(self):
        dead = subprocess.Popen([sys.executable, '-c', 'pass'])
        dead.wait()
        marker = json.dumps(dict(lock_dir=self.temp.name, slot=0, pid=dead.pid))
        out = io.StringIO()
        with mock.patch.dict(os.environ, {'OXPINYIN_HEAVY_SLOT_HELD': marker}):
            with slot.HeavySlot('caller', out=out) as held:
                self.assertEqual(held.slot, 0)
        self.assertIn('stale OXPINYIN_HEAVY_SLOT_HELD', out.getvalue())
        self.assertIn('acquire a slot normally', out.getvalue())

    @unittest.skipIf(sys.platform == 'win32', 'Unix descriptor inheritance')
    def test_sigkill_of_tools_dev_wrapper_keeps_the_workload_slot(self):
        child = ("import os, sys\n"
                 "print('workload-ready:' + str(os.getpid()), flush=True)\n"
                 "sys.stdin.readline()\n")
        wrapper = ("from tools.dev import host\n"
                   "from tools.dev.cli import main\n"
                   "import sys\n"
                   "host.resource_snapshot = lambda: dict(available_mib=16000, cpus=1, "
                   "cargo_cpus=1, notes=['synthetic resources for a sleeping child'])\n"
                   f"raise SystemExit(main(['slot', 'run', '--', sys.executable, '-c', {child!r}]))\n")
        holder = self.run_python(wrapper, wait=False, stdin=subprocess.PIPE)
        contender = None
        child_pid = None
        try:
            ready, _, _ = select.select([holder.stdout], [], [], 5)
            self.assertTrue(ready, 'the workload did not start')
            line = holder.stdout.readline().strip()
            self.assertTrue(line.startswith('workload-ready:'), line)
            child_pid = int(line.split(':', 1)[1])
            holder.send_signal(signal.SIGKILL)
            self.assertEqual(holder.wait(timeout=5), -signal.SIGKILL)
            contender = self.run_python(
                "import sys\n"
                "from tools.dev.slot import HeavySlot\n"
                "print('contender-ready', flush=True)\n"
                "with HeavySlot('contender', out=sys.stderr):\n"
                "    print('acquired', flush=True)\n", wait=False)
            ready, _, _ = select.select([contender.stdout], [], [], 5)
            self.assertTrue(ready, 'the contender did not start')
            self.assertEqual(contender.stdout.readline().strip(), 'contender-ready')
            with self.assertRaises(subprocess.TimeoutExpired):
                contender.wait(timeout=0.2)
            holder.stdin.write('release\n')
            holder.stdin.flush()
            holder.stdin.close()
            holder.stdin = None
            out, err = contender.communicate(timeout=5)
            self.assertEqual(contender.returncode, 0, err)
            self.assertIn('acquired', out)
            self.assertIn('waiting for a heavy slot', err)
        finally:
            if holder.stdin is not None:
                holder.stdin.close()
                holder.stdin = None
            if holder.poll() is None:
                holder.kill()
            if child_pid is not None and contender is not None and contender.poll() is None:
                try:
                    os.killpg(child_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if contender is not None:
                if contender.poll() is None:
                    contender.kill()
                contender.communicate(timeout=5)
            holder.communicate(timeout=5)

    def test_unknown_memory_fails_closed(self):
        env = dict(self.env, OXPINYIN_RESOURCE_WAIT_SECONDS='0')
        out = io.StringIO()
        admission = slot.HeavyAdmission(
            'unknown memory', env, out=out, poll=0.01,
            resource_snapshot=lambda: dict(available_mib=None, cpus=2, cargo_cpus=2,
                                          notes=['synthetic unknown memory']))
        with self.assertRaisesRegex(host.ResourceBlocked, '^resource-blocked: available memory unknown$'):
            with admission:
                self.fail('admission should fail closed')
        self.assertIn('no process started', out.getvalue())

    def test_admission_omits_headroom_only_for_a_finite_compatibility_guest(self):
        env = dict(self.env, CARGO_BUILD_JOBS='2', RUST_TEST_THREADS='2',
                   OXPINYIN_RESOURCE_WAIT_SECONDS='0')
        for compat, limit, available, succeeds in (('1', 4096, 1200, True),
                                                   ('1', 4096, 1199, False),
                                                   ('1', None, 1200, False),
                                                   ('0', 4096, 1200, False)):
            with self.subTest(compat=compat, limit=limit, available=available):
                out = io.StringIO()
                admission = slot.HeavyAdmission(
                    'limited guest', dict(env, OXPINYIN_COMPAT_CONTAINER=compat), out=out,
                    resource_snapshot=lambda: dict(available_mib=available, memory_limit_mib=limit,
                                                   cpus=2, cargo_cpus=2, notes=[]))
                if succeeds:
                    with admission:
                        self.assertEqual(admission.estimate, 1200)
                        self.assertEqual(admission.headroom, 0)
                else:
                    room = available if compat == '1' and limit is not None else 0
                    with self.assertRaisesRegex(host.ResourceBlocked, f'1200 MiB needed, {room} MiB available'):
                        with admission:
                            self.fail('work started without sufficient admitted memory')

    def hold(self, index, estimate):
        """Lock slot INDEX through a second open file description: a live holder."""
        file = (Path(self.temp.name) / f'{slot.HEAVY_SLOT}-{index}').open('a+')
        self.addCleanup(file.close)
        slot.lock_file(file, slot.LOCK_EX | slot.LOCK_NB)
        host.write_holder(file, dict(pid=os.getpid(), estimate_mib=estimate))

    def test_holder_is_charged_only_the_rest_of_its_estimate(self):
        self.hold(1, 2400)
        pid = os.getpid()
        for use, charge in ((900, 1500), (2400, 0), (3000, 0)):
            with self.subTest(use=use), mock.patch.object(host, 'process_tree_use_mib',
                                                          return_value=use) as read:
                notes = []
                self.assertEqual(slot.other_reservations(Path(self.temp.name), 0, notes), charge)
                read.assert_called_once_with(pid)
                self.assertEqual(notes, [f'slot 1 pid {pid}: estimate 2400 MiB, '
                                         f'process tree use {use} MiB, charge {charge} MiB'])
        out = io.StringIO()
        with mock.patch.object(host, 'process_tree_use_mib', return_value=900):
            admission = slot.HeavyAdmission(
                'second session', self.env, out=out, poll=0.01,
                resource_snapshot=lambda: dict(available_mib=5000, total_mib=16384, cpus=10,
                                               cargo_cpus=10, notes=['synthetic 5000 MiB']))
            with admission:
                self.assertEqual(admission.slot.slot, 0)
                self.assertEqual(admission.available, 3500)
        self.assertIn('other slot reservations=1500 MiB', out.getvalue())

    def test_holder_with_unreadable_use_is_charged_its_full_estimate(self):
        self.hold(1, 2400)
        notes = []
        with mock.patch.object(host, 'process_tree_use_mib', return_value=None):
            self.assertEqual(slot.other_reservations(Path(self.temp.name), 0, notes), 2400)
        self.assertEqual(notes, [f'slot 1 pid {os.getpid()}: estimate 2400 MiB, '
                                 'process tree use unknown, charge 2400 MiB'])

    def test_missing_smaps_rollup_charges_the_full_estimate(self):
        self.hold(1, 2400)
        pid, reader = os.getpid(), host.process_tree_use_mib
        with tempfile.TemporaryDirectory() as temporary:
            # A synthetic Linux /proc: the holder and one child, Pss 50 and 250 MiB.
            proc = Path(temporary)
            for process, parent in ((pid, 1), (pid + 1, pid)):
                (proc / str(process)).mkdir()
                (proc / str(process) / 'stat').write_text(f'{process} (python) S {parent} 0 0\n')
            (proc / str(pid) / 'smaps_rollup').write_text('Rss: 61440 kB\nPss: 51200 kB\n')
            child = proc / str(pid + 1) / 'smaps_rollup'
            for rollup, use, charge in ((None, 'unknown', 2400),
                                        ('Rss: 409600 kB\nPss: 256000 kB\n', '300 MiB', 2100)):
                with self.subTest(rollup=rollup):
                    if rollup is not None:
                        child.write_text(rollup)
                    notes = []
                    with mock.patch.object(sys, 'platform', 'linux'), \
                            mock.patch.object(host, 'process_tree_use_mib', lambda target: reader(target, proc)):
                        self.assertEqual(slot.other_reservations(Path(self.temp.name), 0, notes), charge)
                    self.assertEqual(notes, [f'slot 1 pid {pid}: estimate 2400 MiB, '
                                             f'process tree use {use}, charge {charge} MiB'])

    def test_live_holder_is_charged_its_estimate_less_its_measured_use(self):
        code = ("import os, sys\n"
                "from tools.dev import host\n"
                "from tools.dev.slot import HeavySlot\n"
                "with HeavySlot('holder', out=sys.stderr) as held:\n"
                "    host.write_holder(held._file, dict(pid=os.getpid(), estimate_mib=2400))\n"
                "    data = os.urandom(64 * 1024 * 1024)\n"
                "    print('ready', flush=True)\n"
                "    sys.stdin.readline()\n")
        holder = self.run_python(code, wait=False, stdin=subprocess.PIPE)
        try:
            self.assertEqual(holder.stdout.readline().strip(), 'ready')
            notes = []
            charge = slot.other_reservations(Path(self.temp.name), None, notes)
        finally:
            holder.stdin.close()
            holder.communicate(timeout=10)
        self.assertGreater(charge, 0)
        self.assertLessEqual(charge, 2400 - 64)
        self.assertRegex(notes[0], rf'^slot 0 pid {holder.pid}: estimate 2400 MiB, '
                                   rf'process tree use [0-9]+ MiB, charge {charge} MiB$')

    def test_relative_lock_directory_is_rejected_before_a_child_starts(self):
        with mock.patch.dict(os.environ, {'OXPINYIN_LOCK_DIR': 'relative'}):
            with self.assertRaisesRegex(ValueError, 'must be an absolute path'):
                with slot.HeavySlot('relative', out=io.StringIO()):
                    self.fail('the relative lock directory was accepted')

    def test_contract_workers_fit_after_other_live_reservations(self):
        self.hold(1, 4096)
        env = dict(self.env, CARGO_BUILD_JOBS='1', RUST_TEST_THREADS='1',
                   OXPINYIN_CONTRACT_JOBS='4', OXPINYIN_RESOURCE_WAIT_SECONDS='0')
        with mock.patch.object(host, 'process_tree_use_mib', return_value=0):
            admission = slot.HeavyAdmission(
                'parallel contracts', env, runner=True, out=io.StringIO(),
                resource_snapshot=lambda: dict(available_mib=8000, total_mib=16384,
                                               cpus=4, cargo_cpus=4, notes=[]))
            with admission:
                self.assertEqual(admission.env['OXPINYIN_CONTRACT_JOBS'], '2')
                self.assertEqual(admission.estimate, 2048)
                record = host.read_holder(Path(self.temp.name) / 'heavy-slot-0')
                self.assertEqual(record['estimate_mib'], 2048)

    def test_no_contract_worker_starts_when_even_one_cannot_fit(self):
        env = dict(self.env, CARGO_BUILD_JOBS='1', RUST_TEST_THREADS='1',
                   OXPINYIN_CONTRACT_JOBS='4', OXPINYIN_RESOURCE_WAIT_SECONDS='0')
        admission = slot.HeavyAdmission(
            'parallel contracts', env, runner=True, out=io.StringIO(),
            resource_snapshot=lambda: dict(available_mib=2661, total_mib=16384,
                                           cpus=4, cargo_cpus=4, notes=[]))
        with self.assertRaisesRegex(host.ResourceBlocked, '1024 MiB needed, 1023 MiB available'):
            with admission:
                self.fail('an unadmitted worker could start')
        self.assertEqual((Path(self.temp.name) / 'heavy-slot-0').read_text(), '')


if __name__ == '__main__':
    unittest.main()
