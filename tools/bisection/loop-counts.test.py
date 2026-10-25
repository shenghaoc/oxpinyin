#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for the run counts the measurement runners read from the
environment.

A count reaches an arithmetic for-loop, so each runner accepts only a plain
decimal and reads leading zeros as decimal, as `seq` did. Every runner is
driven with stubbed commands that record their calls and stop it at its first
step after the counts are read. run-perf-baseline.sh, whose every input path
comes from the environment, is also driven through both of its loops with a
stubbed workload; the other runners measure fixed /opt container paths, so
their loops are not reached here.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
BASH = shutil.which('bash')
ORACLE_PIN = 'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99'

# Runner, then each environment variable that sets one of its loop counts.
RUNNERS = {
    'run-perf-baseline.sh': ('PERF_RUNS', 'PERF_RAM_RUNS'),
    'run-perf-matrix.sh': ('PERF_RUNS', 'PERF_RAM_RUNS'),
    'run-perf-same-data.sh': ('PERF_RUNS', 'PERF_RAM_RUNS'),
    'run-callgrind-differential.sh': ('CG_ROUNDS', 'T_RUNS', 'HW_RUNS'),
    'run-shipped-check.sh': ('CTRL_RUNS',),
    'run-tree-recipe-control.sh': ('CTRL_RUNS',),
}
ACCEPTED = ('8', '08', '010', '0')
REFUSED = ('2+3', '-1')


def stub(directory, name, body):
    path = directory / name
    path.write_text(f'#!/bin/sh\necho "{name} $*" >> "$STUB_LOG"\n{body}\n')
    path.chmod(0o755)


class Scratch(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix='loop-counts-test-')
        self.addCleanup(directory.cleanup)
        self.work = Path(directory.name)
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.log = self.work / 'stub.log'
        self.log.touch()

    def run_runner(self, runner, environment):
        # A minimal environment: the stubs first, then the Bash the harness
        # resolved (it meets the floor), then the system tools.
        path = os.pathsep.join([str(self.bin), str(Path(BASH).parent), '/usr/bin', '/bin'])
        env = {'PATH': path, 'HOME': str(self.work), 'STUB_LOG': str(self.log), **environment}
        return subprocess.run([BASH, str(HERE / runner)], env=env, cwd=self.work, capture_output=True,
                              text=True, timeout=60, check=False)

    def calls(self):
        return self.log.read_text().splitlines()


class CountValidationTests(Scratch):
    """Each runner refuses a count that is not a plain decimal before it does any work."""

    def setUp(self):
        super().setUp()
        # Every runner's first step after its counts is mkdir or find.
        for name in ('mkdir', 'find', 'gcc', 'cargo', 'uv', 'taskset', 'valgrind', 'ldd', 'otool'):
            stub(self.bin, name, 'exit 1')

    def test_a_count_that_is_not_a_plain_decimal_exits_2_naming_it(self):
        for runner, names in RUNNERS.items():
            for name in names:
                for value in REFUSED:
                    with self.subTest(runner=runner, name=name, value=value):
                        self.log.write_text('')
                        result = self.run_runner(runner, {name: value})
                        self.assertEqual(result.returncode, 2, result.stderr)
                        self.assertEqual(result.stderr,
                                         f"{runner}: {name} must be a decimal count (got '{value}')\n")
                        self.assertEqual(self.calls(), [])

    def test_a_decimal_count_passes_to_the_next_step(self):
        for runner, names in RUNNERS.items():
            for name in names:
                for value in (*ACCEPTED, ''):
                    with self.subTest(runner=runner, name=name, value=value):
                        self.log.write_text('')
                        result = self.run_runner(runner, {name: value})
                        self.assertNotIn('must be a decimal count', result.stderr)
                        self.assertEqual(result.returncode, 1, result.stderr)
                        self.assertRegex(self.calls()[0], r'^(mkdir|find) ')


class BaselineLoopTests(Scratch):
    """run-perf-baseline.sh runs each loop the decimal number of times."""

    def setUp(self):
        super().setUp()
        oracle = self.work / 'oracle'
        (oracle / 'lib' / 'libpinyin' / 'data').mkdir(parents=True)
        (oracle / 'lib' / 'libpinyin.so').touch()
        (oracle / 'oracle-pin.txt').write_text(f'pin_ref={ORACLE_PIN}\n')
        perf = self.work / 'perf'
        usr = perf / 'stage' / 'usr'
        (usr / 'lib' / 'pkgconfig').mkdir(parents=True)
        (usr / 'lib' / 'pkgconfig' / 'libpinyin.pc').touch()
        (usr / 'lib' / 'libpinyin.so').touch()
        data = usr / 'share' / 'oxpinyin'
        data.mkdir(parents=True)
        for name in ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'table.conf',
                     'addon_pinyin_index.bin', 'addon_phrase_index.bin', 'punct.bin', 'gb_char.bin'):
            (data / name).touch()
        # The workload: the harness build, the summaries and every measured
        # process, which taskset launches, record their calls and succeed.
        for name in ('gcc', 'uv', 'taskset', 'ldd', 'otool'):
            stub(self.bin, name, 'exit 0')
        stub(self.bin, 'pkg-config', f'[ "$1" = --variable=libdir ] && echo "{usr / "lib"}"; exit 0')
        self.environment = {'PINYIN_ORACLE_PREFIX': str(oracle), 'OXPINYIN_PERF_WORK': str(perf),
                            'PERF_REUSE_STAGE': '1'}

    def measured(self, mode):
        return sum(1 for call in self.calls() if call.startswith('taskset ') and f' PERF_MODE={mode} ' in call)

    def test_each_loop_runs_its_count_as_a_decimal(self):
        # Empty takes the documented default, as it did before.
        for value, runs, ram_runs in (('8', 8, 8), ('08', 8, 8), ('010', 10, 10), ('0', 0, 0), ('', 20, 10)):
            with self.subTest(value=value):
                self.log.write_text('')
                result = self.run_runner('run-perf-baseline.sh',
                                         {**self.environment, 'PERF_RUNS': value, 'PERF_RAM_RUNS': value})
                self.assertEqual(result.returncode, 0, result.stderr)
                # One oracle and one oxpinyin process per speed run; both, in
                # both RAM modes, per RAM run.
                self.assertEqual(self.measured('speed'), 2 * runs)
                self.assertEqual(self.measured('ram-init'), 2 * ram_runs)
                self.assertEqual(self.measured('ram-cycle'), 2 * ram_runs)


if __name__ == '__main__':
    unittest.main()
