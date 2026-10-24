"""W8 launch setup must guard the subject before a profiler sees the worker."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name('run-w8-cycle.sh')


class LaunchTests(unittest.TestCase):
    def launch_setup(self, status=0):
        source = SCRIPT.read_text()
        # Execute the runner's real command setup, without its unrelated
        # installation/model preparation or capture-producing profiler branches.
        setup = source[source.index('run_pinned() {'):source.index('TIMING_JSON=')]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            environment = dict(os.environ, PROFILE_DIR=str(root), HOST_OS='Linux',
                               CAPI_SO=str(root / 'subject.so'),
                               CAPI_DATA=str(root / 'data'),
                               TKRZW_RUNTIME=str(SCRIPT.parent.parent / 'tkrzw/runtime.sh'))
            command = ('set -eu\n'
                       'tkrzw_run() { printf "GUARD %s %s %s\\n" "$1" "$2" "$3"; '
                       f'return {status}; }}\n' + setup +
                       '\nprintf "PROFILER\\n"; printf "%s\\n" "${CYCLE_CMD[@]}"\n')
            result = subprocess.run(['bash', '-c', command], env=environment,
                                    capture_output=True, text=True)
            return result, root

    def test_profiler_receives_worker_after_subject_preflight(self):
        result, root = self.launch_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(),
                         [f'GUARD auto subject {root / "subject.so"}', 'PROFILER',
                          str(root / 'bisect'), '--perf', str(root / 'subject.so'),
                          str(root / 'data')])

    def test_refusal_prevents_profiler_start(self):
        result, _ = self.launch_setup(23)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertNotIn('PROFILER', result.stdout)

    def check_darwin(self, *, path=None, perf_cpu=None):
        environment = dict(os.environ, OXPINYIN_PROFILE_OS='Darwin')
        environment.pop('PERF_CPU', None)
        if perf_cpu is not None:
            environment['PERF_CPU'] = perf_cpu
        if path is not None:
            environment['PATH'] = path
        return subprocess.run(['bash', str(SCRIPT), '--check'], env=environment,
                              capture_output=True, text=True)

    def test_darwin_refuses_perf_cpu_with_the_affinity_reason(self):
        result = self.check_darwin(perf_cpu='0')
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn('PERF_CPU is unsupported on macOS', result.stderr)
        self.assertIn('Mach affinity is a cache hint, not CPU pinning', result.stderr)

    def test_darwin_refuses_missing_samply(self):
        result = self.check_darwin(path='/usr/bin:/bin')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('samply is required on macOS', result.stderr)

    def test_darwin_check_names_the_linux_only_parts(self):
        with tempfile.TemporaryDirectory() as directory:
            samply = Path(directory) / 'samply'
            samply.write_text('#!/bin/sh\nexit 0\n')
            samply.chmod(0o755)
            result = self.check_darwin(path=f"{directory}:{os.environ['PATH']}")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('profile platform: Darwin; profiler=samply', result.stdout)
        self.assertIn('perf record', result.stderr)
        self.assertIn('perf_event_paranoid', result.stderr)
        self.assertIn('taskset and PERF_CPU', result.stderr)

    def test_darwin_refuses_missing_cargo_c_before_reading_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            bin_dir = Path(directory)
            for name in ('bash', 'uname', 'grep', 'sed', 'awk', 'readlink', 'pkg-config'):
                source = shutil.which(name)
                if source:
                    (bin_dir / name).symlink_to(source)
            samply = bin_dir / 'samply'
            samply.write_text('#!/bin/sh\nexit 0\n')
            samply.chmod(0o755)
            environment = dict(os.environ, OXPINYIN_PROFILE_OS='Darwin',
                               PATH=f'{directory}:/usr/bin:/bin')
            environment.pop('PERF_CPU', None)
            environment.pop('PINYIN_EXPORT_DIR', None)
            result = subprocess.run(['/bin/bash', str(SCRIPT)], env=environment,
                                    capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('cargo-c is required on macOS', result.stderr)


if __name__ == '__main__':
    unittest.main()
