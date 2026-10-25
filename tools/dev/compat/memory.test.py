#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Container budget regressions; no runtime, oracle or build required."""
import io
import ast
import hashlib
import json
import os
from pathlib import Path
import signal
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import host, slot, test_support
from tools.dev.compat import boundary, memory


class MemoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.env = dict(os.environ, OXPINYIN_LOCK_DIR=self.temp.name,
                        OXPINYIN_RESOURCE_WAIT_SECONDS='0', CARGO_BUILD_JOBS='1',
                        RUST_TEST_THREADS='1')
        self.env.pop(slot.HEAVY_SLOT_HELD, None)
        self.patch = mock.patch.dict(os.environ, self.env, clear=True)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.out = io.StringIO()
        self.resources = dict(available_mib=16000, cpus=2, cargo_cpus=2, notes=[])
        self.read = mock.patch.object(host, 'resource_snapshot', side_effect=lambda: self.resources.copy())
        self.read.start()
        self.addCleanup(self.read.stop)
        slot.CANCELLED.clear()

    def reservation(self, size=4096):
        reservation = slot.ContainerReservation('/usr/bin/docker', 'docker', 'test-container', size,
                                                 out=self.out, poll=0.01)
        reservation.acquire()
        self.addCleanup(reservation.close)
        return reservation

    def test_positive_memory_size_and_default_covers_two_job_estimates(self):
        self.assertEqual(host.container_memory_mib(None), host.CONTAINER_MEMORY_MIB)
        self.assertGreaterEqual(host.CONTAINER_MEMORY_MIB, 2 * host.RUSTC_PEAK_MIB)
        self.assertGreaterEqual(host.CONTAINER_MEMORY_MIB, 2 * host.RUNNER_PEAK_MIB)
        self.assertEqual(host.container_memory_mib('1'), 1)
        for value in ('0', '-1', '+1', '1.5', '1G', '', ' 2', 'true'):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, 'positive integer'):
                host.container_memory_mib(value)

    def test_written_before_create_with_no_native_slot_or_inner_state(self):
        reservation = self.reservation()
        record = json.loads(reservation.path.read_text())
        self.assertEqual(record['memory_mib'], 4096)
        self.assertEqual(record['phase'], 'creating')
        self.assertEqual(set(record), {'schema', 'runtime', 'kind', 'name', 'token',
                                       'memory_mib', 'phase', 'since'})
        self.assertFalse(list(self.directory.glob('heavy-slot-*')))

    def test_native_subtracts_full_limit_and_preserves_headroom(self):
        reservation = self.reservation()
        with slot.HeavyAdmission('native', self.env, out=self.out) as admitted:
            self.assertEqual(admitted.available, 16000 - 4096)
        self.resources['available_mib'] = 4096 + host.HEADROOM_MIB + 599
        with self.assertRaises(host.ResourceBlocked):
            with slot.HeavyAdmission('native blocked', self.env, out=self.out):
                self.fail('native work started inside container budget')
        self.assertTrue(reservation.path.exists())

    def test_unknown_or_inadequate_memory_waits_then_fails_without_reservation(self):
        for available in (None, 4096 + host.HEADROOM_MIB - 1):
            self.resources['available_mib'] = available
            with self.subTest(available=available), self.assertRaises(host.ResourceBlocked):
                self.reservation()
            self.assertFalse(list(self.directory.glob('container-memory-*.json')))
        self.assertIn('no process started', self.out.getvalue())

    def test_memory_wait_retries_and_can_admit_after_resources_change(self):
        self.resources['available_mib'] = None
        reads = []
        def read():
            reads.append(1)
            return dict(self.resources, available_mib=None if len(reads) == 1 else 16000)
        with mock.patch.dict(os.environ, OXPINYIN_RESOURCE_WAIT_SECONDS='1'), \
                mock.patch.object(host, 'resource_snapshot', side_effect=read):
            reservation = self.reservation()
        self.assertEqual(len(reads), 2)
        self.assertTrue(reservation.path.exists())

    def test_container_admission_subtracts_native_holders(self):
        self.resources['available_mib'] = 4096 + host.HEADROOM_MIB + 599
        # The holder's tree has used nothing yet, so its whole estimate counts.
        with mock.patch.object(host, 'process_tree_use_mib', return_value=0), \
                slot.HeavyAdmission('native', self.env, out=self.out):
            with self.assertRaises(host.ResourceBlocked):
                self.reservation()
            self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def test_launcher_excludes_only_its_validated_ancestor_slot(self):
        code = ("from unittest import mock\n"
                "from tools.dev import host, slot\n"
                "host.resource_snapshot=lambda: dict(available_mib=6144, cpus=2, cargo_cpus=2, notes=[])\n"
                "host.process_tree_use_mib=lambda pid: 0\n"
                "reservation=slot.ContainerReservation('/usr/bin/podman', 'podman', 'nested', 4096)\n"
                "try:\n"
                "    reservation.acquire()\n"
                "    with mock.patch.object(slot, 'container_recovery_state', return_value='gone'):\n"
                "        reservation.release()\n"
                "finally:\n"
                "    reservation.close()\n")
        def launch(env, held):
            return subprocess.run([sys.executable, '-c', code], cwd=ROOT, env=env,
                                  pass_fds=held.slot.pass_fds(), capture_output=True, text=True, timeout=5)
        with slot.HeavyAdmission('launcher', self.env, out=self.out) as held:
            env = held.slot.child_env(self.env)
            result = launch(env, held)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('other slot reservations=0 MiB', result.stderr)
            marker = json.loads(env[slot.HEAVY_SLOT_HELD])
            invalid = ({slot.HEAVY_SLOT_HELD: json.dumps(dict(marker, pid=os.getppid()))},
                       {slot.HEAVY_SLOT_HELD: json.dumps(dict(marker, lock_dir=str(self.directory / 'elsewhere')))},
                       {slot.HEAVY_SLOT_HELD: None})
            for change in invalid:
                with self.subTest(change=change):
                    changed = dict(env, **change)
                    if changed[slot.HEAVY_SLOT_HELD] is None:
                        changed.pop(slot.HEAVY_SLOT_HELD)
                    result = launch(changed, held)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('resource-blocked: 4096 MiB needed, 3496 MiB available', result.stderr)
            with mock.patch.dict(os.environ, OXPINYIN_HEAVY_SLOTS='2'):
                with slot.HeavyAdmission('other work', self.env, out=self.out):
                    result = launch(env, held)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('resource-blocked: 4096 MiB needed, 3496 MiB available', result.stderr)

    def test_inner_domain_uses_its_effective_limit_and_own_reservations(self):
        self.reservation()
        inner = self.directory / 'inner'
        with mock.patch.dict(os.environ, OXPINYIN_LOCK_DIR=str(inner)):
            self.resources['available_mib'] = 4096
            with slot.HeavyAdmission('inner', self.env, out=self.out) as admitted:
                self.assertEqual(admitted.available, 4096)
            self.resources['available_mib'] = host.HEADROOM_MIB + 599
            with self.assertRaises(host.ResourceBlocked):
                with slot.HeavyAdmission('inner limited', self.env, out=self.out):
                    self.fail('inner effective limit ignored')

    def test_record_updates_and_recovery_share_native_admission_serialization(self):
        def assert_locked():
            with (self.directory / 'heavy-admission').open('a+') as probe:
                with self.assertRaises(BlockingIOError):
                    slot.lock_file(probe, slot.LOCK_EX | slot.LOCK_NB)
        original = slot.write_container_record
        def write(path, record):
            assert_locked()
            original(path, record)
        def gone(_):
            assert_locked()
            return 'gone'
        with mock.patch.object(slot, 'write_container_record', side_effect=write):
            reservation = self.reservation()
            reservation.created()
        reservation.close()
        with mock.patch.object(host, 'container_liveness', side_effect=gone):
            with slot.admission_lock(self.directory):
                self.assertEqual(slot.container_reservations(self.directory, out=self.out), 0)
        self.assertFalse(reservation.path.exists())

    def test_release_requires_confirmed_exit(self):
        reservation = self.reservation()
        reservation.created()
        for state in ('live', 'unknown'):
            with mock.patch.object(host, 'container_liveness', return_value=state):
                self.assertFalse(reservation.release())
                self.assertTrue(reservation.path.exists())
        with mock.patch.object(host, 'container_liveness', return_value='gone'):
            self.assertTrue(reservation.release())
        self.assertFalse(reservation.path.exists())

    def test_recovery_removes_only_confirmed_gone_and_keeps_unknown(self):
        reservation = self.reservation()
        reservation.created()
        reservation.close()
        for state in ('live', 'unknown'):
            with mock.patch.object(host, 'container_liveness', return_value=state):
                with slot.admission_lock(self.directory):
                    self.assertEqual(slot.container_reservations(self.directory, out=self.out), 4096)
                self.assertTrue(reservation.path.exists())
        self.assertIn('liveness unknown', self.out.getvalue())
        with mock.patch.object(host, 'container_liveness', return_value='gone'):
            with slot.admission_lock(self.directory):
                self.assertEqual(slot.container_reservations(self.directory, out=self.out), 0)
        self.assertFalse(reservation.path.exists())

    def test_unsettled_creation_cannot_be_reclaimed_even_if_name_is_absent(self):
        reservation = self.reservation()
        reservation.close()
        with mock.patch.object(host, 'container_liveness', return_value='gone'):
            with slot.admission_lock(self.directory):
                self.assertEqual(slot.container_reservations(self.directory, out=self.out), 4096)
        self.assertTrue(reservation.path.exists())
        self.assertIn('creation outcome unknown', self.out.getvalue())

    def test_corrupt_reservation_fails_native_admission_closed(self):
        (self.directory / 'container-memory-corrupt.json').write_text('{')
        with self.assertRaisesRegex(host.ResourceBlocked, 'available memory unknown'):
            with slot.HeavyAdmission('native', self.env, out=self.out):
                self.fail('corrupt budget was ignored')

    def test_limit_is_verified_before_start_and_reservation_precedes_create(self):
        events = []
        def command(runtime, args, **kwargs):
            records = list(self.directory.glob('container-memory-*.json'))
            self.assertEqual(len(records), 1)
            events.append(args[0])
            if args[0] == 'create':
                self.assertEqual(args[args.index('--memory') + 1], '4096M')
                self.assertEqual(args[args.index('--memory-swap') + 1], '4096M')
                self.assertIn('argument with spaces', args)
            return 0
        with mock.patch.object(memory, 'call', side_effect=command), \
                mock.patch.object(memory, 'verify_limit', side_effect=lambda *_: events.append('verified')), \
                mock.patch.object(host, 'container_liveness', return_value='gone'):
            self.assertEqual(memory.run('/usr/bin/docker', 'docker', 'test', 4096,
                                        ['image', 'argument with spaces'], out=self.out), 0)
        self.assertLess(events.index('create'), events.index('verified'))
        self.assertLess(events.index('verified'), events.index('start'))
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def test_no_start_without_verified_limit_or_native_reservation(self):
        for failure in ('limit', 'reservation'):
            events = []
            self.resources['available_mib'] = None if failure == 'reservation' else 16000
            with self.subTest(failure=failure), \
                    mock.patch.object(memory, 'call', side_effect=lambda _, args, **kw: events.append(args[0]) or 0), \
                    mock.patch.object(memory, 'verify_limit', side_effect=ValueError('hard limit unavailable')), \
                    mock.patch.object(host, 'container_liveness', return_value='gone'):
                with self.assertRaises(ValueError):
                    memory.run('/usr/bin/docker', 'docker', 'test', 4096, ['image'], out=self.out)
            self.assertNotIn('start', events)

    def test_cancellation_stops_and_confirms_exit_before_release(self):
        events = []
        def command(_, args, **kwargs):
            events.append(args[0])
            if args[0] == 'start':
                raise slot.Cancelled('synthetic cancellation')
            return 0
        def liveness(_):
            events.append('exit-confirmed')
            self.assertTrue(list(self.directory.glob('container-memory-*.json')))
            return 'gone'
        with mock.patch.object(memory, 'call', side_effect=command), \
                mock.patch.object(memory, 'verify_limit'), \
                mock.patch.object(host, 'container_liveness', side_effect=liveness):
            with self.assertRaises(slot.Cancelled):
                memory.run('/usr/bin/docker', 'docker', 'test', 4096, ['image'], out=self.out)
        self.assertLess(events.index('stop'), events.index('exit-confirmed'))
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    @unittest.skipIf(sys.platform == 'win32', 'Unix launcher signal semantics')
    def test_launcher_sigkill_retains_live_container_reservation(self):
        code = """import sys
from tools.dev import slot, host
host.resource_snapshot=lambda: dict(available_mib=16000, cpus=1, cargo_cpus=1, notes=[])
r=slot.ContainerReservation('/usr/bin/docker', 'docker', 'live', 4096)
r.acquire()
r.created()
print('ready', flush=True)
sys.stdin.readline()
"""
        process = subprocess.Popen([sys.executable, '-c', code], cwd=ROOT, env=self.env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        try:
            self.assertEqual(process.stdout.readline().strip(), 'ready')
            process.kill()
            self.assertEqual(process.wait(timeout=5), -signal.SIGKILL)
            with mock.patch.object(host, 'container_liveness', return_value='live'):
                with slot.admission_lock(self.directory):
                    self.assertEqual(slot.container_reservations(self.directory, out=self.out), 4096)
            self.assertEqual(len(list(self.directory.glob('container-memory-*.json'))), 1)
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=5)

    def test_runtime_metadata_requires_exact_limit_and_owner(self):
        record = dict(kind='docker', memory_mib=512, token='owner')
        row = dict(HostConfig=dict(Memory=512 * 1024**2, MemorySwap=512 * 1024**2),
                   Config=dict(Labels={'io.oxpinyin.memory-reservation': 'owner'}))
        with mock.patch.object(host, 'inspect_container', return_value=row):
            memory.verify_limit(record)
            row['HostConfig']['Memory'] = 0
            with self.assertRaises(ValueError):
                memory.verify_limit(record)
        apple = dict(configuration=dict(resources=dict(memoryInBytes=512 * 1024**2),
                                        labels={'io.oxpinyin.memory-reservation': 'owner'}))
        record['kind'] = 'container'
        with mock.patch.object(host, 'inspect_container', return_value=apple):
            memory.verify_limit(record)
            apple['configuration']['labels'].clear()
            with self.assertRaises(ValueError):
                memory.verify_limit(record)

    def test_apple_inspect_observed_object_and_documented_scalar_status(self):
        record = dict(kind='container', token='owner')
        # container 1.5.0, exact `container inspect NAME` on this Mac:
        # running: status has networks, startedDate and state; stopped keeps
        # startedDate, clears networks and changes state to stopped.
        for status, expected in ((dict(networks=[dict(network='default')],
                                       startedDate='2026-10-20T15:40:28Z', state='running'), 'live'),
                                 (dict(networks=[], startedDate='2026-10-20T15:40:28Z',
                                       state='stopped'), 'gone'),
                                 ('running', 'live'), ('stopped', 'gone')):
            row = dict(configuration=dict(labels={'io.oxpinyin.memory-reservation': 'owner'}),
                       status=status)
            with self.subTest(status=status), mock.patch.object(host, 'inspect_container', return_value=row):
                self.assertEqual(host.container_liveness(record), expected)
        for status in (None, [], 1, {}, dict(state=[]), dict(state='unexpected'), 'unexpected'):
            row = dict(configuration=dict(labels={'io.oxpinyin.memory-reservation': 'owner'}), status=status)
            with self.subTest(status=status), mock.patch.object(host, 'inspect_container', return_value=row):
                self.assertEqual(host.container_liveness(record), 'unknown')

    def test_stdout_broken_pipe_keeps_draining_full_console_until_child_exits(self):
        class BrokenStdout:
            def __init__(self, flush_failure):
                self.buffer = self
                self.flush_failure = flush_failure
                self.writes = 0

            def write(self, data):
                self.writes += 1
                if not self.flush_failure:
                    raise BrokenPipeError('synthetic closed stdout')

            def flush(self):
                raise BrokenPipeError('synthetic closed stdout on flush')

        for flush_failure in (False, True):
            output = self.directory / 'temporary-console'
            broken = BrokenStdout(flush_failure)
            with self.subTest(flush_failure=flush_failure), \
                    mock.patch.object(memory.sys, 'stdout', broken), \
                    mock.patch.object(slot, 'wait_child', side_effect=lambda child: child.wait(timeout=3)):
                rc = memory.call(sys.executable, ['-c', "import sys; sys.stdout.buffer.write(b'x' * (2 * 1024**2)); sys.stdout.buffer.flush()"],
                                 console=output)
            self.assertEqual(rc, 0)
            self.assertEqual(output.read_bytes(), b'x' * (2 * 1024**2))
            self.assertEqual(broken.writes, 1)
            output.unlink()

    def test_failed_create_releases_only_after_complete_token_listing_proves_absence(self):
        runtime = self.fake_runtime()
        for failure in ('CREATE_FAIL', 'CREATE_FAIL_AFTER'):
            with self.subTest(failure=failure):
                process = self.launcher(runtime, **{failure: '1'})
                _, error = process.communicate(timeout=10)
                self.assertNotEqual(process.returncode, 0)
                self.assertIn('create failed (125)', error)
                self.assertIn('reservation released', error)
                self.assertFalse((self.directory / 'started').exists())
                self.assertFalse((self.directory / 'runtime-state').exists())
                self.assertFalse(list(self.directory.glob('container-memory-*.json')))
                self.assertFalse(list(self.directory.glob('container-memory-*.guard')))

    def test_failed_create_unknown_listing_is_retained_and_later_recovers(self):
        reservation = self.reservation()
        with mock.patch.object(host, 'container_token_absent', return_value=None):
            self.assertFalse(reservation.create_failed())
        self.assertTrue(reservation.path.exists())
        self.assertEqual(json.loads(reservation.path.read_text())['phase'], 'create-failed')
        reservation.close()
        with mock.patch.object(host, 'container_token_absent', return_value=True):
            with slot.admission_lock(self.directory):
                self.assertEqual(slot.container_reservations(self.directory, out=self.out), 0)
        self.assertFalse(reservation.path.exists())

    def test_token_absence_requires_complete_listing_and_all_state_label_filter(self):
        record = dict(runtime='/usr/bin/docker', kind='docker', token='owner')
        for rc, data, expected in ((0, '', True), (0, '{}', False),
                                   (0, '{broken', None), (1, '', None)):
            with self.subTest(rc=rc, data=data), \
                    mock.patch.object(host.subprocess, 'run', return_value=subprocess.CompletedProcess([], rc, data)) as run:
                self.assertIs(host.container_token_absent(record), expected)
                argv = run.call_args.args[0]
                self.assertIn('--all', argv)
                self.assertEqual(argv[argv.index('--filter') + 1], 'label=io.oxpinyin.memory-reservation=owner')
        record['kind'] = 'container'
        for labels, expected in (({}, True), ({'io.oxpinyin.memory-reservation': 'other'}, True),
                                  ({'io.oxpinyin.memory-reservation': 'owner'}, False), (None, None)):
            rows = [dict(configuration=dict(labels=labels))]
            with self.subTest(labels=labels), \
                    mock.patch.object(host.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, json.dumps(rows))):
                self.assertIs(host.container_token_absent(record), expected)

    def test_created_container_is_kept_under_guard_then_removed_before_budget_release(self):
        runtime = self.fake_runtime()
        for settled in (False, True):
            with self.subTest(settled=settled):
                reservation = self.reservation()
                reservation.record['runtime'] = str(runtime)
                with slot.admission_lock(self.directory):
                    slot.write_container_record(reservation.path, reservation.record)
                subprocess.run([str(runtime), 'create', '--name', reservation.record['name'],
                                '--memory', '4096M'], env=self.env, check=True)
                if settled:
                    reservation.created()
                with slot.admission_lock(self.directory):
                    self.assertEqual(slot.container_reservations(self.directory, out=self.out), 4096)
                self.assertTrue((self.directory / 'runtime-state').exists())
                reservation.close()
                with slot.admission_lock(self.directory):
                    self.assertEqual(slot.container_reservations(self.directory, out=self.out), 0)
                self.assertFalse((self.directory / 'runtime-state').exists())
                self.assertFalse(reservation.path.exists())

    def test_created_container_removal_failure_retains_reservation(self):
        reservation = self.reservation()
        reservation.created()
        reservation.close()
        with mock.patch.object(host, 'container_liveness', return_value='created'), \
                mock.patch.object(host.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1)):
            with slot.admission_lock(self.directory):
                self.assertEqual(slot.container_reservations(self.directory, out=self.out), 4096)
        self.assertTrue(reservation.path.exists())

    def test_created_container_with_mismatched_label_is_not_removed(self):
        reservation = self.reservation()
        reservation.created()
        reservation.close()
        row = dict(Config=dict(Labels={'io.oxpinyin.memory-reservation': 'another-owner'}),
                   State=dict(Status='created'))
        with mock.patch.object(host, 'inspect_container', return_value=row), \
                mock.patch.object(host.subprocess, 'run') as runtime:
            with slot.admission_lock(self.directory):
                self.assertEqual(slot.container_reservations(self.directory, out=self.out), 4096)
            test_support.assert_not_called(self, runtime)
        self.assertTrue(reservation.path.exists())

    def test_inspect_error_is_unknown_until_successful_listing_proves_absence(self):
        record = dict(runtime='/usr/bin/docker', kind='docker', name='test', token='owner')
        for rc, output, expected in ((1, '', 'unknown'), (0, '{broken', 'unknown'),
                                      (0, '{"Names":"test"}', 'unknown'),
                                      (0, '{"Names":"other"}', 'gone'), (0, '', 'gone')):
            with self.subTest(output=output, rc=rc), \
                    mock.patch.object(host, 'inspect_container', return_value=None), \
                    mock.patch.object(host.subprocess, 'run', return_value=subprocess.CompletedProcess([], rc, output)):
                self.assertEqual(host.container_liveness(record), expected)

    def test_inner_kernel_limit_gate_refuses_unlimited_runtime_before_payload(self):
        maximum = self.directory / 'memory.max'
        gate = memory.LIMIT_GATE.replace('/sys/fs/cgroup/memory.max', str(maximum))
        gate = gate.replace('/sys/fs/cgroup/memory/memory.limit_in_bytes', str(self.directory / 'missing'))
        for value, expected in (('max', 1), ('0', 1), ('2048', 1), ('1024', 0), ('512', 0)):
            maximum.write_text(value + '\n')
            result = subprocess.run(['sh', '-c', gate, '--', '1024', 'printf', 'payload'],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, expected)
            self.assertEqual(result.stdout, 'payload' if expected == 0 else '')

    def fake_runtime(self):
        runtime = self.directory / 'docker'
        runtime.write_text(f'#!{sys.executable}\n' + r'''
import fcntl, hashlib, io, json, os, runpy, sys, tarfile, time
from pathlib import Path
root=Path(os.environ['OXPINYIN_LOCK_DIR'])
path=root/'runtime-state'
# Invocations overlap: a start polls the state while a stop rewrites it, and
# the launcher inspects it meanwhile. Every write replaces the file atomically
# and every read-modify-write holds one lock, so no reader sees a truncated
# file and no update is lost (#1029).
class locked:
    def __enter__(self): self.file=(root/'runtime-lock').open('a'); fcntl.flock(self.file, fcntl.LOCK_EX); return self
    def __exit__(self, *exc): self.file.close()
def write(target, text):
    temporary=target.with_name(f'{target.name}.{os.getpid()}.tmp'); temporary.write_text(text); os.replace(temporary, target)
def read_state():
    with locked(): return json.loads(path.read_text())
def update_state(change):
    with locked():
        row=json.loads(path.read_text()); change(row); write(path, json.dumps(row)); return row
def stopped(row): row['State']['Status']='exited'; row['status']['state']='stopped'; row['status']['networks']=[]
args=sys.argv[1:]
action=args[0]
if action=='--version': print('synthetic runtime 1')
elif action=='image':
    if args[1]=='list': print(os.environ.get('FAKE_COMPAT_IMAGE',''))
elif action in ('info','volume'): pass
elif action=='create':
    for argument in args:
        if ':/input:ro' in argument:
            write(root/'fixture-input', argument.split(':/input:ro')[0])
    records=list(root.glob('container-memory-*.json'))
    assert len(records)==1, 'create without reservation'
    record=json.loads(records[0].read_text())
    assert record['name']==args[args.index('--name')+1], 'wrong container reservation'
    assert args[args.index('--memory')+1]==str(record['memory_mib'])+'M'
    if os.environ.get('CREATE_FAIL'): sys.exit(125)
    row=dict(Name='/'+record['name'], Config=dict(Labels={'io.oxpinyin.memory-reservation':record['token']}),
             HostConfig=dict(Memory=record['memory_mib']*1024**2, MemorySwap=record['memory_mib']*1024**2),
             State=dict(Status='created'), status=dict(networks=[], state='created'),
             configuration=dict(labels={'io.oxpinyin.memory-reservation':record['token']},
                                resources=dict(memoryInBytes=record['memory_mib']*1024**2)))
    events=root/'created-names'
    with locked():
        names=json.loads(events.read_text()) if events.exists() else []
        names.append(dict(name=record['name'], token=record['token']))
        write(events, json.dumps(names))
    (root/'create-entered').touch()
    while os.environ.get('BLOCK_CREATE') and not (root/'finish-create').exists(): time.sleep(.01)
    with locked(): write(path, json.dumps(row))
    if os.environ.get('CREATE_FAIL_AFTER'): sys.exit(125)
elif action=='inspect':
    with locked():
        if not path.exists(): sys.exit(1)
        row=json.loads(path.read_text())
    if os.environ.get('UNKNOWN_EXIT') and row['State']['Status']=='exited':
        row['status']['state']='unknown'; row['State']['Status']='unknown'
    print(json.dumps([row]))
elif action=='ps':
    with locked():
        if path.exists(): print(json.dumps(dict(Names=json.loads(path.read_text())['Name'][1:])))
elif action=='start':
    def started(row):
        row['State']['Status']='running'; row['status']['state']='running'
        row['status']['startedDate']='2026-10-20T15:40:28Z'; row['status']['networks']=[dict(network='default')]
    row=update_state(started)
    (root/'started').touch()
    if row['Name'].endswith('-results'):
        directory=Path((root/'fixture-input').read_text())
        data=(directory/'identity.json').read_bytes()
        provenance=dict(schema=1, environment='container', inputs=json.loads(data),
                        input_sha256=hashlib.sha256(data).hexdigest(), artifacts=None,
                        artifacts_sha256=hashlib.sha256(b'null').hexdigest(),
                        files={'results.json':hashlib.sha256(b'[]').hexdigest()})
        if not os.environ.get('OMIT_STATE'):
            # The shipped verifier's own state table, as the guest's seal-results records it.
            state_paths=runpy.run_path(str(directory/'identity.py'))['state_paths']
            provenance['state']=dict(state_paths(provenance['inputs']),
                                     executable='/opt/uv/python/cpython-3.15.0-linux-x86_64-gnu/bin/python3.15')
        with tarfile.open(fileobj=sys.stdout.buffer, mode='w|') as output:
            for filename, content in [('results.json', b'[]'), ('provenance.json', json.dumps(provenance).encode())]:
                member=tarfile.TarInfo(filename); member.size=len(content)
                output.addfile(member, io.BytesIO(content))
        update_state(stopped)
    else:
        print('payload-ready',flush=True)
    while read_state()['State']['Status']=='running': time.sleep(.01)
    sys.exit(int(os.environ.get('PAYLOAD_STATUS','0')))
elif action=='stop':
    assert list(root.glob('container-memory-*.json')), 'release before stop'
    update_state(stopped)
    (root/'stop-confirmed').touch()
elif action=='rm':
    with locked(): path.unlink(missing_ok=True)
''')
        runtime.chmod(0o755)
        return runtime

    def launcher(self, runtime, **environment):
        code = ("from tools.dev import host\n"
                "host.resource_snapshot=lambda: dict(available_mib=16000, cpus=1, cargo_cpus=1, notes=[])\n"
                "from tools.dev.compat.memory import main\n"
                f"raise SystemExit(main(['--runtime', {str(runtime)!r}, '--name','test','--memory-mib','512', '--', 'image']))")
        process = subprocess.Popen([sys.executable, '-c', code], cwd=ROOT,
                                   env=dict(self.env, **environment),
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.reap, process)
        return process

    @staticmethod
    def reap(process):
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=10)

    def await_path(self, path):
        deadline = time.monotonic() + 5
        while not path.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertTrue(path.exists(), str(path))

    def test_sigkill_during_create_keeps_fence_then_recovers_observed_created_container(self):
        runtime = self.fake_runtime()
        process = self.launcher(runtime, BLOCK_CREATE='1')
        self.await_path(self.directory / 'create-entered')
        process.kill()
        process.wait(timeout=5)
        with slot.admission_lock(self.directory):
            self.assertEqual(slot.container_reservations(self.directory, out=self.out), 512)
        self.assertFalse((self.directory / 'runtime-state').exists())
        (self.directory / 'finish-create').touch()
        self.await_path(self.directory / 'runtime-state')
        # A matching created object settles creation once its client exits.
        deadline = time.monotonic() + 5
        while list(self.directory.glob('container-memory-*.json')) and time.monotonic() < deadline:
            with slot.admission_lock(self.directory):
                slot.container_reservations(self.directory, out=self.out)
            time.sleep(0.01)
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))
        self.assertFalse((self.directory / 'runtime-state').exists())

    def test_sigkill_attached_launcher_retains_budget_until_container_exit(self):
        runtime = self.fake_runtime()
        process = self.launcher(runtime)
        self.await_path(self.directory / 'started')
        process.kill()
        process.wait(timeout=5)
        with slot.admission_lock(self.directory):
            self.assertEqual(slot.container_reservations(self.directory, out=self.out), 512)
        record = json.loads(next(self.directory.glob('container-memory-*.json')).read_text())
        self.assertEqual(host.container_liveness(record), 'live')
        subprocess.run([str(runtime), 'stop', 'test'], check=True, env=self.env)
        deadline = time.monotonic() + 5
        while list(self.directory.glob('container-memory-*.json')) and time.monotonic() < deadline:
            with slot.admission_lock(self.directory):
                slot.container_reservations(self.directory, out=self.out)
            time.sleep(0.01)
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def test_real_signal_cancels_attached_client_stops_waits_then_releases(self):
        runtime = self.fake_runtime()
        process = self.launcher(runtime)
        self.await_path(self.directory / 'started')
        process.send_signal(signal.SIGTERM)
        _, error = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 130, error)
        self.assertTrue((self.directory / 'stop-confirmed').exists())
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def test_payload_statuses_zero_one_and_77_propagate(self):
        runtime = self.fake_runtime()
        for expected in (0, 1, 77):
            with self.subTest(expected=expected):
                (self.directory / 'started').unlink(missing_ok=True)
                process = self.launcher(runtime, PAYLOAD_STATUS=str(expected))
                self.await_path(self.directory / 'started')
                subprocess.run([str(runtime), 'stop', 'test'], check=True, env=self.env)
                _, error = process.communicate(timeout=5)
                self.assertEqual(process.returncode, expected, error)
                self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def test_fake_runtime_state_is_never_read_half_written(self):
        """Readers overlap the fake's writes: a start polls the state while a stop
        rewrites it (#1029). A reader must see the old or the new state, never a
        truncated file."""
        runtime = self.fake_runtime()
        (self.directory / 'container-memory-test.json').write_text(
            json.dumps(dict(name='test', memory_mib=512, token='test')))
        subprocess.run([str(runtime), 'create', '--name', 'test', '--memory', '512M', 'image'],
                       check=True, env=self.env)
        ready, finished = self.directory / 'reader-ready', self.directory / 'reader-finished'
        reader = subprocess.Popen([sys.executable, '-c', (
            'import json, sys\nfrom pathlib import Path\n'
            'state, ready, finished = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])\n'
            'reads = errors = 0\n'
            'ready.touch()\n'
            'while not finished.exists():\n'
            '    reads += 1\n'
            '    try: json.loads(state.read_text())\n'
            '    except ValueError: errors += 1\n'
            'print(reads, errors)'), str(self.directory / 'runtime-state'), str(ready), str(finished)],
            stdout=subprocess.PIPE, text=True)
        self.addCleanup(self.reap, reader)
        try:
            # The writes start only once the reader is in its loop; a loaded host
            # could otherwise run all forty before the reader is scheduled.
            deadline = time.monotonic() + 10
            while not ready.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(ready.exists(), 'the state reader did not start within 10 s')
            for _ in range(40):
                subprocess.run([str(runtime), 'stop', 'test'], check=True, env=self.env)
        finally:
            finished.touch()
        reads, errors = map(int, reader.communicate(timeout=10)[0].split())
        self.assertGreater(reads, 0)
        self.assertEqual(errors, 0, f'{errors} of {reads} reads saw a half-written state after 40 writes')

    def test_compat_shell_cancellation_forwards_and_waits_before_exit(self):
        runtime = self.fake_runtime()
        env = self.synthetic_uv_env()
        process = subprocess.Popen(['bash', 'tools/oracle/compat.sh', '--runtime', str(runtime),
                                    '--memory-mib', '512', '--memory-smoke'], cwd=ROOT, env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.reap, process)
        self.await_path(self.directory / 'started')
        process.send_signal(signal.SIGTERM)
        _, error = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 130, error)
        self.assertTrue((self.directory / 'stop-confirmed').exists())
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def synthetic_uv_env(self):
        # Keep the entire test synthetic, including host resource readings.
        uv = self.directory / 'uv'
        uv.write_text(f'#!{sys.executable}\n' + """import sys
from tools.dev import host
host.resource_snapshot=lambda: dict(available_mib=16000, cpus=1, cargo_cpus=1, notes=[])
import importlib
module = sys.argv[sys.argv.index('-m')+1]
assert module in ('tools.dev.compat.memory', 'tools.dev.compat.boundary', 'tools.dev.compat.identity')
raise SystemExit(importlib.import_module(module).main(sys.argv[sys.argv.index('-m')+2:]))
""")
        uv.chmod(0o755)
        # This executable lives outside ROOT, as real uv does.
        uv.write_text(uv.read_text().replace('import sys\n', f'import sys\nsys.path.insert(0,{str(ROOT)!r})\n', 1))
        env = dict(self.env, PATH=self.temp.name + os.pathsep + os.environ['PATH'])
        # The launcher deliberately rejects host state variables. A runner may
        # export UV_CACHE_DIR (GitHub-hosted images do), so keep this fixture
        # self-contained instead of inheriting the caller's Python state.
        for name in tuple(env):
            if name in boundary.STATE_SETTINGS or (
                    name.startswith(('OXPINYIN_', 'PINYIN_', 'ORACLE_', 'UV_PYTHON_'))
                    and name not in boundary.HOST_SETTINGS):
                env.pop(name)
        return env

    def reader_fixture(self, unknown_exit=False, omit_state=False):
        runtime = self.fake_runtime()
        apple = self.directory / 'container'
        apple.symlink_to(runtime)
        fixture = self.directory / 'source'
        paths = ('tools/oracle/compat.sh', 'tools/container/runtime.sh', 'tools/dev/compat/identity.py')
        for relative in paths:
            destination = fixture / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        (fixture / 'tools/oracle/compat-inside.sh').write_text('# synthetic input; never executed\n')
        (fixture / 'tools/oracle/image-tag.sh').write_text('printf test-reference\n')
        (fixture / 'tools/oracle/Dockerfile.compat').write_text('synthetic image recipe\n')
        (fixture / 'rust-toolchain.toml').write_text('synthetic toolchain identity\n')
        key = hashlib.sha256(b'synthetic image recipe\nsynthetic toolchain identity\ntest-reference:no-custom-ca').hexdigest()[:16]
        git = self.directory / 'git'
        git.write_text('''#!/bin/sh
case $1 in
ls-files) printf 'tools/oracle/compat-inside.sh\\0tools/dev/compat/identity.py\\0' ;;
rev-parse) printf '0000000000000000000000000000000000000000\\n' ;;
status) : ;;
*) exit 1 ;;
esac
''')
        git.chmod(0o755)
        env = dict(self.synthetic_uv_env(), FAKE_COMPAT_IMAGE='oxpinyin-compat:' + key)
        env.pop('OXPINYIN_PROXY_CA', None)
        if unknown_exit:
            env['UNKNOWN_EXIT'] = '1'
        if omit_state:
            env['OMIT_STATE'] = '1'
        process = subprocess.Popen(['bash', 'tools/oracle/compat.sh', '--runtime', str(apple),
                                    '--memory-mib', '512', '--runners', 'pc-version'],
                                   cwd=fixture, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.reap, process)
        return process, runtime, fixture

    def test_apple_reader_starts_only_after_confirmed_main_exit_with_its_own_reservation(self):
        process, runtime, fixture = self.reader_fixture()
        self.await_path(self.directory / 'started')
        records = list(self.directory.glob('container-memory-*.json'))
        self.assertEqual(len(records), 1)
        record = json.loads(records[0].read_text())
        subprocess.run([str(runtime), 'stop', record['name']], check=True, env=self.env)
        _, error = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, error)
        names = json.loads((self.directory / 'created-names').read_text())
        self.assertEqual([r['name'] for r in names], [record['name'], record['name'] + '-results'])
        self.assertNotEqual(names[0]['token'], names[1]['token'])
        # The fake runtime refuses every create without exactly one matching
        # full-limit reservation: main and reader cannot overlap here.
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def test_apple_reader_export_without_state_fails_verification(self):
        process, runtime, _ = self.reader_fixture(omit_state=True)
        self.await_path(self.directory / 'started')
        record = json.loads(next(self.directory.glob('container-memory-*.json')).read_text())
        subprocess.run([str(runtime), 'stop', record['name']], check=True, env=self.env)
        _, error = process.communicate(timeout=10)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn('exported container state mismatch', error)
        self.assertIn('container result identity verification failed', error)
        self.assertFalse(list(self.directory.glob('container-memory-*.json')))

    def test_apple_reader_does_not_start_when_main_exit_is_unknown(self):
        process, runtime, _ = self.reader_fixture(unknown_exit=True)
        self.await_path(self.directory / 'started')
        record = json.loads(next(self.directory.glob('container-memory-*.json')).read_text())
        subprocess.run([str(runtime), 'stop', record['name']], check=True, env=self.env)
        _, error = process.communicate(timeout=10)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn('main container exit unconfirmed; no result reader started', error)
        names = json.loads((self.directory / 'created-names').read_text())
        self.assertEqual(len(names), 1)
        self.assertEqual(len(list(self.directory.glob('container-memory-*.json'))), 1)

    def test_all_runtime_starts_are_inside_the_single_reserved_launch_function(self):
        shells = ('tools/oracle/compat.sh', 'tools/oracle/compat-inside.sh', 'tools/container/runtime.sh')
        direct = re.compile(r'(?:["\']?\$(?:runtime|ctr)["\']?|\b(?:docker|podman|container))\s+(run|start|create|exec)\b')
        for relative in shells:
            source = (ROOT / relative).read_text()
            self.assertIsNone(direct.search(source), relative)
        source = (ROOT / 'tools/oracle/compat.sh').read_text()
        sites = re.findall(r'^\s*\(?reserved_run\s+([^\n]+)', source, re.MULTILINE)
        self.assertEqual(len(sites), 3, sites)  # main, Apple reader, memory smoke
        self.assertIn('-m tools.dev.compat.memory', source)
        tree = ast.parse((ROOT / 'tools/dev/compat/memory.py').read_text())
        starts = []
        for function in (node for node in tree.body if isinstance(node, ast.FunctionDef)):
            for node in ast.walk(function):
                if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == 'call':
                    if len(node.args) >= 2 and isinstance(node.args[1], ast.List):
                        command = node.args[1].elts
                        if command and isinstance(command[0], ast.Constant) and command[0].value in ('run', 'start'):
                            starts.append(function.name)
                if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == 'Popen':
                    self.assertEqual(function.name, 'call')
        self.assertEqual(starts, ['run'])
        # compat-inside passes --in-container. Its provisioning helper returns
        # before the native-only in_image runtime command in that mode.
        setup = (ROOT / 'tools/cloud/setup.sh').read_text()
        in_image = setup[setup.index('in_image() {'):]
        self.assertLess(in_image.index('if ((inside)); then'), in_image.index('"$ctr" run'))
        self.assertIn('return; fi', in_image[:in_image.index('"$ctr" run')])

    def test_runtime_preference_and_explicit_override_are_preserved(self):
        self.fake_runtime()
        for kind in ('container', 'podman'):
            (self.directory / kind).symlink_to(self.directory / 'docker')
        uname = self.directory / 'uname'
        uname.write_text('#!/bin/sh\nprintf "%s\\n" "$SYNTHETIC_HOST"\n')
        uname.chmod(0o755)
        for platform, override, expected in (('Darwin', '', 'container'), ('Linux', '', 'podman'),
                                             ('Darwin', 'docker', 'docker')):
            env = dict(self.env, SYNTHETIC_HOST=platform, OXPINYIN_CONTAINER=override,
                       PATH=self.temp.name + os.pathsep + os.environ['PATH'])
            result = subprocess.run(['bash', '-c', 'source tools/container/runtime.sh; oxpinyin_select_runtime; printf "selected:%s\\n" "$ctr"'],
                                    cwd=ROOT, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('selected:' + expected, result.stdout)


if __name__ == '__main__':
    unittest.main()
