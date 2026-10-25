"""W8 launch setup must guard the subject before a profiler sees the worker.

A samply record then reports the profile's sample total and warns when it is
too short to rank hot stacks.
"""

import gzip
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name('run-w8-cycle.sh')
EXTRACTOR = SCRIPT.with_name('extract-hot-stacks.py')


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

    def check_darwin(self, *, path=None, perf_cpu=None, cycles=None):
        environment = dict(os.environ, OXPINYIN_PROFILE_OS='Darwin')
        environment.pop('PERF_CPU', None)
        environment.pop('PERF_CYCLES', None)
        if perf_cpu is not None:
            environment['PERF_CPU'] = perf_cpu
        if cycles is not None:
            environment['PERF_CYCLES'] = cycles
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

    def check_darwin_with_samply(self, body, **options):
        with tempfile.TemporaryDirectory() as directory:
            samply = Path(directory) / 'samply'
            samply.write_text('#!/bin/sh\n' + body + '\n')
            samply.chmod(0o755)
            return self.check_darwin(path=f"{directory}:{os.environ['PATH']}",
                                     **options)

    def test_check_reports_the_samply_default_cycle_count(self):
        result = self.check_darwin_with_samply('exit 0')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'samply cycles: {SAMPLY_CYCLES}\n', result.stdout)

    def test_check_reports_a_cycle_count_override(self):
        result = self.check_darwin_with_samply('exit 0', cycles='7')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('samply cycles: 7\n', result.stdout)

    def test_check_reports_presymbolicate_availability(self):
        result = self.check_darwin_with_samply(
            'printf "%s\\n" --unstable-presymbolicate')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('samply presymbolicate: available', result.stdout)

    def test_check_reports_missing_presymbolicate(self):
        result = self.check_darwin_with_samply('exit 0')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('samply presymbolicate: unavailable', result.stdout)
        self.assertIn('samply has no --unstable-presymbolicate', result.stderr)

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



SAMPLY_CYCLES = '2048'
FALLBACK_CYCLES = '32'


def default_level():
    """The level extract-hot-stacks.py applies without --min-samples."""
    spec = importlib.util.spec_from_file_location('extract_hot_stacks', EXTRACTOR)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.MIN_SAMPLES


def profile_gz(samples):
    """A gzipped profile holding `samples` samples on one named frame."""
    profile = {'threads': [{
        'stringArray': ['hot'],
        'samples': {'stack': [0] * samples},
        'stackTable': {'prefix': [None], 'frame': [0]},
        'frameTable': {'func': [0]},
        'funcTable': {'name': [0]},
    }]}
    return gzip.compress(json.dumps(profile).encode())


# Stands in for `samply record --save-only -o OUT -- CMD...`: it saves the
# profile the test prepared at OUT, runs CMD and exits with the chosen status.
FAKE_SAMPLY = '''#!/bin/sh
out=
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        --) shift; break ;;
        *) shift ;;
    esac
done
[ -z "${FAKE_PROFILE:-}" ] || cp "$FAKE_PROFILE" "$out"
"$@"
exit "${FAKE_SAMPLY_STATUS:-0}"
'''

# Stands in for valgrind: it only reports the cycle count the worker would get.
FAKE_VALGRIND = '#!/bin/sh\necho "valgrind PERF_CYCLES=$PERF_CYCLES"\n'

# The worker: reports the cycle count it was given instead of profiling.
CYCLE_CMD = "CYCLE_CMD=(sh -c 'echo \"worker PERF_CYCLES=$PERF_CYCLES\"')\n"


def between(source, start, end):
    """The stretch of the launcher from one marker up to, not including, another."""
    return source[source.index(start):source.index(end)]


def run_launcher_stretch(tools, branch, *, perf_cycles=None, **variables):
    """Run the launcher's cycle counts and `branch` against the fake `tools`.

    `tools` maps a command name to the script that stands in for it. The cycle
    defaults and the export are the launcher's own lines, so the worker sees
    the PERF_CYCLES it would get in a real run. Return the finished process
    and the text of the timing file the branch wrote, if any.
    """
    source = SCRIPT.read_text()
    defaults = between(source, 'CYCLES="${PERF_CYCLES:-', 'EXPORT_DIR=')
    exported = between(source, 'export PERF_BACKEND=', 'run_pinned() {')
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / 'bin').mkdir()
        for name, body in tools.items():
            tool = root / 'bin' / name
            tool.write_text(body)
            tool.chmod(0o755)
        environment = dict(
            os.environ, PATH=f"{root / 'bin'}:{os.environ['PATH']}",
            PROFILE_DIR=str(root), HEADER=str(root / 'header.txt'),
            SCRIPT_DIR=str(SCRIPT.parent),
            PROFILE_JSON=str(root / 'w8-cycle.profile.json.gz'),
            PROFILE_SYMS=str(root / 'w8-cycle.profile.syms.json'),
            TIMING_JSON=str(root / 'timing.json'), SAMPLY_PRESYMBOLICATE='0',
            **variables)
        environment.pop('PERF_CYCLES', None)
        if perf_cycles is not None:
            environment['PERF_CYCLES'] = perf_cycles
        command = ('set -euo pipefail\n' + defaults + exported +
                   'run_pinned() { "$@"; }\n' + CYCLE_CMD +
                   'recorded=0\n' + branch +
                   '\nprintf "recorded=%s\\n" "$recorded"\n')
        result = subprocess.run(['bash', '-c', command], env=environment,
                                capture_output=True, text=True)
        timing = root / 'timing.json'
        return result, timing.read_text() if timing.exists() else ''


class SampleTotalTests(unittest.TestCase):
    """A samply record reports its sample total, and warns when it is short."""

    def record(self, content, *, host='Linux', status=0, perf_cycles=None):
        """Run the script's samply branch against a samply that saves `content`.

        Return the finished process and the timing file it wrote.
        """
        source = SCRIPT.read_text()
        branch = between(
            source, 'report_samples() {',
            'if [ "$HOST_OS" = Darwin ]; then\n    echo "note: perf record')
        with tempfile.TemporaryDirectory() as directory:
            saved = Path(directory) / 'saved.json.gz'
            saved.write_bytes(content)
            return run_launcher_stretch(
                {'samply': FAKE_SAMPLY}, branch, perf_cycles=perf_cycles,
                FAKE_PROFILE=str(saved), FAKE_SAMPLY_STATUS=str(status),
                HOST_OS=host)

    def test_short_profile_prints_the_total_and_warns(self):
        result, _ = self.record(profile_gz(1))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'samples: 1 (fewer than the {default_level()} needed',
                      result.stdout)
        self.assertIn('warning: the profile has 1 of the', result.stderr)
        self.assertIn('recorded=1', result.stdout)

    def test_profile_at_the_level_prints_the_total_without_a_warning(self):
        level = default_level()
        result, _ = self.record(profile_gz(level))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'samples: {level} (at least the {level} needed',
                      result.stdout)
        self.assertNotIn('needed to rank hot stacks; their order', result.stderr)
        self.assertIn('recorded=1', result.stdout)

    def test_unreadable_profile_warns_and_keeps_the_run(self):
        result, _ = self.record(b'not gzip')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('warning: could not read the sample total from',
                      result.stderr)
        self.assertIn('recorded=1', result.stdout)

    def test_failed_record_on_linux_reports_no_total(self):
        result, _ = self.record(profile_gz(1), status=1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('samples:', result.stdout)
        self.assertIn('recorded=0', result.stdout)

    def test_failed_record_on_darwin_reports_no_total_and_fails(self):
        result, _ = self.record(profile_gz(1), host='Darwin', status=1)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn('samples:', result.stdout)

    def test_samply_gets_its_own_default_cycle_count(self):
        _, timing = self.record(profile_gz(1))
        self.assertIn(f'worker PERF_CYCLES={SAMPLY_CYCLES}\n', timing)

    def test_samply_gets_the_requested_cycle_count(self):
        _, timing = self.record(profile_gz(1), perf_cycles='7')
        self.assertIn('worker PERF_CYCLES=7\n', timing)

    def test_timing_is_still_saved_and_printed(self):
        result, timing = self.record(profile_gz(1))
        self.assertIn('worker PERF_CYCLES=', timing)
        self.assertIn('worker PERF_CYCLES=', result.stdout)


class FallbackTests(unittest.TestCase):
    """The profilers other than samply keep the previous default run."""

    def run_callgrind(self, perf_cycles=None):
        source = SCRIPT.read_text()
        branch = between(
            source,
            'if [ "$HOST_OS" != Darwin ] && [ "$recorded" -eq 0 ] && '
            'command -v valgrind',
            'if [ "$recorded" -eq 0 ]; then\n    echo "warning: samply, flamegraph')
        return run_launcher_stretch(
            {'valgrind': FAKE_VALGRIND}, branch, perf_cycles=perf_cycles,
            HOST_OS='Linux')[0]

    def test_callgrind_keeps_the_previous_cycle_count(self):
        result = self.run_callgrind()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'valgrind PERF_CYCLES={FALLBACK_CYCLES}\n', result.stdout)
        self.assertIn('recorded=1', result.stdout)

    def test_callgrind_gets_the_requested_cycle_count(self):
        result = self.run_callgrind(perf_cycles='7')
        self.assertIn('valgrind PERF_CYCLES=7\n', result.stdout)

    def linux_check(self, **environment):
        base = dict(os.environ, OXPINYIN_PROFILE_OS='Linux')
        base.pop('PERF_CPU', None)
        base.pop('PERF_CYCLES', None)
        return subprocess.run(['bash', str(SCRIPT), '--check'],
                              env=dict(base, **environment),
                              capture_output=True, text=True)

    def test_linux_check_reports_both_defaults(self):
        result = self.linux_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'samply cycles: {SAMPLY_CYCLES}\n', result.stdout)
        self.assertIn(f'fallback cycles: {FALLBACK_CYCLES}\n', result.stdout)

    def test_linux_check_applies_a_requested_count_to_both(self):
        result = self.linux_check(PERF_CYCLES='7')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('samply cycles: 7\n', result.stdout)
        self.assertIn('fallback cycles: 7\n', result.stdout)


if __name__ == '__main__':
    unittest.main()
