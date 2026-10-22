#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Teardown instrumentation controls. Run through the existing heavy slot: cc is used."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import teardown as td


class PureTests(unittest.TestCase):
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
        self.assertEqual(set(names), {'ox', 'control', 'pin'})
        self.assertEqual(len(set(names.values())), 3)
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

    def test_precise_equivalence_difference(self):
        self.assertEqual(td.first_difference({'a': [{'x': 1}]}, {'a': [{'x': 2}]}),
                         {'path': '$.a[0].x', 'pin': 1, 'subject': 2})

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
                  'uv run --no-project --managed-python --python cpython@3.15.0+gil '
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

    def campaign(self, worker=None):
        spec = importlib.util.spec_from_file_location('contract_diff', td.HERE / 'contract-diff.py')
        driver = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(driver)
        affinity = getattr(os, 'sched_getaffinity', None)
        cpu = min(affinity(0)) if affinity else None
        return td.Campaign(worker or self.worker, self.root, self.root, cpu, driver)

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


if __name__ == '__main__':
    if '--write-perf-baseline' in sys.argv:
        sys.argv.remove('--write-perf-baseline')
        write_perf_baseline()
        raise SystemExit(0)
    unittest.main()
