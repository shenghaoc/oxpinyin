"""Measurement reports, tools/dev/measure/reports.py: rows, comparisons and capture checks per platform."""
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev.measure import reports
from tools.dev.test_support import PERF_LIBRARY as LIBRARY, native_row as sample, perf_row


def reduced(kind, block, arm, source='perf_event_open', **changes):
    row = reports.linux_observation(perf_row(), cycles=4, library=LIBRARY, source=source)
    row.update(kind=kind, block=block, arm=arm, instruction_source=source, **changes)
    return row


def session(source='perf_event_open', pairs=6, **by_arm):
    """Balanced rows; BY_ARM maps 'comparison_B' style names to per-block changes."""
    rows = []
    for kind in ('comparison', 'control'):
        for block in range(pairs):
            for arm in 'AB':
                change = by_arm.get(f'{kind}_{arm}', lambda block: {})(block)
                rows.append(reduced(kind, block, arm, source, **change))
    return rows


class LinuxRows(unittest.TestCase):
    def test_the_loop_is_the_steady_cycles_and_every_reading_is_required(self):
        row = reports.linux_observation(perf_row(), cycles=4, library=LIBRARY, source='perf_event_open')
        # Cycle 0 is outside both instruction sources, so it is outside the clocks as well.
        self.assertEqual((row['wall_seconds'], row['cpu_seconds']), (3000 / 1e9, 2700 / 1e9))
        self.assertEqual((row['steady_cycles'], row['loop_hw_instructions']), (3, 700000))
        self.assertEqual((row['anonymous_kib'], row['private_dirty_kib']), (4000, 4100))
        broken = dict(
            so='/stage/B/lib/libpinyin.so.15.0.0', pid=0, repeats=2, workload_checksum='bad',
            cycles_ns=[1000] * 3, cycles_cpu_ns=[0] * 4, steady_hw_cycles=2, steady_hw_instructions=0,
            rollup_cycle=dict(anonymous_kib=-1, private_dirty_kib=4100),
            cycles_cpu_ns_missing=None)
        for key, value in broken.items():
            changed = json.loads(perf_row())
            if key.endswith('_missing'):
                del changed[key.removesuffix('_missing')]
            else:
                changed[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                reports.linux_observation(json.dumps(changed), cycles=4, library=LIBRARY, source='perf_event_open')
        with self.assertRaisesRegex(ValueError, 'fingerprint mismatch'):
            reports.linux_observation(perf_row(), cycles=4, library=LIBRARY, source='callgrind', fingerprint='f' * 16)
        with self.assertRaisesRegex(ValueError, 'two cycles'):
            reports.linux_observation(perf_row(1), cycles=1, library=LIBRARY, source='callgrind')

    def test_a_refused_counter_inside_a_hardware_comparison_fails(self):
        refused = json.loads(perf_row())
        del refused['steady_hw_cycles'], refused['steady_hw_instructions']
        refused['hw_errno'] = 2
        with self.assertRaisesRegex(ValueError, 'never mixed'):
            reports.linux_observation(json.dumps(refused), cycles=4, library=LIBRARY, source='perf_event_open')
        # The same row is complete where Callgrind supplies the count, and carries no hardware figure.
        row = reports.linux_observation(json.dumps(refused), cycles=4, library=LIBRARY, source='callgrind')
        self.assertNotIn('loop_hw_instructions', row)
        # A Callgrind worker's clocks and residency ran under Valgrind and are not read.
        worker = reports.linux_observation(json.dumps(refused), cycles=4, library=LIBRARY)
        self.assertEqual(set(worker), {'pid', 'cycles', 'steady_cycles', 'fingerprint'})


class LinuxReport(unittest.TestCase):
    def test_hardware_report_labels_its_source_and_gates_the_control(self):
        rows = session(comparison_B=lambda block: dict(loop_hw_instructions=770000, private_dirty_kib=4132 + block),
                       control_B=lambda block: dict(wall_seconds=3000 / 1e9 * (1.2 if block % 2 else 1.3)))
        summary = reports.linux_comparison(rows, source='perf_event_open')
        self.assertEqual((summary['platform'], summary['instruction_source']), ('linux', 'perf_event_open'))
        self.assertEqual(list(summary['comparison']),
                         ['loop_hw_instructions', 'wall_seconds', 'cpu_seconds', 'anonymous_kib', 'private_dirty_kib'])
        self.assertEqual(set(summary['units']), set(summary['comparison']))
        self.assertIn('perf_event_open', summary['units']['loop_hw_instructions'])
        self.assertEqual(summary['comparison']['loop_hw_instructions']['B_over_A']['median'], 1.1)
        # Residency is a paired difference in KiB, never a ratio.
        residency = summary['comparison']['private_dirty_kib']
        self.assertNotIn('B_over_A', residency)
        self.assertEqual(residency['B_minus_A_kib']['median'], 34.5)
        control = summary['control']
        self.assertEqual(control['loop_hw_instructions']['gate']['rule'], [0.95, 1.05])
        self.assertEqual(control['anonymous_kib']['gate']['rule'], dict(floor_kib=64, fraction=0.01))
        # An unstable control is DIAGNOSTIC; its estimate stays beside the control limits.
        self.assertFalse(control['wall_seconds']['gate']['stable'])
        self.assertIn('B_over_A', summary['comparison']['wall_seconds'])
        self.assertEqual(summary['verdicts'], dict(loop_hw_instructions='VALID', wall_seconds='DIAGNOSTIC',
                                                    cpu_seconds='VALID', anonymous_kib='VALID',
                                                    private_dirty_kib='VALID'))

    def test_residency_control_outside_its_band_is_diagnostic(self):
        rows = session(control_B=lambda block: dict(anonymous_kib=4065))
        summary = reports.linux_comparison(rows, source='perf_event_open')
        self.assertEqual(summary['control']['anonymous_kib']['gate']['bound_kib'], 64)
        self.assertEqual(summary['verdicts']['anonymous_kib'], 'DIAGNOSTIC')
        self.assertEqual(summary['verdicts']['private_dirty_kib'], 'VALID')

    def test_sources_are_never_mixed_and_the_control_is_required(self):
        rows = session()
        rows[3]['instruction_source'] = 'callgrind'
        with self.assertRaisesRegex(ValueError, 'never mixed'):
            reports.linux_comparison(rows, source='perf_event_open')
        with self.assertRaisesRegex(ValueError, 'same-bytes control'):
            reports.linux_comparison([row for row in session() if row['kind'] == 'comparison'],
                                     source='perf_event_open')

    def callgrind(self, counts):
        rows = []
        for row in session('callgrind', pairs=2):
            key = row['kind'], row['arm']
            row['loop_callgrind_ir'] = counts.get(key, lambda block: 5000)(row['block'])
            rows.append(row)
        return reports.linux_comparison(rows, source='callgrind', creators=['callgrind-3.22.0'])

    def test_callgrind_control_requires_exact_counts(self):
        exact = self.callgrind({('comparison', 'B'): lambda block: 5500})
        self.assertEqual(exact['verdicts']['loop_callgrind_ir'], 'VALID')
        self.assertEqual(exact['comparison']['loop_callgrind_ir']['B_over_A']['median'], 1.1)
        self.assertEqual(exact['control']['loop_callgrind_ir']['gate']['spread'], 0)
        self.assertNotIn('loop_hw_instructions', exact['units'])
        self.assertIn('Callgrind', exact['units']['loop_callgrind_ir'])

        invalid = self.callgrind({('control', 'B'): lambda block: 5003 + block})
        self.assertEqual(invalid['verdicts']['loop_callgrind_ir'], 'INVALID')
        self.assertEqual(set(invalid['comparison']['loop_callgrind_ir']), {'withheld'})
        self.assertIn('B_over_A', invalid['comparison']['wall_seconds'])


class NativeReports(unittest.TestCase):
    def test_xctrace_merged_diagnostic_before_final_result(self):
        row = sample()
        self.assertEqual(reports.driver_result('open user.conf failed.' + json.dumps(row) + '\n'), row)
        with self.assertRaises(ValueError):
            reports.driver_result('open user.conf failed.')

    def test_required_metrics_and_fingerprint(self):
        row = sample()
        self.assertEqual(reports.observation(json.dumps(row), cycles=20, operations=row['operations']), row)
        for key, bad in [('operations', 0), ('cycles', 19), ('fingerprint', 'bad'),
                         *[(key, value) for key in reports.MEASURES for value in (None, 0, -1, float('nan'), True)
                           if (key, value) != ('loop_runnable_seconds', 0)]]:
            with self.subTest(key=key, bad=bad), self.assertRaises(ValueError):
                reports.observation(json.dumps({**row, key: bad}), cycles=20, operations=row['operations'])
        idle = {**row, 'loop_runnable_seconds': 0}
        self.assertEqual(reports.observation(json.dumps(idle), cycles=20, operations=row['operations']), idle)
        with self.assertRaisesRegex(ValueError, 'fingerprint mismatch'):
            reports.observation(json.dumps(row), cycles=20, operations=row['operations'], fingerprint='f'*16)

    def test_capture_requires_same_row_process_stack_size_and_product_uuid(self):
        xml = '''<trace-query-result><node><schema/><row>
        <sample-time>1</sample-time><process id="p" fmt="bisect (17)"><pid>17</pid></process>
        <size>64</size><tagged-backtrace id="s"><frame name="pinyin_guess_candidates">
        <binary UUID="00112233-4455-6677-8899-aabbccddeeff"/></frame></tagged-backtrace></row>
        <row><sample-time>2</sample-time><process ref="p"/><size>32</size><tagged-backtrace ref="s"/></row>
        </node></trace-query-result>'''
        uuid = '00112233445566778899aabbccddeeff'
        self.assertEqual(reports.capture_rows(xml, pid=17, uuid=uuid, allocations=True),
                         dict(usable_rows=2, product_frames=2))
        mangled = '_RNvMs2_CsHASH_8oxpinyinNtB5_11RuntimeDict11fill_lookup'
        split = xml.replace('pinyin_guess_candidates', mangled).replace(
            '<row><sample-time>2</sample-time>', '</node><node><row><sample-time>2</sample-time>')
        self.assertEqual(reports.capture_rows(split, pid=17, uuid=uuid, allocations=True),
                         dict(usable_rows=2, product_frames=2))
        for broken in (xml.replace('17', '18'), xml.replace('pinyin_guess_candidates', 'malloc'),
                       xml.replace('aabbccddeeff', 'ffffffffffff'),
                       xml.replace('<size>', '<not-size>').replace('</size>', '</not-size>'),
                       '<trace-query-result/>'):
            with self.subTest(broken=broken), self.assertRaises(ValueError):
                reports.capture_rows(broken, pid=17, uuid=uuid, allocations=True)


class WindowsReports(unittest.TestCase):
    def test_commit_uses_paired_bytes_and_missing_metrics_fail(self):
        row = dict(pid=1, cycles=1, operations=123, fingerprint='a'*16,
                   wall_seconds=1., cpu_seconds=.1, loop_cpu_cycles=1000,
                   peak_private_commit_bytes=100, peak_working_set_bytes=200)
        rows = []
        for block, a, b in ((0, 100, 140), (1, 900, 920)):
            for arm, value in (('A', a), ('B', b)):
                rows.append(dict(row, kind='comparison', block=block, arm=arm,
                                 peak_private_commit_bytes=value,
                                 loop_cpu_cycles=1000 if arm == 'A' else 1500))
        summary = reports.comparison(rows, windows=True)
        result = summary['comparison']['peak_private_commit_bytes']
        self.assertNotIn('B_over_A', result)
        self.assertEqual(result['B_minus_A_bytes']['median'], 30)
        # Cycles and wall time lead; cycles are a count with a B/A ratio, never seconds.
        self.assertEqual(list(summary['comparison'])[:2], ['loop_cpu_cycles', 'wall_seconds'])
        self.assertEqual(summary['comparison']['loop_cpu_cycles']['B_over_A']['median'], 1.5)
        self.assertEqual(set(summary['units']), set(reports.WINDOWS_MEASURES))
        self.assertIn('QueryProcessCycleTime', summary['units']['loop_cpu_cycles'])
        self.assertIn('not convertible to seconds', summary['units']['loop_cpu_cycles'])
        self.assertIn('1/64-second', summary['units']['cpu_seconds'])
        for key in reports.WINDOWS_MEASURES:
            for bad in (None, -1, True):
                with self.subTest(key=key, bad=bad), self.assertRaises(ValueError):
                    reports.observation(json.dumps(dict(row, **{key: bad})), cycles=1,
                                        operations=123, windows=True)
        # Only the GetProcessTimes figure may read zero; a zero or fractional cycle delta is broken.
        reports.observation(json.dumps(dict(row, cpu_seconds=0.)), cycles=1, operations=123, windows=True)
        for bad in (0, 1.5):
            with self.subTest(cycles=bad), self.assertRaises(ValueError):
                reports.observation(json.dumps(dict(row, loop_cpu_cycles=bad)), cycles=1,
                                    operations=123, windows=True)


if __name__ == '__main__':
    unittest.main()
