#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The behaviours tools/perf-gate/series.py must hold to.

Each case here is a rule from docs/perf/ci-perf-size-gate-proposal-2026-09-09.md,
and several of them encode a mistake the rejected per-PR proposal made: absence
read as zero, a move attributed to a cause the data cannot carry, an off-cadence
sample becoming the predecessor. Pure JSON in, exit code and report text out: the
script runs as a child process, as the nightly lane runs it, with its stdout and
stderr read together.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SERIES = Path(__file__).resolve().with_name('series.py')


def snapshot(patch=None):
    """A healthy snapshot, changed by PATCH(d) when given."""
    d = {
        'captured_utc': '2026-09-12T03:00:00Z',
        'commit': 'a' * 40,
        'event': 'schedule',
        'environment': {
            'kernel': 'Linux 6.12.0 x86_64', 'runner_image': 'ubuntu24/1',
            'rustc': 'rustc 1.97.1', 'cc': 'gcc 15.3.0', 'glibc': '2.42',
            'valgrind': '3.26.0', 'binutils': '2.47', 'backend': 'tkrzw',
            'fixture_sha256': 'f' * 64, 'cpu_pinned': True,
            'harness_commit': 'b' * 40,
        },
        'artifacts': {},
        'metrics': {
            'section_sum': 1000000, 'stripped_size': 1446528,
            'alloc_count_per_cycle': 120, 'alloc_bytes_per_cycle': 40960,
            'ir_oxpinyin_object': 2000000,
            'rss_first_cycle_kib': 14000, 'rss_last_cycle_kib': 23000,
        },
    }
    if patch is not None:
        patch(d)
    return d


def metric(**values):
    """A patch that sets metrics."""
    return lambda d: d['metrics'].update(values)


def yesterday(patch=None):
    """A patch that dates the snapshot a night earlier, then applies PATCH."""
    def apply(d):
        d['captured_utc'] = '2026-09-11T03:00:00Z'
        if patch is not None:
            patch(d)
    return apply


class SeriesTests(unittest.TestCase):
    def setUp(self):
        self.work = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.snapshot_path = self.work / 'snap.json'

    def write(self, patch=None, text=None):
        """Write tonight's snapshot, or TEXT verbatim."""
        self.snapshot_path.write_text(text if text is not None else json.dumps(snapshot(patch)))

    def series(self, directory, *extra):
        """Run series.py; return (exit status, stdout and stderr together)."""
        result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', str(SERIES),
                                 '--snapshot', str(self.snapshot_path), '--series-dir', str(directory), *extra],
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        return result.returncode, result.stdout

    def expect(self, code, directory, *extra, says=()):
        """Run series.py and require its exit status and that the report holds every text in SAYS."""
        status, output = self.series(directory, *extra)
        self.assertEqual(status, code, output)
        for text in says:
            self.assertIn(text, output)
        return output

    def samples(self, directory):
        return sorted(directory.glob('*.json'))

    def with_predecessor(self, name='series', patch=None):
        """A series directory holding last night's sample, written by an appending run as the schedule does."""
        directory = self.work / name
        directory.mkdir()
        self.write(yesterday(patch))
        self.expect(0, directory, '--append')
        return directory

    # --- an empty series: nothing to compare, and that is not an error ---------
    def test_the_first_sample_has_no_predecessor_and_says_so(self):
        empty = self.work / 'empty'
        empty.mkdir()
        self.write()
        self.expect(0, empty, says=('nothing to compare',))

    # --- the append rule: only the schedule writes -----------------------------
    def test_a_run_without_append_persists_nothing_and_says_why(self):
        directory = self.work / 'series'
        directory.mkdir()
        self.write(yesterday())
        self.expect(0, directory, says=('only on the nightly schedule',))
        self.assertEqual(self.samples(directory), [])

    def test_append_persists_exactly_one_sample(self):
        directory = self.work / 'series'
        directory.mkdir()
        self.write(yesterday())
        self.expect(0, directory, '--append')
        self.assertEqual(len(self.samples(directory)), 1)

    # --- a quiet night: small moves are the normal state of a live series ------
    def test_a_tenth_of_a_percent_move_is_not_flagged(self):
        directory = self.with_predecessor()
        self.write(metric(section_sum=1001000))
        self.expect(0, directory)

    # --- a jump, environment unchanged: flag it --------------------------------
    def test_a_five_percent_size_jump_is_flagged_and_names_the_metric(self):
        directory = self.with_predecessor()
        self.write(metric(section_sum=1050000))
        self.expect(1, directory, says=('section_sum',))

    def test_an_allocation_jump_is_flagged(self):
        directory = self.with_predecessor()
        self.write(metric(alloc_count_per_cycle=140))
        self.expect(1, directory)

    # --- the same jump with the environment moved: report, do not attribute ----
    def test_a_jump_across_a_toolchain_change_is_not_flagged_and_names_what_moved(self):
        directory = self.with_predecessor()

        def moved(d):
            d['metrics']['section_sum'] = 1050000
            d['environment']['cc'] = 'gcc 16.1.0'
        self.write(moved)
        self.expect(0, directory, says=('unattributable', 'cc'))

    # --- RSS is a trend, never a trigger ---------------------------------------
    def test_a_thirty_percent_rss_move_is_reported_as_a_trend_only(self):
        directory = self.with_predecessor()
        self.write(metric(rss_last_cycle_kib=30000))
        self.expect(0, directory, says=('trend only',))

    # --- absence is not zero ---------------------------------------------------
    def test_a_null_metric_is_not_a_hundred_percent_improvement(self):
        directory = self.with_predecessor()
        self.write(metric(ir_oxpinyin_object=None, alloc_count_per_cycle=None))
        self.expect(0, directory, says=('not measured',))

    # --- a zero predecessor is not a free pass ---------------------------------
    # alloc_count_per_cycle can legitimately be 0. Going 0 -> positive is the one
    # transition most worth seeing, and a percentage of a zero baseline hides it.
    def test_zero_to_positive_is_flagged_and_zero_to_zero_is_not(self):
        zero = self.with_predecessor('zero', metric(alloc_count_per_cycle=0))
        self.write(metric(alloc_count_per_cycle=5))
        self.expect(1, zero, says=('from zero',))
        self.write(metric(alloc_count_per_cycle=0))
        self.expect(0, zero)

    # --- a malformed snapshot is a tool error, not a finding -------------------
    def test_a_malformed_snapshot_is_a_tool_error(self):
        directory = self.with_predecessor()
        self.write(text='not json {')
        self.expect(2, directory)

    def test_a_snapshot_with_no_metrics_block_is_a_tool_error(self):
        directory = self.with_predecessor()
        self.write(lambda d: d.pop('metrics'))
        self.expect(2, directory)

    # --- structurally invalid input, which is valid JSON -----------------------
    # Each of these parses, then blows up somewhere deep in the comparison unless
    # the shape is checked first.
    def test_a_string_metric_is_malformed_not_compared(self):
        directory = self.with_predecessor()
        self.write(metric(section_sum='1000000'))
        self.expect(2, directory)

    def test_a_json_list_is_malformed(self):
        directory = self.with_predecessor()
        self.write(text='[1, 2, 3]')
        self.expect(2, directory)

    # --- a corrupt predecessor degrades to 'no predecessor' --------------------
    def test_a_corrupt_predecessor_does_not_fail_the_lane(self):
        corrupt = self.work / 'corrupt'
        corrupt.mkdir()
        (corrupt / '20260911T030000-aaaaaaaaaaaa.json').write_text('garbage')
        self.write()
        self.expect(0, corrupt)

    # A predecessor that is valid JSON but the wrong shape takes the same path,
    # rather than raising out of prev.get() or the subtraction.
    def test_a_wrongly_shaped_predecessor_is_treated_as_missing(self):
        shape = self.work / 'shape'
        shape.mkdir()
        (shape / '20260911T030000-aaaaaaaaaaaa.json').write_text('[1, 2, 3]')
        self.write()
        self.expect(0, shape, says=('nothing to compare',))
        (shape / '20260911T030001-bbbbbbbbbbbb.json').write_text('{"metrics": {"section_sum": "big"}}')
        self.write()
        self.expect(0, shape)


if __name__ == '__main__':
    unittest.main()
