#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Unit tests for tools.dev.host."""
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host

class HostTests(unittest.TestCase):
    def test_group_liveness_uses_injected_linux_states_and_group_ids(self):
        with tempfile.TemporaryDirectory() as scratch:
            proc = Path(scratch)
            for pid, group, state in ((10, 10, 'Z'), (11, 10, 'S'), (12, 12, 'R')):
                directory = proc / str(pid)
                directory.mkdir()
                (directory / 'stat').write_text(f'{pid} (name (nested)) {state} 1 {group} 0\n')
            self.assertTrue(host.process_group_live(10, system='linux', proc=proc))
            (proc / '11/stat').write_text('11 (name) X 1 10 0\n')
            self.assertFalse(host.process_group_live(10, system='linux', proc=proc))
            self.assertFalse(host.process_group_live(999, system='linux', proc=proc))
            (proc / '11/stat').write_text('malformed\n')
            self.assertIsNone(host.process_group_live(10, system='linux', proc=proc))

    def test_group_liveness_uses_injected_darwin_rows_on_every_platform(self):
        self.assertTrue(host.process_group_live(10, system='darwin', ps_output='10 Z\n10 S+\n11 R\n'))
        self.assertFalse(host.process_group_live(10, system='darwin', ps_output='10 Z+\n11 R\n'))
        self.assertIsNone(host.process_group_live(10, system='darwin', ps_output='malformed\n'))
        self.assertIsNone(host.process_group_live(10, system='unavailable'))

    def test_import_does_not_modify_os(self):
        for remove_affinity in (False, True):
            with self.subTest(remove_affinity=remove_affinity):
                subprocess.run([sys.executable, '-c',
                    'import os\n'
                    f'if {remove_affinity!r}:\n'
                    '    vars(os).pop("sched_getaffinity", None)\n'
                    'before = vars(os).copy()\n'
                    'from tools.dev import host\n'
                    'assert vars(os) == before\n'], cwd=ROOT, check=True)

    def test_allowed_cpus_reads_affinity(self):
        affinity = mock.Mock(return_value={2, 5})
        with mock.patch.object(host, 'os', SimpleNamespace(sched_getaffinity=affinity)):
            self.assertEqual(host.allowed_cpus(), {2, 5})
        affinity.assert_called_once_with(0)

    def test_allowed_cpus_without_affinity(self):
        for count, expected in ((4, {0, 1, 2, 3}), (None, {0})):
            with self.subTest(count=count), mock.patch.object(
                    host, 'os', SimpleNamespace(cpu_count=lambda: count)):
                self.assertEqual(host.allowed_cpus(), expected)


class CpuSnapshotTests(unittest.TestCase):
    def test_snapshot_reports_cpus_and_notes(self):
        snapshot = host.cpu_snapshot()
        self.assertIsInstance(snapshot['cpus'], int)
        self.assertGreaterEqual(snapshot['cpus'], 1)
        self.assertGreaterEqual(snapshot['cargo_cpus'], snapshot['cpus'])
        self.assertIsInstance(snapshot['notes'], list)
        self.assertNotIn('available_mib', snapshot)

    def test_without_affinity_or_cgroups_the_cpu_count_is_used(self):
        with mock.patch.object(sys, 'platform', 'darwin'), \
                mock.patch.object(host.os, 'cpu_count', return_value=6):
            snapshot = host.cpu_snapshot()
        self.assertEqual((snapshot['cpus'], snapshot['cargo_cpus']), (6, 6))
        self.assertIn('CPU count=6', snapshot['notes'])
        with mock.patch.object(sys, 'platform', 'darwin'), \
                mock.patch.object(host.os, 'cpu_count', return_value=None):
            self.assertEqual(host.cpu_snapshot()['cpus'], 1)


class CgroupCpuTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.proc = self.root / 'proc'
        self.unified = self.root / 'sys/fs/cgroup/unified'
        # Deliberately outside the default /sys/fs/cgroup/cpu location.
        self.cpu = self.root / 'controllers/cpu,cpuacct'
        (self.proc / 'self').mkdir(parents=True)
        (self.proc / 'self/cgroup').write_text('9:name=systemd:/unrelated\n7:cpuacct,cpu:/session\n')
        self.mounts = [self.mount(self.cpu, 'cpu,cpuacct')]
        (self.proc / 'self/mountinfo').write_text(''.join(self.mounts))
        for directory in (self.cpu, self.cpu / 'session'):
            directory.mkdir(parents=True)
            (directory / 'cpu.cfs_quota_us').write_text('-1')
            (directory / 'cpu.cfs_period_us').write_text('100000')

    def mount(self, directory, controllers, root='/'):
        escaped = str(directory).replace('\\', r'\134').replace(' ', r'\040')
        return f'30 20 0:27 {root} {escaped} rw,nosuid - cgroup cgroup rw,{controllers}\n'

    def snapshot(self):
        with mock.patch.object(sys, 'platform', 'linux'), \
                mock.patch.object(host, 'allowed_cpus', return_value={0, 1, 2, 3}):
            return host.cpu_snapshot(self.proc, self.unified)

    def test_cpu_quota_combines_with_affinity_and_ancestors(self):
        for quota, cpus, cargo_cpus in ((200000, 2, 2), (250000, 2, 3), (50000, 1, 1),
                                       (800000, 4, 4), (-1, 4, 4)):
            with self.subTest(quota=quota):
                (self.cpu / 'session/cpu.cfs_quota_us').write_text(str(quota))
                snapshot = self.snapshot()
                self.assertEqual((snapshot['cpus'], snapshot['cargo_cpus']), (cpus, cargo_cpus))
        (self.cpu / 'cpu.cfs_quota_us').write_text('100000')
        self.assertEqual(self.snapshot()['cpus'], 1)

    def test_hybrid_empty_unified_hierarchy_adds_no_capacity(self):
        self.unified.mkdir(parents=True)
        (self.unified / 'cgroup.controllers').write_text('')
        (self.proc / 'self/cgroup').write_text('0::/\n7:cpuacct,cpu:/session\n')
        with (self.proc / 'self/mountinfo').open('a') as stream:
            stream.write(f'40 20 0:30 / {self.unified} rw - cgroup2 cgroup rw\n')
        self.assertEqual(self.snapshot()['cpus'], 4)

    def test_unified_cpu_max_limits_the_cpus(self):
        self.unified.mkdir(parents=True)
        (self.proc / 'self/cgroup').write_text('0::/\n')
        (self.unified / 'cpu.max').write_text('200000 100000')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['cpus'], 2)
        self.assertIn('cgroup / CPU quota=200000/100000', snapshot['notes'])

    def test_unified_cpu_max_without_a_limit_or_unreadable_adds_no_capacity(self):
        self.unified.mkdir(parents=True)
        (self.proc / 'self/cgroup').write_text('0::/\n')
        (self.unified / 'cpu.max').write_text('max 100000')
        self.assertEqual(self.snapshot()['cpus'], 4)
        (self.unified / 'cpu.max').write_text('bad')
        snapshot = self.snapshot()
        self.assertEqual(snapshot['cpus'], 4)
        self.assertIn('cgroup / CPU quota=unknown', snapshot['notes'])

    def test_mount_root_limits_the_visible_ancestor_walk(self):
        (self.proc / 'self/cgroup').write_text('7:cpuacct,cpu:/tenant/session\n')
        (self.proc / 'self/mountinfo').write_text(self.mount(self.cpu, 'cpu,cpuacct', '/tenant'))
        directories = host.linux_cgroup_dirs(self.proc, controller='cpu')
        self.assertEqual(directories, [self.cpu / 'session', self.cpu])
        (self.proc / 'self/cgroup').write_text('7:cpuacct,cpu:/tenant\n')
        self.assertEqual(host.linux_cgroup_dirs(self.proc, controller='cpu'), [self.cpu])

    def test_inaccessible_or_invalid_mounts_remain_unknown(self):
        for entry, mountinfo in (('/outside', self.mount(self.cpu, 'cpu,cpuacct', '/tenant')),
                                  ('/tenant/../session', self.mounts[0]),
                                  ('/session', ''), ('/session', 'invalid')):
            with self.subTest(entry=entry, mountinfo=mountinfo):
                (self.proc / 'self/cgroup').write_text(f'7:cpuacct,cpu:{entry}\n')
                (self.proc / 'self/mountinfo').write_text(mountinfo)
                snapshot = self.snapshot()
                self.assertIn('cgroup v1 cpu limits=unknown', snapshot['notes'])
                self.assertEqual(snapshot['cpus'], 4)

    def test_unreadable_cpu_files_are_unknown(self):
        (self.cpu / 'session/cpu.cfs_quota_us').write_text('200000')
        for filename in ('cpu.cfs_quota_us', 'cpu.cfs_period_us'):
            with self.subTest(filename=filename):
                path = self.cpu / 'session' / filename
                content = path.read_text()
                path.unlink()
                snapshot = self.snapshot()
                self.assertEqual(snapshot['cpus'], 4)
                self.assertIn('cgroup v1 /session CPU quota=unknown', snapshot['notes'])
                path.write_text(content)


if __name__ == '__main__':
    unittest.main()
