#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Teardown instrumentation controls using private native stubs and mocked inputs."""
import contextlib
import importlib.util
import io
import json
import mmap
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import teardown as td

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import pool, test_support


class PureTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux measurement controller')
    def test_unordered_subject_is_refused_before_building_or_running_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lib').mkdir()
            (root / 'lib/libpinyin.so').write_bytes(b'not an ordered ELF')
            args = SimpleNamespace(cell='bdb', per_call=None, cases='', issue=[], family=[],
                                   expect_parent=False, observations=None, plan_only=False,
                                   axis_c_replacement=None, axis_c_mutation=None,
                                   axis_c_snapshot_check=False, regenerate_sentence_ties=None,
                                   teardown_work_dir=root, teardown_cases='empty',
                                   teardown_cpu=min(os.sched_getaffinity(0)), teardown_facades='pinyin',
                                   pinyin_so=root / 'lib/libpinyin.so', zhuyin_so=None, prefix=root)
            with patch.object(td, 'filesystem', return_value='ext4'), \
                    patch.object(td, 'build_worker') as worker:
                with self.assertRaisesRegex(ValueError, 'function ordering: arm ox/pinyin refused'):
                    td.run(args, None, None)
                worker.assert_not_called()

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux measurement controller')
    def test_zhuyin_only_provision_prepares_witnesses_with_the_pinyin_provisioner(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lib/libpinyin/data').mkdir(parents=True)
            (root / 'lib/libpinyin/data/table.conf').write_bytes(b'table')
            (root / 'lib/libpinyin.so').write_bytes(b'pin pinyin')
            (root / 'lib/libzhuyin.so').write_bytes(b'pin zhuyin')
            (root / 'ox-libzhuyin.so').write_bytes(b'ox zhuyin')
            args = SimpleNamespace(cell='bdb', teardown_stage='provision', per_call=None, cases='', issue=[],
                                   family=[], expect_parent=False, observations=None, plan_only=False,
                                   axis_c_replacement=None, axis_c_mutation=None,
                                   axis_c_snapshot_check=False, regenerate_sentence_ties=None,
                                   teardown_work_dir=root, teardown_cases='empty',
                                   teardown_cpu=min(os.sched_getaffinity(0)), teardown_facades='zhuyin',
                                   pinyin_so=None, zhuyin_so=root / 'ox-libzhuyin.so', prefix=root)
            profiles = dict(empty=root / 'profiles/empty/user')
            with patch.object(td, 'filesystem', return_value='ext4'), \
                    patch.dict(td.FUNCTION_ORDER, check_arms=lambda *a, **k: None), \
                    patch.object(td, 'build_worker', return_value=(Path('worker'), {})), \
                    patch.object(td, 'Campaign', lambda *a, **k: SimpleNamespace()), \
                    patch.object(td, 'print_record'), \
                    patch.object(td, 'provision', return_value=profiles) as provision, \
                    patch.object(td, 'prepare_witnesses', return_value={'empty': None}) as witnesses, \
                    patch.object(td, 'execute', side_effect=AssertionError('provision returns before execute')):
                self.assertEqual(td.run(args, None, None), 0)
            # No pinyin subject: the pin's pinyin library is copied in as the provisioner,
            # and both the profiles and the witnesses come from that handle.
            provisioner = root / 'libraries/pinlibs/libpinyin.so'
            self.assertEqual(provisioner.read_bytes(), b'pin pinyin')
            self.assertEqual(provision.call_args.args[1], provisioner)
            self.assertEqual(witnesses.call_args.args[1], provisioner)
            self.assertIs(witnesses.call_args.args[0], provision.call_args.args[0])

    def refusal(self, **overrides):
        """What `run` refuses with. The defaults pass every scope check, so the
        refusal is then the missing work directory."""
        values = dict(cell='bdb', teardown_stage='provision', per_call=None, cases='', issue=[], family=[],
                      expect_parent=False, observations=None, plan_only=False, axis_c_replacement=None,
                      axis_c_mutation=None, axis_c_snapshot_check=False, regenerate_sentence_ties=None,
                      teardown_work_dir=None)
        values.update(overrides)

        class Refused(Exception):
            pass

        def error(message):
            raise Refused(message)
        with self.assertRaises(Refused) as raised:
            td.run(SimpleNamespace(**values), None, SimpleNamespace(error=error))
        return str(raised.exception)

    def test_provision_stage_runs_on_every_cell(self):
        for cell in ('bdb', 'kc', 'tkrzw'):
            with self.subTest(cell=cell):
                self.assertIn('--teardown-work-dir', self.refusal(cell=cell))

    def test_measured_stages_without_taskset_refuse_before_build_or_launch(self):
        stages = [('warm', stage) for stage in ('pilot', 'confirm', 'repeat', 'multi', 'attribute', 'init-attribute')]
        stages += [('cold', stage) for stage in ('pilot', 'confirm', 'init-attribute')]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lib').mkdir()
            (root / 'lib/libpinyin.so').write_bytes(b'pin')
            (root / 'subject.so').write_bytes(b'ox')
            for cache, stage in stages:
                with self.subTest(cache=cache, stage=stage), \
                        patch.object(sys, 'platform', 'linux'), \
                        patch.object(td.host, 'allowed_cpus', return_value={3}), \
                        patch.object(td.shutil, 'which', return_value=None), \
                        patch.object(td, 'filesystem', return_value='ext4'), \
                        patch.dict(td.FUNCTION_ORDER, check_arms=lambda *a, **k: None), \
                        patch.object(td, 'build_worker') as build, \
                        patch.object(td.subprocess, 'Popen') as launch:
                    message = self.refusal(teardown_stage=stage, teardown_cache=cache,
                                           teardown_work_dir=root, teardown_cases='empty', teardown_cpu=3,
                                           teardown_facades='pinyin', pinyin_so=root / 'subject.so',
                                           zhuyin_so=None, prefix=root)
                    self.assertIn('measured teardown stages require taskset', message)
                    build.assert_not_called()
                    test_support.assert_not_called(self, launch)

    def test_comparison_stages_accept_all_three_cells(self):
        for stage in ('verify', 'pilot', 'confirm', 'repeat', 'multi', 'attribute', 'init-attribute'):
            for cell in ('bdb', 'kc', 'tkrzw'):
                with self.subTest(cell=cell, stage=stage):
                    self.assertIn('--teardown-work-dir', self.refusal(cell=cell, teardown_stage=stage))

    def test_contract_and_profiler_modes_are_refused_on_every_cell(self):
        modes = dict(per_call='wrap', cases='default-option-parse', issue=['1'], family=['f'],
                     expect_parent=True, observations=Path('observations'), plan_only=True,
                     axis_c_replacement='r', axis_c_mutation='m', axis_c_snapshot_check=True,
                     regenerate_sentence_ties='t')
        for cell in ('bdb', 'kc', 'tkrzw'):
            for mode, value in modes.items():
                with self.subTest(cell=cell, mode=mode):
                    self.assertIn('cannot be combined', self.refusal(cell=cell, **{mode: value}))

    def test_existing_smaps_schema_and_requested_pss(self):
        text = '1000-2000 r--p 0000 00:00 0 /data/gb_char.bin\nRss: 8 kB\nPss: 4 kB\nAnonymous: 0 kB\n'
        old = td.SMAPS.parse_lines(text.splitlines())
        self.assertNotIn('Pss', old[0])
        self.assertEqual(old[0]['Rss'], 8)
        new = td.SMAPS.parse_lines(text.splitlines(), td.FIELDS)
        self.assertEqual(new[0]['Pss'], 4)
        self.assertEqual(td.mapping_group(new[0], Path('/lib.so'), Path('/data')), 'DATA:gb_char.bin:readonly')

    def test_signed_residuals_and_disappearing_mapping(self):
        self.assertEqual(td.signed_delta({'Rss': 0}, {'Rss': 8}), {'Rss': -8})
        self.assertEqual(td.signed_delta({}, {'Rss': 8}), {'Rss': -8})

    def test_memory_totals_and_groups_use_one_read_despite_later_rollup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            proc = root / '123'
            proc.mkdir()
            (proc / 'fd').mkdir()
            (proc / 'smaps').write_text('1000-2000 r--p 0000 00:00 0\n'
                                       'Rss: 8 kB\nPss: 3 kB\nAnonymous: 0 kB\n'
                                       'Private_Clean: 8 kB\nPrivate_Dirty: 0 kB\nShared_Clean: 0 kB\n')
            (proc / 'smaps_rollup').write_text('Rss: 8 kB\nPss: 4 kB\nAnonymous: 0 kB\n'
                                              'Private_Clean: 0 kB\nPrivate_Dirty: 0 kB\nShared_Clean: 8 kB\n')
            result = td.read_memory(123, Path('/lib.so'), Path('/data'), proc_root=root)
            self.assertEqual(result['totals']['Private_Clean'], 8)
            self.assertEqual(result['rollup_minus_smaps']['Private_Clean'], -8)
            self.assertEqual(result['rollup_minus_smaps']['Shared_Clean'], 8)
            self.assertEqual(result['rollup_minus_smaps']['Pss'], 1)
            for field in td.FIELDS:
                self.assertEqual(result['totals'][field], sum(group[field] for group in result['groups'].values()))

    def test_ratios_are_paired_and_control_rule_is_fixed(self):
        self.assertEqual(td.paired_ratio([2, 200], [1, 100])['median'], 2)
        self.assertTrue(td.stable_control([100] * 8, [101] * 8)['stable'])
        self.assertFalse(td.stable_control([100] * 8, [106] * 8)['stable'])
        with self.assertRaises(ValueError):
            td.paired_ratio([1], [0])
        with self.assertRaises(ValueError):
            td.paired_ratio([1], [1, 2])

    # §7.5 boundary tests. Constant samples make the paired median and the
    # whole bootstrap interval equal one value, so each bound is hit exactly.
    def test_timing_control_bounds_are_inclusive(self):
        ox = [100] * 8
        self.assertTrue(td.stable_control(ox, [95] * 8)['stable'])
        self.assertTrue(td.stable_control(ox, [105] * 8)['stable'])
        self.assertEqual(td.stable_control(ox, [95] * 8)['ratio']['ci95'], [0.95, 0.95])
        self.assertFalse(td.stable_control(ox, [94.99] * 8)['stable'])
        self.assertFalse(td.stable_control(ox, [105.01] * 8)['stable'])

    def test_timing_control_needs_the_whole_interval_inside(self):
        result = td.stable_control([100] * 20, [100] * 12 + [120] * 8)
        self.assertEqual(result['ratio']['median'], 1)
        self.assertGreater(result['ratio']['ci95'][1], td.CONTROL_INTERVAL[1])
        self.assertFalse(result['stable'])

    def test_residency_bound_is_inclusive_at_both_signs(self):
        ox = [1000] * 8
        for sign in (1, -1):
            inside = td.stable_residency(ox, [v + sign * 64 for v in ox])
            self.assertEqual(inside['bound_kib'], 64)
            self.assertEqual(inside['difference']['ci95'], [sign * 64, sign * 64])
            self.assertTrue(inside['stable'])
            self.assertFalse(td.stable_residency(ox, [v + sign * 65 for v in ox])['stable'])

    def test_residency_bound_is_the_floor_or_one_percent(self):
        # 1% of 1,000 KiB is 10 KiB, under the floor.
        self.assertEqual(td.stable_residency([1000] * 8, [1000] * 8)['bound_kib'], 64)
        # 1% of 100,000 KiB is 1,000 KiB, over the floor.
        large = [100_000] * 8
        self.assertEqual(td.stable_residency(large, large)['bound_kib'], 1000)
        for sign in (1, -1):
            self.assertTrue(td.stable_residency(large, [v + sign * 1000 for v in large])['stable'])
            self.assertFalse(td.stable_residency(large, [v + sign * 1001 for v in large])['stable'])
        self.assertEqual(td.stable_residency(large, large)['rule'], dict(floor_kib=64, fraction=0.01))

    def test_residency_needs_the_whole_interval_inside(self):
        ox = [1000] * 20
        result = td.stable_residency(ox, [1000] * 12 + [1200] * 8)
        self.assertEqual(result['difference']['median'], 0)
        self.assertGreater(result['difference']['ci95'][1], result['bound_kib'])
        self.assertFalse(result['stable'])
        with self.assertRaises(ValueError):
            td.paired_difference([1], [1, 2])
        with self.assertRaises(ValueError):
            td.paired_difference([], [])

    def test_residency_with_zero_or_negative_ox_uses_the_floor(self):
        for level in (0, -100):
            ox = [level] * 8
            self.assertEqual(td.stable_residency(ox, ox)['bound_kib'], 64)
            self.assertTrue(td.stable_residency(ox, [v + 64 for v in ox])['stable'])
            self.assertFalse(td.stable_residency(ox, [v + 65 for v in ox])['stable'])

    def test_exact_counts_admit_no_difference(self):
        self.assertEqual(td.exact_control(5, 5)['verdict'], 'VALID')
        self.assertEqual(td.exact_control(5, 6), dict(rule='exact', ox=5, control=6, difference=1, verdict='INVALID'))
        self.assertEqual(td.exact_control(6, 5)['difference'], -1)

    def test_invalid_exact_count_withholds_ox_and_pin_rows(self):
        rows = {side: dict(scoped_ir=10, raw_ir_per_call=10.0) for side in ('ox', 'control', 'pin')}
        printed = []
        with patch.object(td, 'print_record', lambda kind, **values: printed.append((kind, values))):
            self.assertFalse(td.report_exact('callgrind', 'pinyin', 'modest', rows, td.callgrind_counts))
            self.assertEqual([(kind, values.get('side')) for kind, values in printed],
                             [('callgrind-control', None), ('callgrind', 'ox'), ('callgrind', 'control'),
                              ('callgrind', 'pin')])
            self.assertTrue(all(values['profiler_only'] for kind, values in printed[1:]))
            printed.clear()
            rows['control'] = dict(scoped_ir=11, raw_ir_per_call=11.0)
            self.assertTrue(td.report_exact('callgrind', 'pinyin', 'modest', rows, td.callgrind_counts))
            self.assertEqual([(kind, values.get('side')) for kind, values in printed],
                             [('callgrind-control', None), ('callgrind', 'control')])
            self.assertEqual({key: printed[0][1][key] for key in ('ox', 'control', 'difference', 'verdict')},
                             dict(ox=10, control=11, difference=1, verdict='INVALID'))

    def test_exact_counts_select_the_ruled_fields(self):
        points = {p: dict(time=i, mem_heap_B=100 + i, mem_heap_extra_B=8, mem_stacks_B=0)
                  for i, p in enumerate(('M0', 'M1', 'M2', 'M3'))}
        self.assertEqual(td.massif_counts(dict(checkpoints=points, sampled_teardown_peak=dict(mem_heap_B=900))),
                         {'M0.mem_heap_B': 100, 'M1.mem_heap_B': 101, 'M2.mem_heap_B': 102, 'M3.mem_heap_B': 103})
        point = dict(heap_live=True, point='M3', cycle=100, lost_bytes=1, possible_bytes=2, reachable_bytes=4,
                     suppressed_bytes=8, lost_blocks=1, possible_blocks=1, reachable_blocks=1, suppressed_blocks=0)
        self.assertEqual(td.live_heap_counts(dict(points=[point], sites=[])),
                         {'M3.100.live_bytes': 15, 'M3.100.live_blocks': 3})

    def test_attribute_stage_exits_nonzero_after_printing_every_record(self):
        points = [('M0', 0), ('M1', 1), ('M2', 1), ('M3', 1), ('M3', 10), ('M3', 50), ('M3', 100)]

        def outcome(control_ir):
            ir = dict(ox=1000, control=control_ir, pin=2000)

            def run(facade, library, profile=None, *, profiler=None, **options):
                side = library.name
                documents = [dict(heap_live=True, point=point, cycle=cycle,
                                  **{kind + '_' + unit: 1 for kind in ('lost', 'possible', 'reachable', 'suppressed')
                                     for unit in ('bytes', 'blocks')}) for point, cycle in points]
                # Profilers read their output from stderr; equivalence runs see one observation.
                return dict(observation=dict(samples=[1], stderr=side if profiler else '', userfiles={},
                                             documents=documents),
                            raw_profile_identity='same')
            driver = SimpleNamespace(per_call_profile=lambda side: dict(total=ir[side]),
                                     per_call_attribution=lambda profile, calls, limit: dict(raw_ir_per_call=profile['total']))
            heaps = {p: dict(time=0, mem_heap_B=64, mem_heap_extra_B=8, mem_stacks_B=0) for p in ('M0', 'M1', 'M2', 'M3')}
            printed = []
            with tempfile.TemporaryDirectory() as directory, \
                    patch.object(td, 'prepare_witnesses', lambda *a: dict(empty=None)), \
                    patch.object(td, 'build_worker', lambda *a: (Path('worker'), {})), \
                    patch.object(td, 'massif_profile', lambda text: dict(checkpoints=heaps)), \
                    patch.object(td, 'memcheck_sites', lambda *a: []), \
                    patch.object(td, 'print_record', lambda kind, **values: printed.append((kind, values))):
                campaign = SimpleNamespace(work=Path(directory), worker=Path('worker'), run=run)
                subjects = {side: dict(pinyin=Path(side)) for side in ('ox', 'control', 'pin')}
                args = SimpleNamespace(teardown_stage='attribute', teardown_attribution_case='empty')
                status = td.execute(campaign, args, subjects, dict(empty=Path(directory)), ('empty',), ('pinyin',), driver)
            return status, printed

        status, printed = outcome(1000)
        self.assertEqual(status, 0)
        self.assertTrue(all(values['verdict'] == 'VALID' for kind, values in printed if kind.endswith('-control')))
        self.assertEqual(sum(kind == 'callgrind' for kind, _ in printed), 3)
        status, printed = outcome(1001)
        self.assertEqual(status, 1)
        invalid = [values for kind, values in printed if kind == 'callgrind-control']
        self.assertEqual([(v['ox'], v['control'], v['difference'], v['verdict']) for v in invalid],
                         [(1000, 1001, 1, 'INVALID')])
        self.assertEqual([values['side'] for kind, values in printed if kind == 'callgrind'], ['control'])
        # The later Massif and Memcheck comparisons still print in full.
        self.assertEqual(sum(kind == 'massif' for kind, _ in printed), 3)
        self.assertEqual(sum(kind == 'live-heap-control' for kind, _ in printed), 14)
        self.assertEqual(sum(kind == 'live-heap' for kind, _ in printed), 3)

    def test_subject_control_and_pin_library_paths_have_equal_length(self):
        names = td.LIBRARY_DIRECTORIES
        self.assertEqual(set(names), {'ox', 'control', 'pin', 'base'})
        self.assertEqual(len(set(names.values())), 4)
        work = Path('/work/libraries')
        lengths = {side: len(bytes(work / name / 'libpinyin.so')) for side, name in names.items()}
        self.assertEqual(len(set(lengths.values())), 1, lengths)

    def test_recovered_rows_and_global_distinctness(self):
        rows = td.modest_rows()
        self.assertEqual(len(rows), 80)
        self.assertEqual(rows[:2], [(1, '测试', "ce4'shi4", 5), (1, '测侧', "ce4'ce4", 5)])
        exported = [(library, f'phrase-{library}-{i}', 'ni3', 5) for library in range(1, 5) for i in range(4000)]
        sequential = td.select_rows(exported)
        balanced = td.select_rows(exported, True)
        self.assertEqual(len(sequential), 10000)
        self.assertEqual([sum(row[0] == library for row in balanced) for library in range(1, 5)], [3000, 3000, 3000, 1000])
        self.assertEqual(td.witness_inputs(rows), ["ce'shi", "ce'ce"])

    def test_user_recipes_have_ten_thousand_items_and_distinct_ordered_readings(self):
        exported = [(1, f'词{i:05}', "ci2'yi1'er4'san1'si4'wu3", 20) for i in range(10000)]
        for readings in (1, 8):
            rows = td.user_rows(exported, readings)
            self.assertEqual(len(rows), 10000 * readings)
            self.assertEqual({row[0] for row in rows}, {7})
            for start in range(0, len(rows), readings):
                group = rows[start:start + readings]
                self.assertEqual(len({row[1] for row in group}), 1)
                self.assertEqual(len({row[2] for row in group}), readings)
                self.assertEqual(group[0][2], exported[start // readings][2])
                self.assertEqual({row[3] for row in group}, {5})

    def test_user_recipes_require_enough_multi_character_items(self):
        with self.assertRaisesRegex(ValueError, '10,000'):
            td.user_rows([(1, '词', 'ci2', 20)] * 10000, 8)
        with self.assertRaisesRegex(ValueError, 'one or eight'):
            td.user_rows([], 2)

    def test_startup_and_teardown_share_user_profile_names(self):
        from tools.dev.measure import startup
        self.assertEqual(startup.PROFILES, td.PROFILES)
        self.assertTrue({'user', 'user8'}.issubset(td.PROFILES))

    def test_allocation_windows_count_freed_blocks_and_file_read_buffers(self):
        def point(name, live, blocks, total, allocated):
            return (f'TEARDOWN_ALLOC {name} BEGIN\nevents: curB curBk totB totBk totFdB totFdBk\n'
                    f'totals: {live} {blocks} {total} {allocated} 0 0\nTEARDOWN_ALLOC {name} END\n')
        text = point('M0', 10, 1, 20, 2) + point('I1', 30, 2, 120, 6)
        result = td.allocation_windows(text, [('open', 'M0', 'I1')])
        self.assertEqual(result, dict(open=dict(allocated_bytes=100, allocations=4,
                                                retained_bytes=20, retained_blocks=1)))
        for broken in (text.replace('totals:', 'unknown:'), text + point('I1', 30, 2, 120, 6),
                       text.replace('120 6', '120 1')):
            with self.assertRaises(ValueError):
                td.allocation_windows(broken, [('open', 'M0', 'I1')])

    def test_user_census_distinguishes_dense_and_sparse_offsets(self):
        import struct
        from contract_case_helpers import _chunk
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'user.bin'
            body = bytes([2, 1]) + struct.pack('<IIIHHI', 15, ord('测'), ord('试'), 1, 2, 5)
            for offsets in ([0, 8, 30], [0, 8] + [0] * 20 + [30]):
                one, two = 17, 18 + 4 * len(offsets)
                content = b'\0' * 8 + body * 2
                payload = struct.pack('<IIII', 30, one, two, two + len(content) + 1)
                payload += b'#' + struct.pack('<' + 'I' * len(offsets), *offsets) + b'#' + content + b'#'
                path.write_bytes(_chunk(payload))
                shape = td.user_library_shape(path)
                self.assertEqual(shape['items'], 2)
                self.assertEqual(shape['slots'], len(offsets))
                self.assertEqual(shape['readings'], {1: 2})
                self.assertEqual(shape['live_body_bytes'], 44)
            path.write_bytes(path.read_bytes()[:-1])
            with self.assertRaisesRegex(ValueError, 'frame'):
                td.user_library_shape(path)

    def test_sampled_heap_peaks_are_not_exact_control_counts(self):
        a = {'open': dict(before_bytes=10, after_bytes=20, sampled_peak_bytes=30)}
        b = {'open': dict(before_bytes=10, after_bytes=20, sampled_peak_bytes=31)}
        self.assertEqual(td.user_heap_counts(a), td.user_heap_counts(b))
        b['open']['after_bytes'] += 1
        self.assertNotEqual(td.user_heap_counts(a), td.user_heap_counts(b))

    def test_compaction_call_counts_resolve_compressed_function_names(self):
        text = ('fn=(1) pinyin::FacadePhraseIndex::compact()\ncfn=(1)\ncalls=2 0\n0 9\n'
                'fn=(2) caller\ncfn=(3) other\ncalls=100 0\n0 3\ncfn=(1)\ncalls=15 0\n0 7\n')
        self.assertEqual(td.compaction_calls(text), {'pinyin::FacadePhraseIndex::compact()': 17})

    def test_user_timing_and_smaps_use_separate_native_processes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'user8').mkdir()
            (root / 'user8/rows.tsv').write_text("7\t测试\tce4'shi4\t5\n" * 8)
            calls = []
            def run(*args, **kwargs):
                calls.append(kwargs)
                times = {name: dict(wall_ns=100) for name, _, _ in td.USER_WINDOWS}
                result = dict(observation=dict(stderr='open $USER/user.conf failed.\n', userfiles={}, documents=[dict(user_phrase=True, times=times)]),
                              user_bin_sha256='same')
                if kwargs.get('user_memory'):
                    result['checkpoints'] = {0: {'M0': dict(groups={}, totals=dict(Pss=100))},
                                             1: {'I1': dict(groups={}, totals=dict(Pss=100))}}
                return result
            campaign = SimpleNamespace(worker=Path('worker'), work=root, run=run)
            subjects = {side: dict(pinyin=Path(side)) for side in ('ox', 'control', 'pin', 'base')}
            with patch.object(td, 'user_library_shape', return_value=dict(items=10000, readings={1: 10000})), \
                    patch.object(td, 'build_worker', return_value=(Path('instrumented'), {})), \
                    patch.object(td, 'allocation_windows', return_value={}), \
                    patch.object(td, 'user_massif', return_value={}), \
                    patch.object(td, 'callgrind_costs', return_value=dict(totals=dict(Ir=1))), \
                    patch.object(td, 'print_record'), \
                    patch.object(td, 'TIMING_ROUNDS', 1), patch.object(td, 'TIMING_WARMUPS', 0), \
                    patch.object(td, 'MEMORY_ROUNDS', 1), patch.object(td, 'MEMORY_WARMUPS', 0):
                self.assertEqual(td.user_phrase_measure(campaign, subjects,
                    dict(user=root / 'user', user8=root / 'user8/user'), ['user']), 0)
                def changed_diagnostic(*args, **kwargs):
                    result = run(*args, **kwargs)
                    if args[1] == Path('pin'):
                        result['observation']['stderr'] += 'unexpected warning\n'
                    return result
                campaign.run = changed_diagnostic
                with self.assertRaisesRegex(ValueError, 'diagnostics changed'):
                    td.user_phrase_measure(campaign, subjects,
                        dict(user=root / 'user', user8=root / 'user8/user'), ['user'], verify=True)
            native = [call for call in calls if call.get('profiler') is None]
            self.assertEqual([call.get('user_memory', False) for call in native[:8]], [False] * 4 + [True] * 4)

    def test_failed_preflight_can_retain_complete_observations_without_waiving_it(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'summaries').mkdir()
            observations = {}
            def run(facade, library, **kwargs):
                observation = dict(samples=[], stderr='', userfiles={'user.bin': 'unchanged'},
                                   documents=[dict(transcript="ai'hu", type=2 if library == Path('pin') else 1)],
                                   reopen=dict(result='retained'))
                observations[str(library)] = observation
                return dict(observation=observation, raw_profile_identity='raw')
            campaign = SimpleNamespace(work=root, provisioner=Path('pin'), run=run,
                                       summary_dir=root / 'summaries', summary_identity={'source_revision': 'current'})
            with patch.object(td, 'prepare_witnesses', return_value={'empty': root / 'witness'}), \
                    patch.object(td, 'print_record'):
                self.assertEqual(td.execute(campaign, SimpleNamespace(teardown_stage='verify'),
                    {'pin': {'pinyin': Path('pin')}, 'ox': {'pinyin': Path('ox')}},
                    {'empty': root}, ['empty'], ['pinyin'], None), 3)
            saved = json.loads((root / 'summaries/pinyin-empty-preflight.json').read_text())
            self.assertEqual(saved['observations'], observations)
            self.assertEqual(saved['digests'], {name: td.identity(value) for name, value in observations.items()})

    def test_precise_equivalence_difference(self):
        self.assertEqual(td.first_difference({'a': [{'x': 1}]}, {'a': [{'x': 2}]}),
                         {'path': '$.a[0].x', 'pin': 1, 'subject': 2})

    def scoped_preflight(self, stage, changed=None):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def run(facade, library, **kwargs):
                obs = dict(samples=[dict(candidate='nihao')], stderr='', userfiles={'user.bin': 'same'},
                           documents=[dict(script_type=2 if library == Path('pin') else 1)])
                if changed and library == Path('ox') and kwargs['profile'] == root / 'modest':
                    obs[changed] = {'samples': [dict(candidate='different')], 'stderr': 'warning',
                                    'userfiles': {'user.bin': 'different'}}[changed]
                return dict(observation=obs, raw_profile_identity='raw')
            campaign = SimpleNamespace(work=root, provisioner=Path('pin'), run=run)
            records = []
            with patch.object(td, 'prepare_witnesses', return_value=dict.fromkeys(('empty', 'modest'), root)), \
                    patch.object(td, 'print_record', side_effect=lambda kind, **row: records.append((kind, row))), \
                    patch.object(td, 'init_attribute', return_value=0) as measured, \
                    patch.object(td, 'report_times') as times, patch.object(td, 'report_memory'), \
                    patch.object(td, 'TIMING_ROUNDS', 1), patch.object(td, 'TIMING_WARMUPS', 0), \
                    patch.object(td, 'MEMORY_ROUNDS', 1), patch.object(td, 'MEMORY_WARMUPS', 0):
                status = td.execute(campaign, SimpleNamespace(teardown_stage=stage, teardown_startup_heap=False),
                    {side: {'pinyin': Path(side)} for side in ('pin', 'ox', 'control')},
                    {name: root / name for name in ('empty', 'modest')}, ['empty', 'modest'], ['pinyin'], None)
            return status, records, measured, times

    def test_scoped_workload_keeps_broad_candidate_failure_strict(self):
        status, records, measured, times = self.scoped_preflight('verify')
        self.assertEqual(status, 3)
        measured.assert_not_called()
        times.assert_not_called()
        self.assertEqual([row['verdict'] for kind, row in records if kind == 'equivalence'],
                         ['NOT COMPARABLE', 'NOT COMPARABLE'])

    def test_script_difference_does_not_block_identical_measured_workload(self):
        for stage in ('confirm', 'init-attribute'):
            with self.subTest(stage=stage):
                status, records, measured, times = self.scoped_preflight(stage)
                self.assertEqual(status, 0)
                self.assertEqual([row['verdict'] for kind, row in records if kind == 'workload-equivalence'],
                                 ['COMPARABLE', 'COMPARABLE'])
                if stage == 'confirm':
                    self.assertEqual(times.call_count, 2)
                else:
                    self.assertEqual(set(measured.call_args.args[5]), {('pinyin', 'empty'), ('pinyin', 'modest')})

    def test_changed_scoped_samples_diagnostics_or_files_block_only_their_case(self):
        for changed in ('samples', 'stderr', 'userfiles'):
            for stage in ('confirm', 'init-attribute'):
                with self.subTest(changed=changed, stage=stage):
                    status, _, measured, times = self.scoped_preflight(stage, changed)
                    self.assertEqual(status, 3)
                    if stage == 'confirm':
                        self.assertEqual(times.call_count, 1)
                    else:
                        self.assertEqual(set(measured.call_args.args[5]), {('pinyin', 'empty')})

    def test_full_export_sets(self):
        self.assertEqual(len(td.exports('pinyin')), 79)
        self.assertEqual(len(td.exports('zhuyin')), 52)

    def test_massif_boundaries_and_sampled_peak_are_separate(self):
        def snapshot(t, heap):
            return f'snapshot={t}\ntime={t}\nmem_heap_B={heap}\nmem_heap_extra_B=8\nmem_stacks_B=0\nheap_tree=empty\n'
        text = ''.join(f'TEARDOWN_HEAP {point} BEGIN\n{snapshot(t, heap)}TEARDOWN_HEAP {point} END\n'
                       for point, t, heap in (('M0', 0, 10), ('M1', 10, 100), ('M2', 20, 80), ('M3', 30, 20)))
        text += snapshot(5, 200) + snapshot(15, 120)
        result = td.massif_profile(text)
        self.assertEqual(result['freed_heap_bytes'], {'M1-M2': 20, 'M2-M3': 60})
        self.assertEqual(result['sampled_teardown_peak']['mem_heap_B'], 120)
        self.assertEqual(result['whole_lifecycle_peak']['mem_heap_B'], 200)
        with self.assertRaisesRegex(ValueError, 'missing Massif'):
            td.massif_profile(snapshot(1, 10))

    def test_memcheck_sites_keep_loss_classes_and_nonadditive_roots(self):
        text = ('TEARDOWN_LEAK M3 100 BEGIN\n==123== 1,024 bytes in 2 blocks are definitely lost in loss record 1 of 2\n'
                '==123==    at 0x123: malloc (malloc.c:1)\n==123==    by 0x234: db_create (db.c:2)\n==123== \n'
                '==123== 2,048 bytes in 4 blocks are still reachable in loss record 2 of 2\n'
                '==123==    at 0x123: malloc (malloc.c:1)\n==123== \nTEARDOWN_LEAK M3 100 END\n')
        sites = td.memcheck_sites(text, 'M3', 100)
        self.assertEqual([(site['bytes'], site['kind']) for site in sites], [(2048, 'still reachable'), (1024, 'definitely lost')])
        self.assertIn('db_create', sites[1]['frames'][1])

    def test_profile_semantics_retain_empty_rows_and_all_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            db = root / 'user_bigram.db'
            db.write_bytes(b'A' * 12 + b'\x62\x31\x05\x00' + b'arbitrary')
            (root / 'unexpected.tmp').write_bytes(b'kept')
            driver = SimpleNamespace(index_file_rows=lambda _: [['01', ''], ['02', '03']])
            a = td.profile_contents(driver, root)
            db.write_bytes(b'B' * 12 + b'\x62\x31\x05\x00' + b'changed metadata')
            b = td.profile_contents(driver, root)
            self.assertEqual(a, b)
            self.assertEqual(a['user_bigram.db']['rows'][0], ['01', ''])
            self.assertIn('unexpected.tmp', a)
            driver.index_file_rows = lambda _: [['01', ''], ['02', '04']]
            self.assertNotEqual(a, td.profile_contents(driver, root))


    def peer_driver(self, rows):
        return SimpleNamespace(USER_INDEX_FILES=('user_phrase_index.bin', 'user_pinyin_index.bin'),
                               zero_item_padding=lambda key, value: value,
                               run_worker=MagicMock(return_value=dict(exit=0, stderr_lines=0, result=dict(rows=rows))))

    def test_peer_profiles_compare_records_keep_empty_values_and_unknown_files(self):
        for cell in ('kc', 'tkrzw'):
            with self.subTest(cell=cell), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                names = ('user_phrase_index.bin', 'user_pinyin_index.bin', 'user_bigram.db')
                for name in (*names, 'unexpected.tmp'):
                    (root / name).write_bytes(b'first container bytes')
                driver = self.peer_driver([['01', ''], ['02', '03']])
                first = td.profile_contents(driver, root, library=Path('/subject.so'), cell=cell)
                self.assertEqual(first['user_bigram.db']['rows'], [['01', ''], ['02', '03']])
                self.assertEqual(first['user_bigram.db']['format'], cell + '-rows')
                self.assertIn('unexpected.tmp', first)
                for call in driver.run_worker.call_args_list:
                    mode, anchor, path, kind, scratch = call.args
                    self.assertEqual((mode, anchor, scratch), ('profile-rows', Path('/subject.so'), root.parent))
                    self.assertEqual(kind, 'hash' if path.name == 'user_bigram.db' else 'tree')
                    self.assertEqual(sorted(call.kwargs), ['extra_env', 'timeout'])
                    self.assertEqual(call.kwargs['timeout'], 120)
                    test_support.assert_same_environment(self, call.kwargs['extra_env'], dict(CONTRACT_DIFF_CELL=cell))
                for name in names:
                    (root / name).write_bytes(b'different layout and metadata')
                self.assertEqual(first, td.profile_contents(driver, root, library=Path('/subject.so'), cell=cell))
                driver.run_worker.return_value['result']['rows'] = [['01', ''], ['02', '04']]
                self.assertNotEqual(first, td.profile_contents(driver, root, library=Path('/subject.so'), cell=cell))

    def test_peer_profiles_reject_failed_reader_partial_or_malformed_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'user_bigram.db').write_bytes(b'container')
            for cell in ('kc', 'tkrzw'):
                driver = self.peer_driver([])
                with self.assertRaisesRegex(ValueError, 'runtime certification'):
                    td.profile_contents(driver, root, cell=cell)
                test_support.assert_not_called(self, driver.run_worker)
                # Empty database is distinct from an absent database.
                result = td.profile_contents(driver, root, library=Path('/subject.so'), cell=cell)
                self.assertIn('user_bigram.db', result)
                self.assertEqual(result['user_bigram.db']['rows'], [])
                for observation in (dict(exit=1, stderr_lines=0, result=dict(rows=[])),
                                    dict(exit=0, stderr_lines=1, result=dict(rows=[])),
                                    dict(exit=0, stderr_lines=0, result=None),
                                    dict(exit=0, stderr_lines=0, result=dict(rows=[], extra=True))):
                    with self.subTest(cell=cell, observation=observation):
                        driver.run_worker.return_value = observation
                        with self.assertRaisesRegex(ValueError, 'reader failed'):
                            td.profile_contents(driver, root, library=Path('/subject.so'), cell=cell)
                for rows in (None, [['01']], [['01', None]], [['01', '0']], [['01', 'zz']]):
                    driver.run_worker.return_value = dict(exit=0, stderr_lines=0, result=dict(rows=rows))
                    with self.subTest(cell=cell, rows=rows), self.assertRaisesRegex(ValueError, 'invalid profile row'):
                        td.profile_contents(driver, root, library=Path('/subject.so'), cell=cell)

    def test_peer_index_padding_and_only_registered_timestamp_bytes_are_normalized(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'user_pinyin_index.bin').write_bytes(b'container')
            (root / 'user.bin').write_bytes(b'headTIMEpayload')
            (root / 'custom.bin').write_bytes(b'headTIMEpayload')
            driver = self.peer_driver([['01', 'aabb']])
            driver.zero_item_padding = MagicMock(return_value='aa00')
            first = td.profile_contents(driver, root, library=Path('/subject.so'), cell='kc')
            self.assertEqual(first['user_pinyin_index.bin']['rows'], [['01', 'aa00']])
            driver.zero_item_padding.assert_called_once_with('01', 'aabb')
            (root / 'user.bin').write_bytes(b'headNEW!payload')
            self.assertEqual(first, td.profile_contents(driver, root, library=Path('/subject.so'), cell='kc'))
            (root / 'custom.bin').write_bytes(b'headNEW!payload')
            self.assertNotEqual(first, td.profile_contents(driver, root, library=Path('/subject.so'), cell='kc'))


class ColdTests(unittest.TestCase):
    def test_residency_labels_are_independent_of_the_sample_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ('facade.so', 'libdb.so.5', 'data/table.conf', 'sample-000001/user/sub/user.bin')
            for name in names:
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_bytes(b'x')
            with patch.object(td, 'resident_pages', return_value=dict(pages=1, resident=0)):
                rows = td.file_residency([root / name for name in names], root / 'facade.so',
                                         root / 'data', root / 'sample-000001/user')
            self.assertEqual(set(rows), {'facade', 'libdb.so.5', 'data/table.conf', 'user/sub/user.bin'})

    def test_dependency_closure_includes_non_c_runtimes_and_transitive_libraries(self):
        with tempfile.TemporaryDirectory() as directory:
            # The resolver returns resolved paths; macOS's /var is a symlink.
            root = Path(directory).resolve()
            paths = {name: root / name for name in ('facade.so', 'libstdc++.so.6', 'libdb.so.5',
                                                    'libglib-2.0.so.0', 'libpcre2.so.0', 'libgcc_s.so.1')}
            for path in paths.values():
                path.touch()
            needed = {'facade.so': ['libstdc++.so.6', 'libdb.so.5', 'libglib-2.0.so.0', 'libc.so.6'],
                      'libstdc++.so.6': ['libm.so.6', 'libgcc_s.so.1', 'libc.so.6'],
                      'libdb.so.5': ['libc.so.6'], 'libglib-2.0.so.0': ['libpcre2.so.0'],
                      'libpcre2.so.0': ['libc.so.6'], 'libgcc_s.so.1': ['libc.so.6']}
            def output(command, **kwargs):
                if command[0] == 'ldd':
                    return '\n'.join(f'{name} => {path} (0x1234)' for name, path in paths.items())
                return '\n'.join(f'0x1 (NEEDED) Shared library: [{name}]'
                                 for name in needed[Path(command[-1]).name])
            with patch.object(td.subprocess, 'check_output', side_effect=output):
                result = td.library_dependencies(paths['facade.so'])
            # libgcc_s is C runtime: the controller's interpreter maps it.
            self.assertEqual(set(result), set(paths.values()) - {paths['facade.so'], paths['libgcc_s.so.1']})
            with patch.object(td.subprocess, 'check_output', return_value='libdb.so.5 => not found'):
                with self.assertRaisesRegex(ValueError, 'unresolved cold dependency.*libdb'):
                    td.library_dependencies(paths['facade.so'])
            with patch.object(td.subprocess, 'check_output', side_effect=['', '(NEEDED) [libdb.so.5]']):
                with self.assertRaisesRegex(ValueError, 'unresolved cold DT_NEEDED.*libdb'):
                    td.library_dependencies(paths['facade.so'])

    def test_file_set_includes_facade_dependencies_data_and_private_user_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('data/table.conf', 'data/sub/custom.bin', 'user/user.conf', 'user/user.bin'):
                path = root / name
                path.parent.mkdir(exist_ok=True, parents=True)
                path.touch()
            with patch.object(td, 'library_dependencies', return_value=[root / 'libdb.so']):
                files = td.cold_files(root / 'facade.so', root / 'data', root / 'user')
            self.assertEqual(set(files), {root / name for name in ('facade.so', 'libdb.so', 'data/table.conf',
                                                                 'data/sub/custom.bin', 'user/user.conf', 'user/user.bin')})
            # A dirty scenario's rows file is read inside the lifecycle.
            (root / 'dirty.tsv').touch()
            with patch.object(td, 'library_dependencies', return_value=[]):
                files = td.cold_files(root / 'facade.so', root / 'data', root / 'user', root / 'dirty.tsv')
            self.assertIn(root / 'dirty.tsv', files)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux per-file eviction')
    def test_eviction_checks_each_file_before_after_and_before_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / name for name in ('facade.so', 'user.conf')]
            for path in paths:
                path.write_bytes(b'fixture')
            readings = [dict(pages=1, resident=n) for n in (1, 0, 1, 0, 0, 0)]
            with patch.object(td, 'filesystem', return_value='ext4'), \
                    patch.object(td, 'resident_pages', side_effect=readings) as residency, \
                    patch.object(td.os, 'posix_fadvise') as advise, patch.object(td, 'print_record') as report:
                records = td.evict_files(paths + paths)
            self.assertEqual(len(records), 2)
            self.assertEqual(residency.call_count, 6)
            self.assertEqual(advise.call_count, 2)
            self.assertTrue(all(call.args[1:] == (0, 0, os.POSIX_FADV_DONTNEED) for call in advise.call_args_list))
            self.assertEqual([row['before']['resident'] for row in records], [1, 1])
            self.assertTrue(all(row['after']['resident'] == row['verified']['resident'] == 0 for row in records))
            report.assert_called_once_with('cache-eviction', cache='cold', files=records)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux per-file eviction')
    def test_still_resident_and_refaulted_files_are_named_and_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'mapped-library.so'
            path.write_bytes(b'fixture')
            for readings in ((1, 1, 1), (1, 0, 1)):
                with self.subTest(readings=readings), patch.object(td, 'filesystem', return_value='ext4'), \
                        patch.object(td, 'resident_pages', side_effect=[dict(pages=1, resident=n) for n in readings]), \
                        patch.object(td.os, 'posix_fadvise'), patch.object(td, 'print_record') as report:
                    with self.assertRaisesRegex(ValueError, 'file stays resident: ' + str(path)):
                        td.evict_files([path])
                    self.assertEqual(report.call_args.kwargs['files'][0]['verified']['resident'], 1)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux per-file eviction')
    def test_non_ext4_file_refuses_entire_set_before_any_eviction(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / name for name in ('facade.so', 'tmpfs-profile')]
            for path in paths:
                path.touch()
            with patch.object(td, 'filesystem', side_effect=['ext4', 'tmpfs']), \
                    patch.object(td.os, 'posix_fadvise') as advise:
                with self.assertRaisesRegex(ValueError, 'not on ext4: ' + str(paths[1]) + '.*tmpfs'):
                    td.evict_files(paths)
                advise.assert_not_called()

    def test_cold_rejects_non_native_and_repeated_stages(self):
        pure = PureTests()
        with patch.object(td.sys, 'platform', 'darwin'):
            self.assertIn('native Linux', pure.refusal(teardown_cache='cold'))
        with patch.object(td.sys, 'platform', 'linux'):
            for stage in ('provision', 'repeat', 'multi', 'attribute'):
                self.assertIn('only verify, pilot, confirm and init-attribute',
                              pure.refusal(teardown_cache='cold', teardown_stage=stage))

    def test_cold_and_warm_execute_use_identical_rotating_sample_order(self):
        sequences = []
        for cache in ('warm', 'cold'):
            with tempfile.TemporaryDirectory() as directory:
                calls = []
                def sample(facade, library, **options):
                    calls.append((options['mode'], library))
                    return dict(observation=dict(samples=[], stderr='', userfiles={}, documents=[]), raw_profile_identity='stub')
                campaign = SimpleNamespace(work=Path(directory), cache=cache, run=sample)
                args = SimpleNamespace(teardown_stage='pilot')
                subjects = {side: {'pinyin': side} for side in ('ox', 'control', 'pin')}
                with patch.object(td, 'prepare_witnesses', return_value={'empty': None}), \
                        patch.object(td, 'print_record'), patch.object(td, 'report_times'), patch.object(td, 'report_memory'):
                    self.assertEqual(td.execute(campaign, args, subjects, {'empty': Path('/fixture')},
                                                ('empty',), ('pinyin',), None), 0)
                timed = [call for call in calls if call[0] in ('time', 'memory')]
                sequences.append(timed)
                for mode, rounds in (('time', 6), ('memory', 4)):
                    order = [side for kind, side in timed if kind == mode]
                    expected = [('ox', 'control', 'pin'), ('control', 'pin', 'ox'), ('pin', 'ox', 'control')]
                    self.assertEqual(order, [side for index in range(rounds) for side in expected[index % 3]])
        self.assertEqual(*sequences)


class PageResidencyTests(unittest.TestCase):
    """The per-page report (#1163) over fake mincore readings and the pages ELF fixture.

    A reading is data here, never taken from a page cache. The host page is 32
    bytes, so the 3,848-byte ELF fixture has 121 pages and the offsets that
    tools/dev/measure/pages.test.py checks against GNU readelf apply.
    """
    PAGE = 32
    ELF = ROOT / 'tools/dev/measure/fixtures/pages-elf-x86_64.so'
    ELF_PAGES = -(-3848 // PAGE)

    def setUp(self):
        patcher = patch.object(td.mman, 'PAGE_SIZE', self.PAGE)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def entry(self, resident, pages, kind, path='', size=None, identity=None):
        """One file's reading at a checkpoint, with the pages in RESIDENT set in its vector."""
        info = Path(path).stat() if kind == 'object' and Path(path).is_file() else None
        result = dict(pages=pages, resident=len(resident), kind=kind, path=str(path),
                      size=(info.st_size if info else pages * self.PAGE) if size is None else size,
                      identity=list(identity or ((info.st_dev, info.st_ino) if info else (7, 1))),
                      vector=bytes(index in resident for index in range(pages)))
        if info is not None:
            result['mtime_ns'] = info.st_mtime_ns
        return result

    def sample(self, *points):
        """A sample row from (name, residency) pairs, laid out as Campaign.run records them."""
        checkpoints = {0: {points[0][0]: dict(residency=points[0][1])},
                       1: {name: dict(residency=value) for name, value in points[1:]}}
        if len(points) > 2:
            # A memory sample's cycle 1 also holds a copy of the initial M0.
            checkpoints[1]['M0'] = checkpoints[0]['M0']
        return dict(checkpoints=checkpoints)

    def report(self, samples):
        """Print the report as the campaign does and return its records, parsed from the text."""
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            td.report_page_residency('pinyin', 'empty', 'ox', samples)
        lines = output.getvalue().splitlines()
        records = [json.loads(line) for line in lines]
        # The existing output: one sorted, ASCII JSON object per line, tagged with its kind.
        self.assertTrue(all(record.pop('teardown') == 'page-residency' for record in records))
        self.assertEqual(lines, [json.dumps(dict(teardown='page-residency', **record), sort_keys=True)
                                 for record in records])
        return records

    def test_resident_pages_derive_every_shape_from_one_vector(self):
        file = self.root / 'file'
        file.write_bytes(b'x')
        with file.open('rb') as handle, \
                patch.object(td.mman, 'vector', return_value=bytes([1, 0, 3, 0x80])) as read:
            fd = handle.fileno()
            self.assertEqual(td.resident_pages(fd), dict(pages=4, resident=2))
            self.assertEqual(td.resident_pages(fd, ranges=True),
                             dict(pages=4, resident=2, ranges=[[0, 1], [2, 3]]))
            self.assertEqual(td.resident_pages(fd, vector=True),
                             dict(pages=4, resident=2, vector=bytes([1, 0, 3, 0x80])))
            self.assertEqual(read.call_count, 3)
            read.return_value = b''
            # An empty file has no pages, so no runs, and an empty vector when asked for one.
            self.assertEqual(td.resident_pages(fd, ranges=True), dict(pages=0, resident=0))
            self.assertEqual(td.resident_pages(fd, vector=True), dict(pages=0, resident=0, vector=b''))

    def test_vectors_are_kept_from_the_one_read_that_counts_the_pages(self):
        names = ('facade.so', 'libdb.so.5', 'data/table.conf', 'sample/user/user.bin', 'dirty.tsv')
        for name in names:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            (self.root / name).write_bytes(b'x' * 40)
        files = [self.root / name for name in names]
        marks = [bytes([1, 0]), bytes([0, 1]), bytes([1, 1]), bytes([0, 0]), bytes([1, 0])]
        where = (self.root / 'facade.so', self.root / 'data', self.root / 'sample/user')
        with patch.object(td.mman, 'vector', side_effect=lambda fd: marks[(read.call_count - 1) % len(marks)]) as read:
            plain = td.file_residency(files, *where)
            kept = td.file_residency(files, *where, vectors=True, rows=self.root / 'dirty.tsv')
            by_role = td.residency(files, *where, vectors=True, rows=self.root / 'dirty.tsv')
        # Every call reads each file once; keeping the vector adds no second read.
        self.assertEqual(read.call_count, 15)
        self.assertEqual({label: set(row) for label, row in plain.items()}, {label: {'pages', 'resident'} for label in plain})
        kinds = ['object', 'object', 'data', 'data', 'data']
        self.assertEqual(list(kept), ['facade', 'libdb.so.5', 'data/table.conf', 'user/user.bin', 'dirty.tsv'])
        self.assertEqual(list(by_role), ['SUBJECT', 'LIB:libdb.so.5', 'DATA:table.conf', 'USER:user.bin',
                                         'LIB:dirty.tsv'])
        for readings in (kept, by_role):
            self.assertEqual([row['kind'] for row in readings.values()], kinds)
            self.assertEqual([row['vector'] for row in readings.values()], marks)
            self.assertEqual([row['resident'] for row in readings.values()], [1, 1, 2, 0, 1])
            self.assertEqual([row['size'] for row in readings.values()], [40] * 5)
            for path, row in zip(files, readings.values()):
                info = path.stat()
                self.assertEqual((row['path'], row['identity']), (str(path), [info.st_dev, info.st_ino]))
                self.assertEqual(row['mtime_ns'], info.st_mtime_ns)
        # The facade's own runs stay, for the init pages probe.
        self.assertEqual(by_role['SUBJECT']['ranges'], [[0, 1]])
        self.assertNotIn('ranges', by_role['LIB:libdb.so.5'])

    def test_checkpoint_sequence_is_the_order_the_worker_paused_in(self):
        init = self.sample(('M0', {}), ('I1', {}))
        memory = self.sample(('M0', {}), ('M1', {}), ('M2', {}), ('M3', {}))
        self.assertIn('M0', memory['checkpoints'][1])
        self.assertEqual([point for point, _ in td.checkpoint_sequence(init['checkpoints'])], ['M0', 'I1'])
        self.assertEqual([point for point, _ in td.checkpoint_sequence(memory['checkpoints'])],
                         ['M0', 'M1', 'M2', 'M3'])
        # The initial observation is cycle 0's, not the copy.
        observations = dict(td.checkpoint_sequence(memory['checkpoints']))
        self.assertIs(observations['M0'], memory['checkpoints'][0]['M0'])

    def test_an_init_pair_maps_object_pages_to_sections_and_symbols_and_data_pages_to_offsets(self):
        def round_(subject, data):
            return self.sample(
                ('M0', {'SUBJECT': self.entry({0}, self.ELF_PAGES, 'object', self.ELF),
                        'DATA:bigram.db': self.entry({0}, 7, 'data', size=224),
                        'LIB:libquiet.so': self.entry({0}, 3, 'object', self.ELF)}),
                ('I1', {'SUBJECT': self.entry({0, *subject}, self.ELF_PAGES, 'object', self.ELF),
                        'DATA:bigram.db': self.entry({0, *data}, 7, 'data', size=224),
                        'LIB:libquiet.so': self.entry({0}, 3, 'object', self.ELF)}))
        # Pages 34, 42 and 45 start at 0x440, 0x540 and 0x5a0; the second round drops 42 and 6 and adds 45.
        records = self.report([round_({34, 42}, {1, 2, 6}), round_({34, 45}, {1, 2})])
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0], dict(
            facade='pinyin', case='empty', side='ox', cache='cold', checkpoints=['M0', 'I1'], rounds=2,
            page_size=self.PAGE, limits=list(td.page_map.LIMITS), unchanged=['LIB:libquiet.so'], files={
                'DATA:bigram.db': dict(kind='data', pages=7, new_pages=td.summary([3, 2]), rows=[
                    dict(file='DATA:bigram.db', offset_range=[32, 96], in_rounds=2),
                    dict(file='DATA:bigram.db', offset_range=[192, 224], in_rounds=1)]),
                'SUBJECT': dict(kind='object', pages=self.ELF_PAGES, new_pages=td.summary([2, 2]), rows=[
                    dict(file='SUBJECT', page_offset=0x440, elf_sections=['.rodata'], elf_symbols=['pages_table'],
                         in_rounds=2),
                    dict(file='SUBJECT', page_offset=0x540, elf_sections=['.text'],
                         elf_symbols=['pages_first', 'pages_second', 'pages_third'], in_rounds=1),
                    dict(file='SUBJECT', page_offset=0x5a0, elf_sections=['.plt', '.tdata', '.dynamic'],
                         elf_symbols=['pages_tls'], in_rounds=1)])}))
        text = ' '.join(records[0]['limits'])
        self.assertIn('readahead', text)
        self.assertIn('pausing at a checkpoint', text)

    def test_a_memory_sample_reports_each_neighbouring_pair_and_never_a_skipped_one(self):
        def user(resident):
            return {'user/user.db': self.entry(resident, 5, 'data', size=150)}
        # Page 1 is new at M1 and stays; page 2 is new at M2; nothing is new at M3.
        sample = self.sample(('M0', user({0})), ('M1', user({0, 1})), ('M2', user({0, 1, 2})), ('M3', user({0, 1, 2})))
        records = self.report([sample])
        self.assertEqual([record['checkpoints'] for record in records], [['M0', 'M1'], ['M1', 'M2'], ['M2', 'M3']])
        self.assertEqual([record['files']['user/user.db']['rows'] if record['files'] else None for record in records],
                         [[dict(file='user/user.db', offset_range=[32, 64], in_rounds=1)],
                          [dict(file='user/user.db', offset_range=[64, 96], in_rounds=1)], None])
        self.assertEqual([record['unchanged'] for record in records], [[], [], ['user/user.db']])
        # The last page is clipped to the file's 150 bytes.
        sample = self.sample(('M0', user({0})), ('M1', user({0, 4})), ('M2', user({0, 4})), ('M3', user({0, 4})))
        self.assertEqual(self.report([sample])[0]['files']['user/user.db']['rows'],
                         [dict(file='user/user.db', offset_range=[128, 150], in_rounds=1)])

    def test_a_file_replaced_between_checkpoints_has_nothing_to_compare(self):
        def round_(identity, resident):
            return self.sample(('M0', {'USER:user.db': self.entry({0}, 4, 'data', identity=(7, 10))}),
                               ('I1', {'USER:user.db': self.entry(resident, 4, 'data', identity=identity)}))
        # Round 1 finds a new inode, whose pages are all resident because they were just written.
        shown = self.report([round_((7, 11), {0, 1, 2, 3}), round_((7, 10), {0, 2})])[0]['files']['USER:user.db']
        self.assertEqual(shown['replaced_in_rounds'], 1)
        self.assertEqual(shown['rows'], [dict(file='USER:user.db', offset_range=[64, 96], in_rounds=1)])
        self.assertEqual(shown['new_pages'], td.summary([1]))
        only = self.report([round_((7, 11), {0, 1, 2, 3})])[0]
        self.assertEqual(only['files']['USER:user.db'], dict(kind='data', pages=4, new_pages=None, rows=[],
                                                              replaced_in_rounds=1))
        self.assertEqual(only['unchanged'], [])

    def test_an_object_that_cannot_be_parsed_is_reported_by_byte_range_with_the_reason(self):
        text, missing = self.root / 'not-an-object.so', self.root / 'gone.so'
        text.write_bytes(b'tab\tseparated\n' * 8)
        sample = self.sample(
            ('M0', {'LIB:a': self.entry({0}, 4, 'object', text), 'LIB:b': self.entry({0}, 4, 'object', missing)}),
            ('I1', {'LIB:a': self.entry({0, 1}, 4, 'object', text), 'LIB:b': self.entry({0, 3}, 4, 'object', missing)}))
        files = self.report([sample])[0]['files']
        self.assertEqual(files['LIB:a']['rows'], [dict(file='LIB:a', offset_range=[32, 64], in_rounds=1)])
        self.assertTrue(files['LIB:a']['layout_error'].startswith(f'{text}: unknown object format'))
        self.assertEqual(files['LIB:b']['rows'], [dict(file='LIB:b', offset_range=[96, 128], in_rounds=1)])
        self.assertIn('No such file', files['LIB:b']['layout_error'])
        # A parsed object has no layout_error.
        sample = self.sample(('M0', {'SUBJECT': self.entry({0}, self.ELF_PAGES, 'object', self.ELF)}),
                             ('I1', {'SUBJECT': self.entry({0, 34}, self.ELF_PAGES, 'object', self.ELF)}))
        self.assertNotIn('layout_error', self.report([sample])[0]['files']['SUBJECT'])

    def test_each_object_is_parsed_once_per_report_and_only_when_a_page_is_new(self):
        def facade(resident):
            return {'SUBJECT': self.entry(resident, self.ELF_PAGES, 'object', self.ELF)}
        sample = self.sample(('M0', facade({0})), ('M1', facade({0, 34})), ('M2', facade({0, 34, 42})),
                             ('M3', facade({0, 34, 42})))
        with patch.object(td.page_map, 'object_layout', wraps=td.page_map.object_layout) as parse:
            self.report([sample, sample])
        # Two pairs have a new page, one has none; the object is parsed for the first and reused for the second.
        self.assertEqual(parse.call_count, 1)

    def test_a_valid_object_replacing_the_path_after_the_last_checkpoint_falls_back(self):
        path, replacement = self.root / 'subject.so', self.root / 'replacement.so'
        path.write_bytes(self.ELF.read_bytes())
        sample = self.sample(('M0', {'SUBJECT': self.entry({0}, self.ELF_PAGES, 'object', path)}),
                             ('I1', {'SUBJECT': self.entry({0, 34}, self.ELF_PAGES, 'object', path)}))
        replacement.write_bytes(self.ELF.read_bytes().replace(b'pages_table', b'other_table'))
        replacement.replace(path)
        # The replacement is a different valid ELF object, so a path-only parse would succeed.
        self.assertIn('other_table', [name for _, _, name in td.page_map.object_layout(path)['elf_symbols']])
        shown = self.report([sample])[0]['files']['SUBJECT']
        self.assertIn('replaced', shown['layout_error'])
        self.assertEqual(shown['rows'], [dict(file='SUBJECT', offset_range=[0x440, 0x460], in_rounds=1)])

    def test_same_inode_and_size_with_a_changed_mtime_falls_back(self):
        path = self.root / 'subject.so'
        path.write_bytes(self.ELF.read_bytes())
        sample = self.sample(('M0', {'SUBJECT': self.entry({0}, self.ELF_PAGES, 'object', path)}),
                             ('I1', {'SUBJECT': self.entry({0, 34}, self.ELF_PAGES, 'object', path)}))
        info = path.stat()
        path.write_bytes(self.ELF.read_bytes().replace(b'pages_table', b'other_table'))
        os.utime(path, ns=(info.st_atime_ns, info.st_mtime_ns + 1000000000))
        self.assertEqual((path.stat().st_dev, path.stat().st_ino, path.stat().st_size),
                         (info.st_dev, info.st_ino, info.st_size))
        self.assertIn('mtime changed', self.report([sample])[0]['files']['SUBJECT']['layout_error'])

    def test_parsing_uses_verified_descriptor_bytes_when_the_path_changes_again(self):
        path, replacement = self.root / 'subject.so', self.root / 'replacement.so'
        path.write_bytes(self.ELF.read_bytes())
        replacement.write_bytes(self.ELF.read_bytes().replace(b'pages_table', b'other_table'))
        sample = self.sample(('M0', {'SUBJECT': self.entry({0}, self.ELF_PAGES, 'object', path)}),
                             ('I1', {'SUBJECT': self.entry({0, 34}, self.ELF_PAGES, 'object', path)}))
        parse = td.page_map.object_layout

        def replace_then_parse(name, *, data=None):
            replacement.replace(path)
            return parse(name, data=data)

        with patch.object(td.page_map, 'object_layout', side_effect=replace_then_parse):
            shown = self.report([sample])[0]['files']['SUBJECT']
        self.assertNotIn('layout_error', shown)
        self.assertEqual(shown['rows'][0]['elf_symbols'], ['pages_table'])

    def test_replaced_objects_across_retained_rounds_fall_back(self):
        path, replacement = self.root / 'subject.so', self.root / 'replacement.so'
        path.write_bytes(self.ELF.read_bytes())

        def sample():
            return self.sample(('M0', {'SUBJECT': self.entry({0}, self.ELF_PAGES, 'object', path)}),
                               ('I1', {'SUBJECT': self.entry({0, 34}, self.ELF_PAGES, 'object', path)}))

        first = sample()
        replacement.write_bytes(self.ELF.read_bytes().replace(b'pages_table', b'other_table'))
        replacement.replace(path)
        shown = self.report([first, sample()])[0]['files']['SUBJECT']
        self.assertIn('replaced', shown['layout_error'])
        self.assertEqual(shown['rows'], [dict(file='SUBJECT', offset_range=[0x440, 0x460], in_rounds=2)])

    def test_samples_that_paused_at_different_checkpoints_are_refused(self):
        init = self.sample(('M0', {}), ('I1', {}))
        memory = self.sample(('M0', {}), ('M1', {}), ('M2', {}), ('M3', {}))
        with patch.object(td, 'print_record') as record:
            with self.assertRaisesRegex(ValueError, 'did not pause at the same checkpoints'):
                td.report_page_residency('pinyin', 'empty', 'ox', [init, memory])
        record.assert_not_called()


class InitAttributionTests(unittest.TestCase):
    TRACE = '\n'.join((
        '41 openat(AT_FDCWD</w>, "/data/table.conf", O_RDONLY) = 3</data/table.conf> <0.000010>',
        '41 write(7</dev/null>, "B", 1) = 1 <0.000004>',
        '41 openat(AT_FDCWD</w>, "/data/gb_char.bin", O_RDONLY|O_CLOEXEC) = 3</data/gb_char.bin> <0.000020>',
        '41 read(3</data/gb_char.bin>, "\\x01 = \\x02"..., 4096) = 4096 <0.000005>',
        '41 mmap(NULL, 8192, PROT_READ, MAP_PRIVATE, 3</data/gb_char.bin>, 0) = 0x7f0000000000 <0.000006>',
        '41 mmap(NULL, 4096, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0) = 0x7f0000002000 <0.000003>',
        '41 openat(AT_FDCWD</w>, "/user/user.conf", O_RDONLY) = -1 ENOENT (No such file or directory) <0.000007>',
        '41 read(3</data/gb_char.bin>,  <unfinished ...>',
        '41 <... read resumed>"", 4096) = 0 <0.000002>',
        '41 write(7</dev/null>, "E", 1) = 1 <0.000004>',
        '41 close(3</data/gb_char.bin>) = 0 <0.000002>'))

    def test_trace_window_excludes_markers_and_outside_calls(self):
        trace = td.init_trace(self.TRACE, 7, {'/data': '$DATA', '/user': '$USER'})
        self.assertTrue(trace['window'])
        self.assertEqual({name: value['count'] for name, value in trace['calls'].items()},
                         dict(openat=2, read=2, mmap=2))
        self.assertEqual(trace['calls']['openat']['errors'], 1)
        self.assertAlmostEqual(trace['total']['seconds'], 0.000043)
        self.assertEqual(trace['targets']['$DATA/gb_char.bin'],
                         dict(openat=dict(count=1, bytes=0), read=dict(count=2, bytes=4096),
                              mmap=dict(count=1, bytes=8192)))
        self.assertEqual(trace['targets']['anonymous'], dict(mmap=dict(count=1, bytes=4096)))
        self.assertIn('$USER/user.conf', trace['targets'])
        self.assertNotIn('close', trace['calls'])
        self.assertEqual((trace['unfinished'], trace['unparsed'], trace['threads']), (1, 0, 1))

    def test_resumed_calls_count_by_the_window_they_entered(self):
        trace = '\n'.join((
            '42 read(4</data/a.bin>,  <unfinished ...>',
            '41 write(7</dev/null>, "B", 1) = 1 <0.000004>',
            '42 <... read resumed>"", 16) = 0 <0.000002>',
            '41 openat(AT_FDCWD</w>, "/data/b.bin", O_RDONLY) = 3</data/b.bin> <0.000010>',
            '42 statx(AT_FDCWD</w>, "/data/c.bin", AT_STATX_SYNC_AS_STAT, STATX_ALL <unfinished ...>',
            '41 write(7</dev/null>, "E", 1) = 1 <0.000004>',
            '42 <... statx resumed>, {stx_mask=STATX_ALL, stx_size=8}) = 0 <0.000003>'))
        result = td.init_trace(trace, 7, {'/data': '$DATA'})
        self.assertTrue(result['window'])
        self.assertEqual({name: value['count'] for name, value in result['calls'].items()}, dict(openat=1, statx=1))
        self.assertEqual(result['targets']['$DATA/c.bin'], dict(statx=dict(count=1, bytes=0)))
        self.assertNotIn('$DATA/a.bin', result['targets'])
        self.assertEqual((result['unfinished'], result['unparsed'], result['threads']), (1, 0, 2))

    def test_trace_without_both_markers_is_not_a_window(self):
        self.assertFalse(td.init_trace(self.TRACE.split('\n41 write(7</dev/null>, "E"')[0], 7, {})['window'])

    def test_kernel_samples_take_the_innermost_decisive_frame(self):
        symbols = ([0x1000, 0x2000, 0x3000, 0x4000], ['clear_page_erms', 'exc_page_fault', '__x64_sys_read', 'do_syscall_64'])
        fault = dict(ip=0x1010, chain=[0x1010, 0x2010, 0x3010, 0x4010])
        self.assertEqual(td.kernel_category(fault, symbols), ('page-fault', 'clear_page_erms'))
        self.assertEqual(td.kernel_category(dict(ip=0x3010, chain=[0x4010]), symbols), ('syscall:read', '__x64_sys_read'))
        self.assertEqual(td.kernel_category(dict(ip=0x4010, chain=[]), symbols), ('syscall:entry-exit', 'do_syscall_64'))
        self.assertEqual(td.kernel_category(fault, None), ('unsymbolized', '0x1010'))
        maps = [(0x100, 0x200, 'r-xp', '/w/libraries/subject/libzhuyin.so'), (0x300, 0x400, 'r-xp', '/usr/lib/libc.so.6'),
                (0x500, 0x600, 'rw-p', '/w/libraries/subject/libzhuyin.so')]
        library = Path('/w/libraries/subject/libzhuyin.so')
        self.assertEqual([td.user_object(address, maps, library, Path('/w/worker')) for address in (0x150, 0x350, 0x500)],
                         ['SUBJECT', 'libc.so.6', 'unmapped'])

    def test_faults_take_the_mapping_that_held_the_address_at_the_time(self):
        library, data, user = Path('/w/lib/libzhuyin.so'), Path('/d'), Path('/w/sample-000001/user')
        before = [(0x1000, 0x2000, 'r-xp', str(library)), (0x3000, 0x4000, 'rw-p', '[heap]')]
        records = [dict(fault='minor', kernel=False, ip=0x1100, addr=0x1100),
                   dict(mmap=0x5000, len=0x2000, pgoff=0, prot=1, flags=2, file='/d/gb_char.bin'),
                   dict(fault='major', kernel=False, ip=0x1100, addr=0x5800),
                   dict(mmap=0x5000, len=0x1000, pgoff=0, prot=3, flags=0x22, file='//anon'),
                   dict(fault='minor', kernel=True, ip=0xffff0000, addr=0x5800),
                   dict(mmap=0x8000, len=0x1000, pgoff=0, prot=1, flags=2, file=str(user / 'user.conf')),
                   dict(fault='minor', kernel=False, ip=0x1100, addr=0x8000),
                   dict(fault='minor', kernel=False, ip=0x1100, addr=0x9000)]
        self.assertEqual([fault['region'] for fault in td.init_faults(records, before, library, data, user)],
                         ['SUBJECT:text', 'DATA:gb_char.bin:readonly', 'ANON:writable', 'USER:user.conf:readonly', 'unmapped'])

    def test_sample_report_names_categories_objects_and_functions(self):
        library = Path('/w/libraries/subject/libzhuyin.so')
        maps = [(0x1000, 0x2000, 'r--p', str(library)), (0x2000, 0x3000, 'r-xp', str(library)),
                (0x5000, 0x6000, 'r-xp', '/usr/lib/libc.so.6'), (0x7000, 0x8000, 'r-xp', '[vdso]')]
        symbols = ([0xff00], ['clear_page_erms'])
        report = dict(errno=0, lost=0, throttled=0, valid=True, user_only=False, samples=[
            dict(kernel=False, ip=0x2100, chain=[]), dict(kernel=False, ip=0x2900, chain=[]),
            dict(kernel=False, ip=0x5010, chain=[]), dict(kernel=False, ip=0x7010, chain=[]),
            dict(kernel=True, ip=0xff10, chain=[0xff10])])
        row = dict(init=dict(probe=report), checkpoints={1: dict(I1=dict(maps=maps))},
                   samples=[dict(times=dict(init_ns=dict(cpu_ns=1000)))])
        tables = {str(library): ([0x1100, 0x1800], ['init_tables', 'replay']), '/usr/lib/libc.so.6': ([0], ['memcpy'])}
        with patch.object(td, 'print_record') as record, patch.object(td, 'elf_symbols', side_effect=tables.get):
            td.report_init_samples('zhuyin', 'phrases', {side: [row] for side in ('ox', 'control', 'pin')}, symbols,
                                   {side: {'zhuyin': library} for side in ('ox', 'control', 'pin')},
                                   Path('/w/worker'), 'warm', 'sample-coarse')
        shown = record.call_args_list[0].kwargs
        self.assertEqual((shown['probe'], shown['period_ns'], shown['samples']), ('sample-coarse', 200000, 5))
        self.assertEqual(shown['user'], {'SUBJECT': 2, 'libc.so.6': 1, '[vdso]': 1})
        self.assertEqual(shown['kernel'], {'other': 1})
        self.assertEqual(shown['user_functions']['SUBJECT'], {'init_tables': 1, 'replay': 1})
        self.assertEqual(shown['user_functions']['[vdso]'], {'[vdso]': 1})

    def test_lackey_stream_keeps_only_accesses_between_markers(self):
        stream = (b'==1== Lackey\nI  00001000,3\n L 00002000,8\nBI  00003000,4\n S 00004030,8\n'
                  b' M 0000407e,4\nI  00003004,2\nEI  00005000,3\n L 00006000,8\n')
        for cut in range(1, len(stream)):
            accesses = td.LackeyAccesses()
            accesses.feed(stream[:cut])
            accesses.feed(stream[cut:])
            self.assertEqual(accesses.result(), dict(window=True, lines=4, code=[0x3000, 0x3004],
                                                     data_lines=[0x4030 >> 6, 0x407e >> 6, (0x4081 >> 6)]))
        accesses = td.LackeyAccesses()
        accesses.feed(stream[:stream.index(b'E')])
        self.assertFalse(accesses.result()['window'])

    def test_page_runs_round_trip(self):
        self.assertEqual(td.page_ranges([7, 3, 4, 5, 9]), [[3, 6], [7, 8], [9, 10]])
        self.assertEqual(td.range_pages([[3, 6], [9, 10]]), {3, 4, 5, 9})

    def init_pages(self, page_size, read_in):
        # Two local functions share a demangled name; a table lives in .rodata.
        library, base = Path('/w/libraries/subject/libzhuyin.so'), 0x7f0000000000
        layout = dict(sections=[dict(name='.text', address=0x10000, offset=0x10000, size=0x30000),
                                dict(name='.rodata', address=0x40000, offset=0x40000, size=0x10000)],
                      symbols=[(0x10000, 0x100, 't', 'helper'), (0x2f000, 0x2000, 't', 'helper'),
                               (0x40000, 0x40, 'r', 'TABLE')])
        maps = [(base, base + 0x50000, 'r-xp', str(library), 0)]
        accesses = dict(window=True, code=[base + 0x10010, base + 0x10020, base + 0x2f000],
                        data_lines=[(base + 0x40000) >> 6])
        rows = {'ox': [dict(init=dict(accesses=accesses), checkpoints=[dict(M0={}), dict(I1=dict(maps=maps))])]}
        bitmaps = {'ox': [dict(checkpoints=[dict(M0=dict(residency=dict(SUBJECT=dict(ranges=[])))),
                                            dict(I1=dict(residency=dict(SUBJECT=dict(ranges=read_in))))])]}
        with patch.object(td, 'library_layout', return_value=layout), \
                patch.object(td, 'policy_prefix', return_value=(0x10000, 0x30000)), \
                patch.object(td.os, 'sysconf', return_value=page_size), patch.object(td, 'print_record') as record:
            td.report_init_pages('zhuyin', 'phrases', rows, bitmaps, {'ox': {'zhuyin': library}}, 'cold')
        return record.call_args.kwargs

    def test_init_pages_count_host_pages(self):
        # 64 KiB pages: mincore indices and file offsets must use one size.
        shown = self.init_pages(0x10000, [[1, 3]])
        self.assertEqual(shown['page_size'], 0x10000)
        self.assertEqual(shown['sections'], {'.text': [1, 4], '.rodata': [4, 5]})
        self.assertEqual(shown['prefix']['pages'], [1, 3])
        self.assertEqual(shown['executed']['pages'], 2)
        self.assertEqual((shown['executed']['function_bytes'], shown['executed']['contiguous_pages']), (0x2100, 1))
        self.assertEqual((shown['read_in']['executed'], shown['read_in']['neither']), (2, 0))
        self.assertEqual([(page['page'], page['section'], page['executed'], page['symbols']) for page in shown['pages']],
                         [(1, '.text', True, ['helper']), (2, '.text', True, ['helper']),
                          (4, '.rodata', False, ['TABLE'])])

    def test_init_pages_keep_executed_functions_apart_by_address(self):
        shown = self.init_pages(0x1000, [[16, 17], [47, 48]])
        self.assertEqual(shown['executed']['functions'], 2)
        self.assertEqual((shown['executed']['function_bytes'], shown['executed']['contiguous_pages']), (0x2100, 3))
        self.assertEqual([(entry['function'], entry['instructions'], entry['pages'])
                          for entry in shown['executed']['largest']],
                         [('helper', 2, [[16, 17]]), ('helper', 1, [[47, 48]])])
        self.assertEqual(shown['read_in']['executed'], 2)

    CALLGRIND = '\n'.join(('events: Ir Bc', 'positions: line', 'ob=(1) /w/libraries/subject/libzhuyin.so',
                            'fn=(1) zhuyin_init', '3 10 2', 'cob=(2) /usr/lib/libc.so.6', 'cfn=(2) memcpy',
                            'calls=1 5', '5 100 20', '4 1', 'ob=(2)', 'fn=(2)', '5 100 20', 'totals: 111 22'))

    def test_callgrind_costs_skip_inclusive_calls_and_reconcile(self):
        costs = td.callgrind_costs(self.CALLGRIND, Path('/w/libraries/subject/libzhuyin.so'))
        self.assertEqual(costs['totals'], dict(Ir=111, Bc=22))
        self.assertEqual(costs['objects'], {'SUBJECT': dict(Ir=11, Bc=2), 'libc.so.6': dict(Ir=100, Bc=20)})
        self.assertEqual(costs['functions'][0], dict(object='libc.so.6', function='memcpy', Ir=100))
        with self.assertRaisesRegex(ValueError, 'do not reconcile'):
            td.callgrind_costs(self.CALLGRIND.replace('totals: 111', 'totals: 112'), Path('/x'))

    def test_inclusive_edges_do_not_enter_exclusive_totals(self):
        costs = td.callgrind_costs(self.CALLGRIND, Path('/w/libraries/subject/libzhuyin.so'))
        self.assertEqual(costs['inclusive_functions'][0],
                         dict(object='SUBJECT', function='zhuyin_init', Ir=111))
        self.assertEqual(costs['edges'], [dict(caller_object='SUBJECT', caller='zhuyin_init',
                         callee_object='libc.so.6', callee='memcpy', Ir=100)])
        self.assertEqual(sum(row['Ir'] for row in costs['functions']), costs['totals']['Ir'])

    def test_callgrind_callee_without_object_uses_callers_object(self):
        text = '\n'.join(('events: Ir', 'ob=/subject.so', 'fn=root', '1 2',
                          'cob=/libc.so', 'cfn=copy', 'calls=1 1', '1 3',
                          'cfn=local', 'calls=1 1', '1 5', 'fn=local', '1 5',
                          'ob=/libc.so', 'fn=copy', '1 3', 'totals: 10'))
        costs = td.callgrind_costs(text, Path('/subject.so'))
        self.assertIn(dict(caller_object='SUBJECT', caller='root', callee_object='SUBJECT',
                           callee='local', Ir=5), costs['edges'])
        self.assertEqual(costs['inclusive_functions'][0]['Ir'], 10)
        with self.assertRaisesRegex(ValueError, 'do not reconcile'):
            td.callgrind_costs(text + '\ncob=/subject.so\ncfn=root\ncalls=1 1', Path('/subject.so'))

    def test_profiler_summary_retains_full_tables_and_refuses_replacement(self):
        costs = td.callgrind_costs(self.CALLGRIND, Path('/subject.so'))
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            path = td.retain_profiler_summary(directory, 'sample-1', dict(profile=costs))
            self.assertEqual(json.loads(path.read_text()), dict(profile=costs))
            before = path.read_bytes()
            with self.assertRaises(FileExistsError):
                td.retain_profiler_summary(directory, 'sample-1', dict(profile={}))
            self.assertEqual(path.read_bytes(), before)
            raw = td.retain_profiler_profile(directory, 'sample-1', self.CALLGRIND)
            self.assertEqual(raw.read_text(), self.CALLGRIND)
            with self.assertRaises(FileExistsError):
                td.retain_profiler_profile(directory, 'sample-1', 'replacement')
            self.assertEqual(raw.read_text(), self.CALLGRIND)

    def test_init_ir_is_exact_off_bdb_and_diagnostic_on_bdb(self):
        def costs(*values):
            return [dict(totals=dict(Ir=value), objects={}, functions=[]) for value in values]
        drifting = dict(ox=costs(10, 10, 11), control=costs(10, 10, 10), pin=costs(20, 20, 20))
        for cell, invalid, sides in (('kc', True, ['control']), ('bdb', False, ['ox', 'control', 'pin'])):
            with self.subTest(cell=cell), patch.object(td, 'print_record') as record:
                self.assertEqual(td.report_init_ir(cell, 'zhuyin', 'phrases', drifting), invalid)
                verdicts = [call.kwargs['verdict'] for call in record.call_args_list if call.args[0] == 'init-ir-control']
                self.assertEqual(verdicts, ['INVALID' if invalid else 'DIAGNOSTIC'])
                self.assertEqual([call.kwargs['side'] for call in record.call_args_list if call.args[0] == 'init-ir'], sides)
        with patch.object(td, 'print_record'):
            self.assertFalse(td.report_init_ir('kc', 'zhuyin', 'phrases',
                                               dict(ox=costs(10, 10, 10), control=costs(10, 10, 10), pin=costs(20, 21, 20))))

    def test_first_candidate_window_requires_warm_attribute_stage(self):
        for fields in (dict(teardown_stage='confirm'),
                       dict(teardown_stage='init-attribute', teardown_cache='cold')):
            self.assertIn('requires the warm init-attribute stage',
                          PureTests().refusal(teardown_attribute_window='first-candidate', **fields))

    def test_first_candidate_ir_reports_ranges_without_single_baseline(self):
        def values(*counts):
            return [dict(totals=dict(Ir=n), objects={}, functions=[], inclusive_functions=[], edges=[])
                    for n in counts]
        costs = dict(ox=values(10, 11, 12), control=values(9, 11, 13), pin=values(5, 6, 5))
        with patch.object(td, 'print_record') as record:
            self.assertFalse(td.report_init_ir('kc', 'pinyin', 'phrases', costs,
                                               window='first-candidate'))
        control, *sides = record.call_args_list
        self.assertEqual(control.kwargs['verdict'], 'DIAGNOSTIC')
        self.assertEqual(sides[0].kwargs['Ir_min_max'], [10, 12])
        self.assertIsNone(sides[0].kwargs['totals'])

    def test_base_rotates_with_every_measurement_arm(self):
        arms = dict(base={}, ox={}, control={}, pin={})
        for index in range(4):
            self.assertEqual(td.rotation(index, arms),
                             list(arms)[index:] + list(arms)[:index])
        costs = {side: [dict(totals=dict(Ir=value), objects={}, functions=[])] * 3
                 for side, value in zip(arms, (100, 80, 80, 60))}
        with patch.object(td, 'print_record') as record:
            self.assertFalse(td.report_init_ir('bdb', 'pinyin', 'phrases', costs))
        self.assertEqual({call.kwargs['side'] for call in record.call_args_list[1:]}, set(arms))

    def test_probes_are_refused_outside_init_mode(self):
        campaign = td.Campaign(Path('/worker'), Path('/'), Path('/'), None, None)
        for mode in ('time', 'memory', 'observe'):
            with self.subTest(mode=mode), self.assertRaisesRegex(ValueError, 'require init mode'):
                campaign.run('pinyin', Path('/lib.so'), Path('/profile'), mode=mode, probe='count')
        with self.assertRaisesRegex(ValueError, 'unknown init probe'):
            campaign.run('pinyin', Path('/lib.so'), Path('/profile'), mode='init', probe='ebpf')

    def test_measured_samples_without_taskset_refuse_before_launch(self):
        for cache in ('warm', 'cold'):
            for mode in ('time', 'memory', 'init'):
                for cpu in (None, 3):
                    with self.subTest(cache=cache, mode=mode, cpu=cpu):
                        campaign = td.Campaign(Path('/worker'), Path('/'), Path('/'), cpu, None, cache=cache)
                        with patch.object(td.shutil, 'which', return_value=None), \
                                patch.object(td.subprocess, 'Popen') as launch, \
                                patch.object(td, 'evict_files') as evict:
                            with self.assertRaisesRegex(ValueError, 'measured teardown samples require taskset'):
                                campaign.run('pinyin', Path('/lib.so'), Path('/profile'), mode=mode)
                        test_support.assert_not_called(self, launch)
                        evict.assert_not_called()
                        self.assertEqual(campaign.serial, 0)


STUB = r'''
#define _POSIX_C_SOURCE 200809L
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
struct context { unsigned live; };
struct instance { struct context *context; };
void *fake_init(const char *data, const char *user) { (void)data; (void)user; fputs("INIT\n", stderr); return calloc(1, sizeof(struct context)); }
void fake_fini(void *p) { struct context *c=p; if (c->live) abort(); fputs("FINI\n", stderr); free(p); }
void *fake_alloc(void *p) { struct context *c=p; struct instance *i=malloc(sizeof *i); i->context=c; c->live++; fputs("ALLOC\n", stderr); return i; }
void fake_free(void *p) { struct instance *i=p; i->context->live--; fputs("FREE\n", stderr); free(i); }
bool fake_options(void *p, uint32_t flags) { return p && flags == 0x18a; }
size_t fake_parse(void *p, const char *input) { return p ? strlen(input) : 0; }
size_t fake_length(void *p) { return p ? 5 : 0; }
bool fake_guess(void *p, size_t offset, uint32_t sort) { return p && offset == 0 && sort == 0x1e; }
bool fake_after(void *p, size_t offset) { return p && offset == 0; }
bool fake_count(void *p, unsigned *count) { *count=1; return p != NULL; }
bool fake_candidate(void *p, unsigned index, void **out) { *out=p; return p && index == 0; }
bool fake_type(void *p, void *candidate, int *type) { *type=2; return p && candidate; }
bool fake_string(void *p, void *candidate, const char **text) { *text="你好"; return p && candidate; }
bool fake_nbest(void *p, void *candidate, uint8_t *index) { (void)p; (void)candidate; (void)index; abort(); }
bool fake_true(void *p) { return p != NULL; }
bool fake_save(void *p) { fputs("SAVE\n", stderr); return p != NULL; }
bool fake_load(void *p, uint8_t index) { return p && index > 0; }
void *fake_begin(void *p, uint8_t index) { (void)index; return p; }
bool fake_add(void *p, const char *s, const char *r, int n) { return p && s && r && n > 0; }
bool fake_zhuyin_add(void *p, const char *s, const char *r, int n) { return r && strcmp(r, "ㄘㄜˋ ㄕˋ") == 0 && fake_add(p, s, r, n); }
void fake_end(void *p) { (void)p; }
bool fake_sentence(void *p, uint8_t rank, char **text) { (void)rank; *text=strdup("你好"); return p != NULL; }
bool fake_zhuyin_sentence(void *p, char **text) { *text=strdup("你好"); return p != NULL; }
bool fake_has(void *p) { (void)p; return false; }
bool fake_next(void *p, char **s, char **r, int *n) { (void)p; (void)s; (void)r; (void)n; return false; }
void g_free(void *p) { free(p); }
'''


PERF_BASELINE = Path(__file__).with_name('teardown-perf-baseline.json')
PERF_CYCLES = 2
PERF_VOLATILE = ('init_ns', 'alloc_ns', 'rss_kib', 'hwm_kib', 'vm_size_kib',
                 'rss_anon_kib', 'rss_file_kib')


def build_stub(root):
    pairs = dict(init='init', fini='fini', alloc_instance='alloc', free_instance='free', set_options='options',
                 parse_more_full_pinyins='parse', parse_more_chewings='parse', reset='true',
                 guess_sentence='true', get_sentence='sentence', get_n_candidate='count',
                 get_candidate='candidate', get_candidate_type='type', get_candidate_string='string',
                 save='save', begin_add_phrases='begin', iterator_add_phrase='add', end_add_phrases='end',
                 load_phrase_library='load')
    signatures = {
        'init': ('void *', 'const char *data, const char *user', 'data, user'),
        'fini': ('void', 'void *p', 'p'),
        'alloc_instance': ('void *', 'void *p', 'p'),
        'free_instance': ('void', 'void *p', 'p'),
        'set_options': ('bool', 'void *p, uint32_t flags', 'p, flags'),
        'parse_more_full_pinyins': ('size_t', 'void *p, const char *input', 'p, input'),
        'parse_more_chewings': ('size_t', 'void *p, const char *input', 'p, input'),
        'reset': ('bool', 'void *p', 'p'),
        'guess_sentence': ('bool', 'void *p', 'p'),
        'get_sentence': ('bool', 'void *p, uint8_t rank, char **text', 'p, rank, text'),
        'get_n_candidate': ('bool', 'void *p, unsigned *count', 'p, count'),
        'get_candidate': ('bool', 'void *p, unsigned index, void **out', 'p, index, out'),
        'get_candidate_type': ('bool', 'void *p, void *candidate, int *type', 'p, candidate, type'),
        'get_candidate_string': ('bool', 'void *p, void *candidate, const char **text', 'p, candidate, text'),
        'save': ('bool', 'void *p', 'p'),
        'begin_add_phrases': ('void *', 'void *p, uint8_t index', 'p, index'),
        'iterator_add_phrase': ('bool', 'void *p, const char *s, const char *r, int n', 'p, s, r, n'),
        'end_add_phrases': ('void', 'void *p', 'p'),
        'load_phrase_library': ('bool', 'void *p, uint8_t index', 'p, index'),
        'guess_candidates': ('bool', 'void *p, size_t offset, uint32_t sort', 'p, offset, sort'),
        'get_parsed_input_length': ('size_t', 'void *p', 'p'),
        'get_candidate_nbest_index': ('bool', 'void *p, void *candidate, uint8_t *index', 'p, candidate, index'),
        'begin_get_phrases': ('void *', 'void *p, uint8_t index', 'p, index'),
        'iterator_has_next_phrase': ('bool', 'void *p', 'p'),
        'iterator_get_next_phrase': ('bool', 'void *p, char **s, char **r, int *n', 'p, s, r, n'),
        'end_get_phrases': ('void', 'void *p', 'p'),
        'guess_candidates_after_cursor': ('bool', 'void *p, size_t offset', 'p, offset'),
    }
    # Mach-O has no alias attribute, so export wrappers instead.
    overrides = {('zhuyin', 'get_sentence'): ('bool', 'void *p, char **text', 'p, text')}
    source = STUB
    for facade in ('pinyin', 'zhuyin'):
        names = dict(pairs)
        if facade == 'zhuyin':
            names['get_sentence'] = 'zhuyin_sentence'
            names['iterator_add_phrase'] = 'zhuyin_add'
        names.update(dict(guess_candidates='guess', get_parsed_input_length='length',
                          get_candidate_nbest_index='nbest', begin_get_phrases='begin',
                          iterator_has_next_phrase='has', iterator_get_next_phrase='next', end_get_phrases='end')
                     if facade == 'pinyin' else dict(guess_candidates_after_cursor='after'))
        for suffix, implementation in names.items():
            returns, parameters, arguments = overrides.get((facade, suffix), signatures[suffix])
            call = f'fake_{implementation}({arguments})'
            body = f'{call};' if returns == 'void' else f'return {call};'
            source += f'{returns} {facade}_{suffix}({parameters}) {{ {body} }}\n'
    cfile = root / 'stub.c'
    cfile.write_text(source)
    library = root / 'stub.so'
    link = ['-dynamiclib'] if sys.platform == 'darwin' else ['-shared']
    subprocess.run(['cc', *link, '-fPIC', '-Wall', '-Wextra', '-Werror', '-O2',
                    '-o', str(library), str(cfile)], check=True)
    return library


def perf_output(worker, library, systemdir):
    env = {key: value for key, value in os.environ.items() if not key.startswith('PERF_')}
    env.update(PERF_BACKEND='stub', PERF_MODE='speed', PERF_CYCLES=str(PERF_CYCLES), PERF_REPEATS='1')
    result = subprocess.run([str(worker), '--perf', str(library), str(systemdir)],
                            env=env, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


def perf_schema(value):
    if isinstance(value, dict):
        return {key: perf_schema(item) for key, item in value.items()}
    if isinstance(value, list):
        return [perf_schema(item) for item in value]
    return type(value).__name__


def perf_values(value, library, systemdir):
    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            if key == 'repeats':
                result[key] = item
            elif key in PERF_VOLATILE:
                result[key] = '<number>'
            else:
                result[key] = perf_values(item, library, systemdir)
        return result
    if isinstance(value, list):
        return [perf_values(item, library, systemdir) for item in value]
    if isinstance(value, str):
        return {str(library): '<so>', str(systemdir): '<systemdir>'}.get(value, value)
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return value
    return '<number>'


def perf_digest(schema, values):
    payload = json.dumps(dict(schema=schema, values=values), sort_keys=True, separators=(',', ':'))
    return td.digest_bytes(payload.encode())


def perf_observation(worker, library, systemdir):
    output = perf_output(worker, library, systemdir)
    schema = perf_schema(output)
    values = perf_values(output, library, systemdir)
    return dict(schema=schema, values=values, checksum=perf_digest(schema, values))


def write_perf_baseline():
    with tempfile.TemporaryDirectory(prefix='teardown-perf-baseline-') as directory:
        root = Path(directory)
        source = root / 'bisect-main.c'
        source.write_bytes(subprocess.check_output(
            ['git', 'show', '2ce6cb7d:tools/bisection/bisect.c'], cwd=td.ROOT))
        worker = root / 'bisect-main'
        subprocess.run(['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2',
                        '-o', str(worker), str(source), '-ldl'], check=True)
        library = build_stub(root)
        observation = perf_observation(worker, library, root)
        input_sha256 = td.digest_bytes((root / 'stub.c').read_bytes())
    fixture = dict(
        _comment=('Generated once from tools/bisection/bisect.c at main 2ce6cb7d '
                  '(pre-teardown) using the NativeTests stub. Regenerate with: '
                  'uv run --no-project --managed-python '
                  'python tools/bisection/teardown.test.py --write-perf-baseline'),
        input_sha256=input_sha256, **observation)
    PERF_BASELINE.write_text(json.dumps(fixture, indent=2, sort_keys=True) + '\n')


class NativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix='teardown-controls-')
        cls.root = Path(cls.temporary.name)
        cls.worker, _ = td.build_worker(cls.root)
        cls.library = build_stub(cls.root)
        cls.profile = cls.root / 'profile'
        cls.profile.mkdir()

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def setUp(self):
        which = td.shutil.which
        if not which('taskset'):
            # These native stubs test controller protocols, not Linux affinity.
            # A portable fake supplies the pinning tool's argv contract on macOS.
            tool = self.root / 'fake-taskset'
            tool.write_text('#!/bin/sh\nshift 2\nexec "$@"\n')
            tool.chmod(0o755)
            patcher = patch.object(td.shutil, 'which', side_effect=lambda name:
                                   str(tool) if name == 'taskset' else which(name))
            patcher.start()
            self.addCleanup(patcher.stop)

    def campaign(self, worker=None):
        spec = importlib.util.spec_from_file_location('contract_diff', td.HERE / 'contract-diff.py')
        driver = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(driver)
        if not sys.platform.startswith('linux'):
            # The runtime resolver reads ldd output, which exists only for
            # Linux ELF objects. These stub lifecycles are not loader fixtures;
            # tkrzw/runtime.test.py holds those.
            driver.check_tkrzw = lambda *args, **kwargs: None
        cpu = min(td.host.allowed_cpus())
        return td.Campaign(worker or self.worker, self.root, self.root, cpu, driver)

    def test_fixture_work_without_taskset_proceeds_unpinned(self):
        campaign = self.campaign()
        with patch.object(td.shutil, 'which', return_value=None), \
                patch.object(td, 'exports', return_value=['pinyin_init']), \
                patch.object(td.subprocess, 'Popen', wraps=td.subprocess.Popen) as launch:
            for mode in ('export', 'observe'):
                with self.subTest(mode=mode):
                    row = campaign.run('pinyin', self.library, self.profile, mode=mode)
                    self.assertEqual(row['command'][0], str(self.worker))
            rows = self.root / 'rows'
            rows.write_text("1\t测试\tce4'shi4\t20000\n")
            user = self.root / 'population-user'
            user.mkdir()
            td.populate_user(campaign, self.library, user, rows)
            self.assertEqual(launch.call_args.args[0][0], str(self.worker))

    def test_measured_sample_pins_to_the_reported_cpu(self):
        campaign = self.campaign()
        with patch.object(td, 'exports', return_value=['pinyin_init']):
            row = campaign.run('pinyin', self.library, self.profile)
        self.assertEqual(row['command'][:3], [td.shutil.which('taskset'), '-c', str(campaign.cpu)])

    def test_population_accepts_user_library_and_refuses_unrequested_libraries(self):
        campaign = self.campaign()
        rows = self.root / 'user-rows.tsv'
        with patch.object(td, 'exports', return_value=['pinyin_init']):
            for library in (1, 4, 7):
                rows.write_text(f"{library}\t测试\tce4'shi4\t5\n")
                td.populate_user(campaign, self.library, self.profile, rows)
            for library in (0, 5, 6, 8):
                rows.write_text(f"{library}\t测试\tce4'shi4\t5\n")
                with self.assertRaisesRegex(ValueError, 'population failed'):
                    td.populate_user(campaign, self.library, self.profile, rows)

    def require_eviction(self, directory):
        """Fail, naming the file system, where fadvise cannot evict the directory's files.

        The cold controls patch evict_files' ext4 check and evict on the
        temporary directory's real file system. tmpfs and ramfs keep a file's
        pages as its only storage and ignore fadvise (mm/fadvise.c), so there the
        pages would stay resident and evict_files would blame a mapping for it.
        """
        kind = td.filesystem(directory)
        if kind in ('tmpfs', 'ramfs'):
            self.fail(f'unsupported temporary file system: {directory} is on {kind}, which ignores '
                      'POSIX_FADV_DONTNEED; set TMPDIR to a directory on a disk-backed file system')

    @unittest.skipUnless(sys.platform == 'linux', 'Linux mincore and fadvise')
    def test_native_private_file_eviction_and_mapped_file_refusal(self):
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            self.require_eviction(directory)
            path = Path(directory) / 'private-cold-input'
            path.write_bytes(b'x' * (os.sysconf('SC_PAGE_SIZE') + 1))
            with path.open('rb') as handle:
                self.assertEqual(td.resident_pages(handle.fileno()), dict(pages=2, resident=2))
            # The ext4 refusal has its own test. This one exercises the real
            # fadvise and mincore on the temporary directory's file system,
            # which need not be ext4: overlay, for one, passes both to the
            # real inode. require_eviction refuses the ones that cannot evict.
            with patch.object(td, 'print_record'), patch.object(td, 'filesystem', return_value='ext4'):
                records = td.evict_files([path])
            self.assertEqual(records[0]['after'], dict(pages=2, resident=0))
            with path.open('rb') as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as mapping:
                self.assertEqual(mapping[0], ord('x'))
                with patch.object(td, 'print_record'), patch.object(td, 'filesystem', return_value='ext4'), \
                        self.assertRaisesRegex(ValueError, 'file stays resident: ' + str(path)):
                    td.evict_files([path])

    @unittest.skipUnless(sys.platform == 'linux', 'Linux cold native stub samples')
    def test_cold_native_samples_evict_only_private_inputs_for_both_facades_and_all_arms(self):
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            self.require_eviction(directory)
            root = Path(directory)
            data, profile = root / 'data', root / 'profile'
            data.mkdir()
            profile.mkdir()
            (data / 'table.conf').write_bytes(b'private-system-fixture')
            (profile / 'user.conf').write_bytes(b'private-profile-fixture')
            for side in ('subject', 'control', 'pinlibs'):
                library = root / (side + '.so')
                library.write_bytes(self.library.read_bytes())
                # The native stub needs only libc. Inspect it for real, without
                # advising any host library or another lane's measurement input.
                self.assertEqual(td.library_dependencies(library), [])
                for facade in ('pinyin', 'zhuyin'):
                    campaign = self.campaign()
                    campaign.data, campaign.work, campaign.cache = data, root, 'cold'
                    # As above: the ext4 refusal is tested on its own.
                    with patch.object(td, 'exports', return_value=[facade + '_init']), \
                            patch.object(td, 'filesystem', return_value='ext4'), \
                            patch.object(td, 'print_record') as report:
                        row = campaign.run(facade, library, profile)
                    self.assertEqual(row['samples'][0]['observations'][0]['candidates'][0]['text'], '你好')
                    records = report.call_args.kwargs['files']
                    paths = {Path(record['path']) for record in records}
                    self.assertEqual(len(paths), 3)
                    self.assertIn(library, paths)
                    self.assertIn(data / 'table.conf', paths)
                    self.assertTrue(any(path.name == 'user.conf' and path.parent.name == 'user' for path in paths))
                    self.assertNotIn(profile / 'user.conf', paths)
                    self.assertTrue(all(record['verified']['resident'] == 0 for record in records))
                    self.assertFalse(list(root.glob('sample-*')))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux cold native stub samples')
    def test_cold_memory_samples_report_per_file_residency_at_each_checkpoint(self):
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            self.require_eviction(directory)
            root = Path(directory)
            data, profile = root / 'data', root / 'profile'
            data.mkdir()
            profile.mkdir()
            (data / 'table.conf').write_bytes(b'private-system-fixture')
            (profile / 'user.conf').write_bytes(b'private-profile-fixture')
            library = root / 'subject.so'
            library.write_bytes(self.library.read_bytes())
            campaign = self.campaign()
            campaign.data, campaign.work, campaign.cache = data, root, 'cold'
            # The ext4 refusal is tested on its own. Objects are parsed when the
            # rounds are over, never inside a sample.
            with patch.object(td, 'exports', return_value=['pinyin_init']), \
                    patch.object(td, 'filesystem', return_value='ext4'), patch.object(td, 'print_record'), \
                    patch.object(td.page_map, 'object_layout', side_effect=AssertionError('parsed in a sample')):
                rows = [campaign.run('pinyin', library, profile, mode='memory') for _ in range(2)]
            for row in rows:
                points = dict(row['checkpoints'][1], M0=row['checkpoints'][0]['M0'])
                for point in ('M0', 'M1', 'M2', 'M3'):
                    self.assertEqual(set(points[point]['residency']), {'facade', 'data/table.conf', 'user/user.conf'})
                    # Each reading keeps its vector, one byte per page, and counts from it.
                    for label, reading in points[point]['residency'].items():
                        self.assertEqual(len(reading['vector']), reading['pages'], label)
                        self.assertEqual(sum(value & 1 for value in reading['vector']), reading['resident'], label)
                    self.assertEqual({label: reading['kind'] for label, reading in points[point]['residency'].items()},
                                     {'facade': 'object', 'data/table.conf': 'data', 'user/user.conf': 'data'})
                # Loading faults the facade in before M0; nothing evicts it after.
                self.assertGreater(points['M0']['residency']['facade']['resident'], 0)
            with patch.object(td, 'print_record') as record:
                td.report_memory('pinyin', 'empty', {side: rows for side in ('ox', 'control', 'pin')}, True)
            residency = [call.kwargs for call in record.call_args_list if call.args[0] == 'residency']
            self.assertEqual([value['side'] for value in residency], ['ox', 'control', 'pin'])
            self.assertEqual(residency[0]['files']['data/table.conf']['pages']['median'], 1)
            self.assertEqual(set(residency[0]['files']['facade']), {'pages', 'M0', 'M1', 'M2', 'M3'})
            # One page-residency record per neighbouring pair for each side, after that side's residency.
            pairs = [(call.kwargs['side'], call.kwargs['checkpoints'])
                     for call in record.call_args_list if call.args[0] == 'page-residency']
            self.assertEqual(pairs, [(side, pair) for side in ('ox', 'control', 'pin')
                                     for pair in (['M0', 'M1'], ['M1', 'M2'], ['M2', 'M3'])])
            for call in record.call_args_list:
                if call.args[0] == 'page-residency':
                    # Every cold file is either named by a page or listed as unchanged.
                    self.assertEqual(sorted(call.kwargs['files']) + call.kwargs['unchanged'],
                                     sorted(['facade', 'data/table.conf', 'user/user.conf']))
        # Warm samples carry no residency and print no residency record.
        with patch.object(td, 'exports', return_value=['pinyin_init']):
            warm = self.campaign().run('pinyin', self.library, self.profile, mode='memory')
        self.assertNotIn('residency', warm['checkpoints'][0]['M0'])

    @unittest.skipUnless(sys.platform == 'linux', 'Linux cold native stub samples')
    def test_cold_init_samples_report_page_residency_from_load_to_init(self):
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            self.require_eviction(directory)
            root = Path(directory)
            data, profile = root / 'data', root / 'profile'
            data.mkdir()
            profile.mkdir()
            (data / 'table.conf').write_bytes(b'private-system-fixture')
            (profile / 'user.conf').write_bytes(b'private-profile-fixture')
            library = root / 'subject.so'
            library.write_bytes(self.library.read_bytes())
            campaign = self.campaign()
            campaign.data, campaign.work, campaign.cache = data, root, 'cold'
            with patch.object(td, 'exports', return_value=['pinyin_init']), \
                    patch.object(td, 'filesystem', return_value='ext4'), patch.object(td, 'print_record'):
                rows = [campaign.run('pinyin', library, profile, mode='init', probe='count') for _ in range(2)]
            for row in rows:
                for point, cycle in (('M0', 0), ('I1', 1)):
                    reading = row['checkpoints'][cycle][point]['residency']['SUBJECT']
                    self.assertEqual((len(reading['vector']), reading['kind']), (reading['pages'], 'object'))
            with patch.object(td, 'print_record') as record:
                td.report_init_counts('pinyin', 'empty', {side: rows for side in ('ox', 'control', 'pin')}, 'cold')
                td.report_init_counts('pinyin', 'empty', {side: rows for side in ('ox', 'control', 'pin')}, 'warm')
            shown = [call.kwargs for call in record.call_args_list if call.args[0] == 'page-residency']
            # Only the cold call reports pages; the init pair is M0 to I1.
            self.assertEqual([(value['side'], value['checkpoints'], value['rounds']) for value in shown],
                             [(side, ['M0', 'I1'], 2) for side in ('ox', 'control', 'pin')])
            self.assertEqual(sorted(shown[0]['files']) + shown[0]['unchanged'],
                             sorted(['SUBJECT', 'DATA:table.conf', 'USER:user.conf']))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux mincore and fadvise')
    def test_a_real_vector_shows_a_read_page_and_its_readahead_as_newly_resident(self):
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            self.require_eviction(directory)
            page = os.sysconf('SC_PAGE_SIZE')
            path = Path(directory) / 'database'
            path.write_bytes(os.urandom(64 * page))
            with patch.object(td, 'filesystem', return_value='ext4'), patch.object(td, 'print_record'):
                td.evict_files([path])
            where = (Path(directory) / 'facade.so', Path(directory), Path(directory) / 'user')
            before = td.file_residency([path], *where, vectors=True)
            self.assertEqual(before['data/database']['resident'], 0)
            with path.open('rb') as handle:
                os.pread(handle.fileno(), 1, 40 * page)
                after = td.file_residency([path], *where, vectors=True)
            newly = td.page_map.newly_resident(before['data/database']['vector'], after['data/database']['vector'], page)
            # The page that was read is newly resident; the kernel may have read more around it,
            # which a difference of two vectors cannot tell from the page that was faulted.
            self.assertIn(40 * page, newly)
            self.assertEqual(len(newly), after['data/database']['resident'])
            self.assertLessEqual(len(newly), 64)
            rows = td.page_map.data_tally_rows(td.page_map.tally([newly]), page, 64 * page, 'data/database')
            self.assertTrue(any(start <= 40 * page < end for start, end in (row['offset_range'] for row in rows)))
            self.assertEqual(after['data/database']['kind'], 'data')

    def test_cold_prepares_every_arm_after_copy_and_before_child_and_cleans_refusal(self):
        for side in ('subject', 'control', 'pinlibs'):
            with self.subTest(side=side):
                campaign = self.campaign()
                campaign.cache = 'cold'
                library = self.root / side / 'facade.so'
                paths = []
                def files(measured, data, user, rows):
                    self.assertEqual(measured, library)
                    self.assertTrue(user.is_dir())
                    self.assertNotEqual(user, self.profile)
                    paths.extend([measured, data / 'system.bin', user / 'user.conf'])
                    return paths
                with patch.object(campaign.driver, 'check_tkrzw'), \
                        patch.object(td, 'cold_files', side_effect=files), \
                        patch.object(td, 'evict_files', side_effect=ValueError('file stays resident: private-stub')) as eviction, \
                        patch.object(td.subprocess, 'Popen') as child:
                    with self.assertRaisesRegex(ValueError, 'file stays resident: private-stub'):
                        campaign.run('pinyin', library, self.profile)
                    eviction.assert_called_once_with(paths)
                    test_support.assert_not_called(self, child)
                self.assertFalse(list(self.root.glob('sample-*')))

    def test_cold_eviction_precedes_successful_stub_launch_and_warm_does_not_evict(self):
        campaign = self.campaign()
        for cache in ('warm', 'cold'):
            campaign.cache = cache
            events = []
            popen = td.subprocess.Popen
            def launch(*args, **kwargs):
                events.append('launch')
                return popen(*args, **kwargs)
            with patch.object(campaign.driver, 'check_tkrzw'), \
                    patch.object(td, 'exports', return_value=['pinyin_init']), \
                    patch.object(td, 'cold_files', return_value=[self.library]), \
                    patch.object(td, 'evict_files', side_effect=lambda _: events.append('evict')), \
                    patch.object(td.subprocess, 'Popen', side_effect=launch):
                campaign.run('pinyin', self.library, self.profile)
            self.assertEqual(events, ['evict', 'launch'] if cache == 'cold' else ['launch'])

    @unittest.skipUnless(sys.platform == 'linux', 'Linux mincore and perf_event_open')
    def test_init_mode_pauses_after_load_and_init_and_reports_each_probe(self):
        for probe in (None, 'count', 'sample', 'sample-coarse', 'faults'):
            with self.subTest(probe=probe), patch.object(td, 'exports', return_value=['pinyin_init']):
                row = self.campaign().run('pinyin', self.library, self.profile, mode='init', probe=probe)
                self.assertEqual((set(row['checkpoints'][0]), set(row['checkpoints'][1])), ({'M0'}, {'I1'}))
                after = row['checkpoints'][1]['I1']
                subject = after['residency']['SUBJECT']
                self.assertTrue(any(path == str(self.library) and 'x' in perms for _, _, perms, path, _ in after['maps']))
                # Warm: the page cache holds every page the loader mapped from the stub, not every page of
                # the file. A linker that aligns segments to 64 KiB (GNU ld on arm64) leaves a hole that no
                # segment maps; a hole page is cached only if something read the file, and never on tmpfs.
                page = os.sysconf('SC_PAGE_SIZE')
                mapped = {index for start, end, perms, path, offset in after['maps']
                          if path == str(self.library) and perms[:3] != '---'
                          for index in range(offset // page, (offset + end - start) // page)}
                resident = td.range_pages(subject['ranges'])
                self.assertEqual(subject['pages'], -(-self.library.stat().st_size // page))
                self.assertEqual(len(resident), subject['resident'])
                self.assertEqual(mapped - resident, set())
                self.assertEqual(set(row['samples'][0]['times']['init_ns']), set(td.CLOCK_FIELDS))
                self.assertEqual(row['observation']['stderr'].splitlines(), ['INIT', 'ALLOC', 'FREE', 'FINI'])
                self.assertEqual(row['observation']['documents'], [])
                report = row['init']['probe']
                if probe is None:
                    self.assertIsNone(report)
                elif probe == 'count':
                    self.assertEqual([event['name'] for event in report['events']][:4],
                                     ['task-clock', 'page-faults', 'minor-faults', 'major-faults'])
                    for event in report['events']:
                        # A refused event names its errno and carries no value.
                        self.assertEqual('value' in event, not event['errno'])
                elif probe.startswith('sample'):
                    self.assertEqual(report['period_ns'], td.INIT_SAMPLE_PERIOD_NS[probe])
                    self.assertTrue(report['errno'] or report['valid'])
                else:
                    self.assertTrue(report['errno'] or report['valid'])
                    if not report['errno']:
                        # fake_init's calloc and stderr write fault at least once.
                        self.assertTrue(any(value.get('fault') == 'minor' for value in report['samples']))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux worker probes')
    def test_first_candidate_markers_exclude_init_and_allocation(self):
        read, write = os.pipe()
        env = dict(os.environ, OXPINYIN_TEARDOWN_EXPORTS='pinyin_init',
                   OXPINYIN_TEARDOWN_INIT_PROBE='trace',
                   OXPINYIN_TEARDOWN_PROBE_WINDOW='first-candidate',
                   OXPINYIN_TEARDOWN_MARKER_FD=str(write))
        try:
            child = subprocess.run([str(self.worker), '--teardown', 'pinyin', str(self.library),
                                    str(self.root), str(self.profile), 'init', '0', '1', '1',
                                    'first-candidate', '-'], input=b'\n' * 3, stdout=subprocess.PIPE,
                                   stderr=write, env=env, pass_fds=(write,), timeout=20)
        finally:
            os.close(write)
        try:
            trace = os.read(read, 65536)
        finally:
            os.close(read)
        self.assertEqual(child.returncode, 0, trace)
        self.assertEqual(trace, b'INIT\nALLOC\nBEFREE\nFINI\n')
        rows = [json.loads(line) for line in child.stdout.splitlines()]
        self.assertEqual([(row['checkpoint'], row['cycle']) for row in rows if 'checkpoint' in row],
                         [('M0', 0), ('I1', 1)])
        sample = next(row for row in rows if row.get('teardown') == 'sample')
        self.assertEqual(set(sample['times']['first_candidate_ns']), set(td.CLOCK_FIELDS))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux worker probes')
    def test_trace_probe_writes_begin_and_end_markers_around_init_only(self):
        read, write = os.pipe()
        env = dict(os.environ, OXPINYIN_TEARDOWN_EXPORTS='pinyin_init', OXPINYIN_TEARDOWN_INIT_PROBE='trace',
                   OXPINYIN_TEARDOWN_MARKER_FD=str(write))
        command = [str(self.worker), '--teardown', 'pinyin', str(self.library), str(self.root), str(self.profile),
                   'init', '0', '1', '1', 'first-candidate', '-']
        with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              env=env, pass_fds=(write,)) as process:
            os.close(write)
            stdout, stderr = process.communicate(b'\n\n\n', timeout=60)
        with os.fdopen(read, 'rb') as markers:
            self.assertEqual(markers.read(), b'BE')
        self.assertEqual(process.returncode, 0, stderr)
        points = [json.loads(line).get('checkpoint') for line in stdout.splitlines()]
        self.assertEqual([point for point in points if point], ['M0', 'I1'])
        for mode in ('time', 'memory'):
            with self.subTest(mode=mode):
                refused = subprocess.run([*command[:6], mode, *command[7:]], env=env, stdin=subprocess.DEVNULL,
                                         capture_output=True, text=True, timeout=60)
                self.assertEqual(refused.returncode, 2)
                self.assertIn('require init mode', refused.stderr)

    def test_report_times_records_major_faults_per_phase_without_ratio(self):
        def row(load, init):
            times = {'load_ns': dict(wall_ns=10, cpu_ns=5, majflt=load),
                     'init_ns': dict(wall_ns=20, cpu_ns=9, majflt=init)}
            return {'samples': [{'times': times}]}
        samples = {side: [row(index, 2 * index) for index in range(1, 6)] for side in ('ox', 'control', 'pin')}
        with patch.object(td, 'print_record') as record:
            td.report_times('pinyin', 'phrases', samples, True)
        faults = [call.kwargs for call in record.call_args_list if call.args[0] == 'faults']
        self.assertEqual([value['metric'] for value in faults], ['load_ns', 'init_ns'])
        self.assertEqual(faults[1]['summaries']['pin']['median'], 6)
        self.assertTrue(all('ox_pin_ratio' not in value for value in faults))

    def test_both_facades_and_teardown_order(self):
        for facade in ('pinyin', 'zhuyin'):
            with self.subTest(facade=facade), patch.object(td, 'exports', return_value=[facade + '_init']):
                row = self.campaign().run(facade, self.library, self.profile)
                self.assertEqual(row['observation']['stderr'].splitlines(), ['INIT', 'ALLOC', 'FREE', 'FINI'])
                self.assertEqual(row['samples'][0]['observations'][0]['candidates'][0]['text'], '你好')
                self.assertNotIn('save_ns', row['samples'][0]['times'])
                self.assertIn('dlclose_ns', row['samples'][0]['times'])

    def test_observation_uses_each_facades_sentence_signature(self):
        for facade in ('pinyin', 'zhuyin'):
            with self.subTest(facade=facade), patch.object(td, 'exports', return_value=[facade + '_init']):
                row = self.campaign().run(facade, self.library, self.profile, mode='observe')
                documents = row['observation']['documents']
                self.assertEqual(len(documents), 21 if facade == 'zhuyin' else 18)
                self.assertTrue(all(value['sentence_read'] and value['sentence'] == '你好' for value in documents))

    @unittest.skipUnless(sys.platform.startswith('linux'), '/proc memory readers and glibc allocator counters')
    def test_parent_checkpoints_and_surviving_instance(self):
        for facade in ('pinyin', 'zhuyin'):
            with self.subTest(facade=facade), patch.object(td, 'exports', return_value=[facade + '_init']):
                row = self.campaign().run(facade, self.library, self.profile, mode='memory', instances=2, scenario='multi')
                self.assertEqual(set(row['checkpoints'][1]), {'M0', 'M1', 'M2', 'M3'})
                self.assertEqual(row['observation']['stderr'].splitlines(), ['INIT', 'ALLOC', 'ALLOC', 'FREE', 'FREE', 'FINI'])
                self.assertEqual(len(row['samples'][0]['free_instance_ns']), 2)

    @unittest.skipUnless(sys.platform.startswith('linux'), '/proc memory readers and glibc allocator counters')
    def test_repeated_lifecycle_resets_profile_and_keeps_library_loaded(self):
        with patch.object(td, 'exports', return_value=['pinyin_init']):
            row = self.campaign().run('pinyin', self.library, self.profile, mode='memory', cycles=10)
        self.assertEqual(len(row['samples']), 10)
        self.assertEqual(set(row['checkpoints']), {0, 1, 10})
        self.assertEqual(td.file_set(self.profile), {})

    def test_save_is_named_and_precedes_every_free(self):
        rows = self.root / 'dirty.tsv'
        for facade in ('pinyin', 'zhuyin'):
            with self.subTest(facade=facade), patch.object(td, 'exports', return_value=[facade + '_init']):
                reading = "ce4'shi4" if facade == 'pinyin' else 'ㄘㄜˋ ㄕˋ'
                rows.write_text(td.row_text([(1, '测试', reading, 20000)]))
                row = self.campaign().run(facade, self.library, self.profile, scenario='dirty', save=True, rows=rows)
                self.assertEqual(row['observation']['stderr'].splitlines(), ['INIT', 'ALLOC', 'SAVE', 'FREE', 'FINI'])
                self.assertTrue(row['samples'][0]['saved'])
                self.assertIn('save_ns', row['samples'][0]['times'])
                self.assertIn('full_lifecycle_ns', row['samples'][0]['times'])
                times = row['samples'][0]['times']
                self.assertTrue(all(isinstance(value['majflt'], int) and value['majflt'] >= 0
                                    for value in times.values()))
                self.assertEqual(times['full_lifecycle_ns']['majflt'],
                                 sum(times[name]['majflt'] for name in ('load_ns', 'lifecycle_ns', 'dlclose_ns')))
                self.assertEqual(times['teardown_total_ns']['majflt'],
                                 times['free_all_instances_ns']['majflt'] + times['fini_ns']['majflt'])

    def test_each_native_launch_certifies_copied_object_and_stops_before_eviction_on_refusal(self):
        campaign = self.campaign()
        for facade in ('pinyin', 'zhuyin'):
            for cache in ('warm', 'cold'):
                campaign.cache = cache
                with self.subTest(facade=facade, cache=cache), \
                        patch.object(campaign.driver, 'check_tkrzw', side_effect=RuntimeError('runtime refused')) as guard, \
                        patch.object(td, 'evict_files') as evict, patch.object(td.subprocess, 'Popen') as child:
                    with self.assertRaisesRegex(RuntimeError, 'runtime refused'):
                        campaign.run(facade, self.library, self.profile)
                    self.assertEqual(guard.call_args.args[1:4], ('teardown.py', facade, self.library))
                    evict.assert_not_called()
                    test_support.assert_not_called(self, child)
                self.assertFalse(list(self.root.glob('sample-*')))

    def test_profile_population_certifies_pin_before_launch(self):
        campaign = self.campaign()
        with patch.object(campaign.driver, 'check_tkrzw', side_effect=RuntimeError('runtime refused')) as guard, \
                patch.object(td.subprocess, 'Popen') as child:
            with self.assertRaisesRegex(RuntimeError, 'runtime refused'):
                td.populate_user(campaign, self.library, self.profile, self.root / 'rows')
            self.assertEqual(guard.call_args.args[1:4], ('teardown.py/populate', 'pin', self.library))
            test_support.assert_not_called(self, child)

    def test_unit_launch_gets_its_admitted_environment_and_slot_descriptor_and_private_scratch(self):
        worker = self.root / 'unit-worker'
        worker.write_text(f'#!{sys.executable}\nimport os, sys\n'
                          'os.fstat(int(os.environ["TD_FD"]))\n'
                          'print("probe=" + os.environ["TD_PROBE"] + " fd=open", file=sys.stderr)\n'
                          'sys.stdout.write(\'{"teardown":\')\nraise SystemExit(1)\n')
        worker.chmod(0o755)
        with open(self.root / 'held', 'w') as held:
            unit = self.campaign(worker).unit('witness', environment=dict(TD_PROBE='admitted', TD_FD=str(held.fileno())),
                                              fds=(held.fileno(),))
            with self.assertRaises(ValueError) as raised:
                unit.run('pinyin', self.library, self.profile, mode='observe')
        self.assertIn('probe=admitted fd=open', str(raised.exception))
        self.assertEqual(unit.serial, 1)
        self.assertEqual(self.campaign(worker).serial, 0)
        self.assertFalse(list(self.root.glob('witness-sample-*')))

    def test_profile_population_passes_the_unit_environment_and_slot_descriptor(self):
        worker = self.root / 'populate-worker'
        worker.write_text(f'#!{sys.executable}\nimport os\n'
                          'os.fstat(int(os.environ["TD_FD"]))\n'
                          'assert os.environ["TD_PROBE"] == "admitted"\n'
                          'print(\'{"populated": true, "saved": true}\')\n')
        worker.chmod(0o755)
        with open(self.root / 'held-populate', 'w') as held:
            unit = self.campaign(worker).unit('modest', environment=dict(TD_PROBE='admitted', TD_FD=str(held.fileno())),
                                              fds=(held.fileno(),))
            td.populate_user(unit, self.library, self.profile, self.root / 'rows')

    def test_missing_export_is_invalid_and_cleans_private_sample(self):
        campaign = self.campaign()
        with patch.object(td, 'exports', return_value=['missing_export']), self.assertRaisesRegex(ValueError, 'missing export'):
            campaign.run('pinyin', self.library, self.profile)
        self.assertFalse(list(self.root.glob('sample-*')))

    def test_partial_stdout_reports_exit_status_and_stderr(self):
        worker = self.root / 'partial-worker'
        worker.write_text('#!/bin/sh\nprintf \'{"teardown":\'\nprintf \'candidate too long\\n\' >&2\nexit 1\n')
        worker.chmod(0o755)
        with self.assertRaises(ValueError) as raised:
            self.campaign(worker).run('pinyin', self.library, self.profile, mode='observe')
        self.assertIn('exit 1', str(raised.exception))
        self.assertIn('candidate too long', str(raised.exception))

    def test_existing_perf_mode_preserves_schema_and_checksum(self):
        baseline = json.loads(PERF_BASELINE.read_text())
        self.assertEqual(td.digest_bytes((self.root / 'stub.c').read_bytes()), baseline['input_sha256'])
        observation = perf_observation(self.worker, self.library, self.root)
        self.assertEqual(observation['schema'], baseline['schema'])
        self.assertEqual(observation['values'], baseline['values'])
        self.assertEqual(observation['checksum'], baseline['checksum'])


class FakeCancelled(Exception):
    pass


class FakeDriver:
    """The driver surface a campaign uses, with an observable cancellation flag."""
    Cancelled = FakeCancelled

    def __init__(self):
        self.cancelled = False

    def check_cancelled(self):
        if self.cancelled:
            raise FakeCancelled('cancelled')

    def check_tkrzw(self, *args, **kwargs):
        pass


class FakeAdmission:
    """HeavyAdmission stand-in: records its lifecycle and admits a fixed worker count."""
    workers = 2
    blocked = False
    events = None

    def __init__(self, label, env, out=None, runner=False, prepare_heavy_env=None):
        self.label, self.runner = label, runner
        self.env = {'OXPINYIN_POOL_WORKERS': str(self.workers), 'FAKE_ADMITTED': '1'}
        self.slot = SimpleNamespace(pass_fds=lambda: (), handoff=lambda: self.events.append('handoff'))
        self.events.append('admission')

    def try_acquire(self):
        return not self.blocked

    def close(self):
        self.events.append('close')


class UnitPoolTests(unittest.TestCase):
    """run_units, with a fake admission and fake units; no native worker."""
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='teardown-pool-')
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        self.driver = FakeDriver()
        self.events = []
        self.lock = threading.Lock()
        FakeAdmission.events, FakeAdmission.workers, FakeAdmission.blocked = self.events, 2, False
        patcher = patch('tools.dev.slot.HeavyAdmission', FakeAdmission)
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = patch.object(td.host, 'allowed_cpus', return_value={0, 1, 2, 3})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.children = []

    def campaign(self, jobs=None, cpu=2):
        return td.Campaign(self.root / 'worker', self.root, self.root, cpu, self.driver, jobs=jobs)

    def log(self, event):
        with self.lock:
            self.events.append(event)

    def spawn(self):
        """Keep real processes to verify cancellation kills and reaps the whole group."""
        child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'], start_new_session=True)
        self.children.append(child)
        self.addCleanup(lambda: (child.kill(), child.wait()))
        return child

    def watch(self, unit, child):
        """Hold a child until the unit is stopped or cancelled, then reap its group before returning."""
        try:
            while True:
                unit.driver.check_cancelled()
                time.sleep(.01)
        except BaseException:
            os.killpg(child.pid, 9)
            child.wait()
            self.log(('reaped', unit.prefix))
            raise

    def test_overlap_is_bounded_by_admission(self):
        for admitted in (1, 2, 3):
            with self.subTest(admitted=admitted):
                FakeAdmission.workers = admitted
                active, peak, order = [0], [0], []
                everyone = threading.Barrier(admitted, timeout=5)

                def unit(name):
                    def run(campaign):
                        with self.lock:
                            active[0] += 1
                            peak[0] = max(peak[0], active[0])
                            order.append(name)
                        # Synchronize actual workers; elapsed time is not the overlap contract.
                        everyone.wait()
                        with self.lock:
                            active[0] -= 1
                        return name
                    return name, run
                units = [unit('u%d' % number) for number in range(6)]
                self.assertEqual(td.fan_out(self.campaign(), units, 'pool'), [name for name, _ in units])
                self.assertEqual(peak[0], admitted)
                if admitted == 1:
                    self.assertEqual(order, [name for name, _ in units])

    def test_the_pool_is_admitted_before_its_units_and_released_after_them(self):
        td.fan_out(self.campaign(), [('a', lambda c: self.log('a')), ('b', lambda c: self.log('b')),
                                       ('c', lambda c: self.log('c'))], 'pool')
        self.assertEqual(self.events[:2], ['admission', 'handoff'])
        self.assertEqual(self.events[-1], 'close')
        self.assertEqual(sorted(self.events[2:-1]), ['a', 'b', 'c'])

    def test_one_job_and_one_unit_run_in_order_under_admission(self):
        caller, seen = threading.get_ident(), []
        units = [(name, lambda c, name=name: seen.append((name, threading.get_ident() == caller, c.prefix, c.cpu)))
                 for name in ('a', 'b', 'c')]
        td.fan_out(self.campaign(jobs=1), units, 'pool')
        td.fan_out(self.campaign(), units[:1], 'pool')
        self.assertEqual(seen, [('a', False, 'a-sample', 2), ('b', False, 'b-sample', 2),
                                ('c', False, 'c-sample', 2), ('a', False, 'a-sample', 2)])
        self.assertEqual(self.events, ['admission', 'handoff', 'close'] * 2)

    def test_serial_failure_stops_the_remaining_units(self):
        ran = []

        def failing(campaign):
            raise ValueError('populate failed')
        units = [('a', lambda c: ran.append('a')), ('b', failing), ('c', lambda c: ran.append('c'))]
        with self.assertRaisesRegex(ValueError, 'populate failed'):
            td.fan_out(self.campaign(jobs=1), units, 'pool')
        self.assertEqual(ran, ['a'])

    def test_failure_starts_no_queued_unit_and_reaps_running_children_before_release(self):
        FakeAdmission.workers = 2
        started = []
        both_running = threading.Barrier(2, timeout=5)

        def failing(campaign):
            started.append('fail')
            both_running.wait()
            raise ValueError('populate failed')

        def waiting(campaign):
            started.append('wait')
            child = self.spawn()
            both_running.wait()
            self.watch(campaign, child)
        queued = [(name, lambda c, name=name: started.append(name)) for name in ('q1', 'q2', 'q3')]
        with self.assertRaisesRegex(ValueError, 'populate failed'):
            td.fan_out(self.campaign(), [('fail', failing), ('wait', waiting), *queued], 'pool')
        self.assertEqual(sorted(started), ['fail', 'wait'])
        self.assertIsNotNone(self.children[0].returncode)
        self.assertEqual(self.events[-2:], [('reaped', 'wait-sample'), 'close'])

    def test_cancellation_stops_queued_units_and_reaps_before_release(self):
        FakeAdmission.workers = 2
        started, running = [], threading.Barrier(3, timeout=5)

        def holding(name):
            def run(campaign):
                started.append(name)
                child = self.spawn()
                running.wait()
                self.watch(campaign, child)
            return name, run
        queued = [(name, lambda c, name=name: started.append(name)) for name in ('q1', 'q2')]

        def cancel():
            running.wait()
            self.driver.cancelled = True
        thread = threading.Thread(target=cancel)
        thread.start()
        self.addCleanup(thread.join)
        with self.assertRaises(FakeCancelled):
            td.fan_out(self.campaign(), [holding('a'), holding('b'), *queued], 'pool')
        self.assertEqual(sorted(started), ['a', 'b'])
        self.assertTrue(all(child.returncode is not None for child in self.children))
        self.assertEqual(sorted(event for event in self.events if isinstance(event, tuple) and event[0] == 'reaped'),
                         [('reaped', 'a-sample'), ('reaped', 'b-sample')])
        self.assertEqual(self.events[-1], 'close')

    def test_cancellation_while_waiting_for_admission_starts_nothing_and_releases(self):
        FakeAdmission.blocked = True
        started = []
        threading.Timer(.2, lambda: setattr(self.driver, 'cancelled', True)).start()
        with self.assertRaises(FakeCancelled):
            td.fan_out(self.campaign(), [('a', lambda c: started.append('a')), ('b', lambda c: started.append('b'))],
                         'pool')
        self.assertEqual(started, [])
        self.assertEqual(self.events[-1], 'close')

    def test_each_concurrent_unit_has_private_names_counter_cpu_and_stop_view(self):
        FakeAdmission.workers = 3
        base = self.campaign(cpu=2)
        seen, together = {}, threading.Barrier(3, timeout=5)

        def unit(name):
            def run(campaign):
                together.wait()
                seen[name] = dict(prefix=campaign.prefix, cpu=campaign.cpu, driver=campaign.driver,
                                  names=[campaign.next_scratch() for _ in range(50)], environment=campaign.environment)
            return name, run
        td.fan_out(base, [unit(name) for name in ('modest', 'phrases', 'journals')], 'pool')
        names = [path for value in seen.values() for path in value['names']]
        self.assertEqual(len(set(names)), 150)
        self.assertEqual(base.serial, 0)
        self.assertEqual(base.prefix, 'sample')
        self.assertEqual({value['prefix'] for value in seen.values()},
                         {'modest-sample', 'phrases-sample', 'journals-sample'})
        # The campaign's own CPU leads; no two concurrent units share one.
        self.assertEqual(sorted(value['cpu'] for value in seen.values()), [0, 1, 2])
        self.assertTrue(all(isinstance(value['driver'], pool.StoppableDriver) for value in seen.values()))
        self.assertTrue(all(value['environment'].get('FAKE_ADMITTED') == '1' for value in seen.values()))
        self.assertEqual(base.environment, {})

    def test_unit_names_are_unique_across_threads_of_one_campaign_family(self):
        base = self.campaign()
        names, lock = [], threading.Lock()

        def take(unit):
            for _ in range(200):
                path = unit.next_scratch()
                with lock:
                    names.append(path)
        threads = [threading.Thread(target=take, args=(base.unit('u%d' % number),)) for number in range(4)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        self.assertEqual(len(set(names)), 800)

    def test_results_and_emission_follow_unit_order_whatever_the_completion_order(self):
        FakeAdmission.workers = 4
        caller, emitted = threading.get_ident(), []
        finished = [threading.Event() for _ in range(4)]

        def unit(number):
            def run(campaign):
                # Unit n ends only after unit n+1 has: completion is the reverse of unit order.
                if number < 3:
                    finished[number + 1].wait(5)
                finished[number].set()
                return 'result-%d' % number
            return 'u%d' % number, run

        def emit(name, result):
            emitted.append((name, result, threading.get_ident() == caller))
        self.assertEqual(td.fan_out(self.campaign(), [unit(number) for number in range(4)], 'pool', emit),
                         ['result-%d' % number for number in range(4)])
        self.assertEqual(emitted, [('u%d' % number, 'result-%d' % number, True) for number in range(4)])


def export_lines():
    """An export that fills both 10,000-phrase recipes, in library order."""
    lines = []
    for library, count in ((1, 4000), (2, 4000), (3, 4000), (4, 1500)):
        for number in range(count):
            phrase = f'词{library}_{number}'
            reading = "'".join(['ci2'] * len(phrase))
            lines.append(f'{library}\t{phrase}\t{reading}\t7')
    return lines


class FixtureFlowTests(unittest.TestCase):
    """provision and prepare_witnesses with fake native launches, serial against pooled."""
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='teardown-fixtures-')
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        FakeAdmission.events, FakeAdmission.workers, FakeAdmission.blocked = [], 4, False
        for patcher in (patch('tools.dev.slot.HeavyAdmission', FakeAdmission),
                        patch.object(td.host, 'allowed_cpus', return_value={0, 1, 2, 3})):
            patcher.start()
            self.addCleanup(patcher.stop)
        self.records = []
        patcher = patch.object(td, 'print_record', lambda kind, **values: self.records.append((kind, values)))
        patcher.start()
        self.addCleanup(patcher.stop)

    def campaign(self, jobs):
        return td.Campaign(self.root / 'worker', self.root, self.root, 0, FakeDriver(), jobs=jobs)

    @staticmethod
    def tree(directory):
        return {str(path.relative_to(directory)): path.read_bytes()
                for path in sorted(Path(directory).rglob('*')) if path.is_file()}

    def provisioned(self, name, jobs):
        """Run provision under fake population that finishes in reverse profile order."""
        order = list(td.PROFILES)
        done = {}

        def populate(campaign, pin, user, rows, **kwargs):
            profile = user.parent.name
            # A later profile finishes first, when the pool lets it.
            later = order[order.index(profile) + 1:]
            if jobs != 1:
                for other in later:
                    done.setdefault(other, threading.Event()).wait(5)
            (user / 'user.bin').write_bytes(rows.read_bytes()[:64] + profile.encode())
            done.setdefault(profile, threading.Event()).set()

        def run(self_, facade, library, profile, *, mode='time', **kwargs):
            self.assertEqual(mode, 'export')
            return dict(observation=dict(documents=export_lines()))
        del self.records[:]
        with patch.object(td, 'populate_user', populate), patch.object(td.Campaign, 'run', run):
            profiles = td.provision(self.campaign(jobs), Path('pin.so'), self.root / name)
        return profiles, list(self.records)

    def test_pooled_provision_matches_serial_output_order_and_files(self):
        serial_profiles, serial_records = self.provisioned('serial', 1)
        self.assertEqual(FakeAdmission.events, ['admission', 'handoff', 'close'])
        pooled_profiles, pooled_records = self.provisioned('pooled', None)
        self.assertEqual([record[1]['profile'] for record in serial_records], list(td.PROFILES))
        self.assertEqual(serial_records, pooled_records)
        self.assertEqual({name: path.relative_to(self.root / 'serial') for name, path in serial_profiles.items()},
                         {name: path.relative_to(self.root / 'pooled') for name, path in pooled_profiles.items()})
        self.assertEqual(self.tree(self.root / 'serial'), self.tree(self.root / 'pooled'))
        self.assertEqual(FakeAdmission.events, ['admission', 'handoff', 'close'] * 2)

    def test_provision_failure_in_one_profile_raises_after_the_export_and_stops_the_rest(self):
        FakeAdmission.workers = 1
        seen = []

        def populate(campaign, pin, user, rows, **kwargs):
            seen.append(user.parent.name)
            if user.parent.name == 'modest':
                raise ValueError('fixture population failed')

        def run(self_, *args, mode='time', **kwargs):
            return dict(observation=dict(documents=export_lines()))
        with patch.object(td, 'populate_user', populate), patch.object(td.Campaign, 'run', run):
            with self.assertRaisesRegex(ValueError, 'fixture population failed'):
                td.provision(self.campaign(None), Path('pin.so'), self.root / 'failed')
        self.assertEqual(seen, ['modest'])
        self.assertEqual(self.records, [('fixture', unittest.mock.ANY)])
        self.assertEqual(self.records[0][1]['profile'], 'empty')
        self.assertEqual(FakeAdmission.events[-1], 'close')

    def witnesses(self, name, jobs):
        profiles = {}
        for profile in td.PROFILES:
            directory = self.root / name / profile
            (directory / 'user').mkdir(parents=True)
            (directory / 'witness.txt').write_text('')
            (directory / 'rows.tsv').write_text(td.row_text(
                [(1, 'p%s%d' % (profile, number), 'wo%s%d' % ('abc'[number % 3], number % 5 + 1), 5) for number in range(6)]
                if profile != 'empty' else []))
            profiles[profile] = directory / 'user'
        (self.root / name / 'phrases' / 'witness.txt').write_text('kept\n')
        order = list(td.PROFILES[1:])
        done = {}

        def run(self_, facade, library, profile, *, mode='time', inputs=None, **kwargs):
            populated = profile.parent.name != 'empty'
            if populated and jobs != 1:
                # Reverse completion: a later profile's check ends before an earlier one's.
                for other in order[order.index(profile.parent.name) + 1:]:
                    done.setdefault(other, threading.Event()).wait(5)
            documents = [dict(parser='full', transcript=value, answer=value + (profile.parent.name if populated else ''))
                         for value in inputs.read_text().split()]
            if populated:
                done.setdefault(profile.parent.name, threading.Event()).set()
            return dict(observation=dict(documents=documents))
        del self.records[:]
        with patch.object(td.Campaign, 'run', run):
            result = td.prepare_witnesses(self.campaign(jobs), Path('pin.so'), profiles)
        return result, list(self.records), self.tree(self.root / name)

    def test_pooled_witnesses_match_serial_records_and_files(self):
        serial = self.witnesses('serial', 1)
        pooled = self.witnesses('pooled', None)
        self.assertEqual(list(serial[0]), list(td.PROFILES))
        self.assertEqual([(kind, values['profile']) for kind, values in serial[1]],
                         [('witness', name) for name in td.PROFILES[1:]])
        self.assertEqual(serial[1], pooled[1])
        self.assertEqual(serial[2], pooled[2])
        self.assertEqual(serial[2]['phrases/witness.txt'], b'kept\n')
        self.assertEqual(serial[2]['empty/witness.txt'], b'')

    def test_a_populated_profile_that_changes_no_answer_stops_the_witness_pool(self):
        profiles = {}
        for profile in td.PROFILES:
            directory = self.root / profile
            (directory / 'user').mkdir(parents=True)
            (directory / 'witness.txt').write_text('')
            (directory / 'rows.tsv').write_text(td.row_text([] if profile == 'empty' else [(1, 'p', 'wo3', 5)]))
            profiles[profile] = directory / 'user'

        def run(self_, facade, library, profile, *, mode='time', inputs=None, **kwargs):
            return dict(observation=dict(documents=[dict(parser='full', transcript=value, answer='same')
                                                    for value in inputs.read_text().split()]))
        with patch.object(td.Campaign, 'run', run):
            with self.assertRaisesRegex(ValueError, 'changes no witness answer'):
                td.prepare_witnesses(self.campaign(None), Path('pin.so'), profiles)
        self.assertEqual(self.records, [])
        self.assertEqual(FakeAdmission.events[-1], 'close')


if __name__ == '__main__':
    if '--write-perf-baseline' in sys.argv:
        sys.argv.remove('--write-perf-baseline')
        write_perf_baseline()
        raise SystemExit(0)
    unittest.main()
