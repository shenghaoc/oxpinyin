#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Teardown instrumentation controls. Run through the existing heavy slot: cc is used."""
import importlib.util
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


class NativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix='teardown-controls-')
        cls.root = Path(cls.temporary.name)
        cls.worker, _ = td.build_worker(cls.root)
        pairs = dict(init='init', fini='fini', alloc_instance='alloc', free_instance='free', set_options='options',
                     parse_more_full_pinyins='parse', parse_more_chewings='parse', reset='true',
                     guess_sentence='true', get_sentence='sentence', get_n_candidate='count',
                     get_candidate='candidate', get_candidate_type='type', get_candidate_string='string',
                     save='save', begin_add_phrases='begin', iterator_add_phrase='add', end_add_phrases='end',
                     load_phrase_library='load')
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
                source += f'extern __typeof__(fake_{implementation}) {facade}_{suffix} __attribute__((alias("fake_{implementation}")));\n'
        cfile = cls.root / 'stub.c'
        cfile.write_text(source)
        cls.library = cls.root / 'stub.so'
        subprocess.run(['cc', '-shared', '-fPIC', '-Wall', '-Wextra', '-Werror', '-O2',
                        '-o', str(cls.library), str(cfile)], check=True)
        cls.profile = cls.root / 'profile'
        cls.profile.mkdir()

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def campaign(self):
        spec = importlib.util.spec_from_file_location('contract_diff', td.HERE / 'contract-diff.py')
        driver = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(driver)
        return td.Campaign(self.worker, self.root, self.root, min(os.sched_getaffinity(0)), driver)

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

    def test_parent_checkpoints_and_surviving_instance(self):
        for facade in ('pinyin', 'zhuyin'):
            with self.subTest(facade=facade), patch.object(td, 'exports', return_value=[facade + '_init']):
                row = self.campaign().run(facade, self.library, self.profile, mode='memory', instances=2, scenario='multi')
                self.assertEqual(set(row['checkpoints'][1]), {'M0', 'M1', 'M2', 'M3'})
                self.assertEqual(row['observation']['stderr'].splitlines(), ['INIT', 'ALLOC', 'ALLOC', 'FREE', 'FREE', 'FINI'])
                self.assertEqual(len(row['samples'][0]['free_instance_ns']), 2)

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

    def test_existing_perf_mode_preserves_schema_and_checksum(self):
        source = self.root / 'bisect-main.c'
        source.write_bytes(subprocess.check_output(['git', 'show', 'origin/main:tools/bisection/bisect.c'], cwd=td.ROOT))
        baseline = self.root / 'bisect-main'
        subprocess.run(['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2',
                        '-o', str(baseline), str(source), '-ldl'], check=True)
        values = []
        for executable in (baseline, self.worker):
            result = subprocess.run([str(executable), '--perf', str(self.library), str(self.root)],
                                    env=dict(os.environ, PERF_MODE='speed', PERF_CYCLES='2'),
                                    capture_output=True, text=True, check=True)
            values.append(td.json.loads(result.stdout))
        def schema(value):
            if isinstance(value, dict):
                return {key: schema(item) for key, item in value.items()}
            if isinstance(value, list):
                return [schema(item) for item in value]
            return type(value).__name__
        self.assertEqual(schema(values[0]), schema(values[1]))
        for key in ('backend', 'so', 'systemdir', 'mode', 'checksum', 'cycles'):
            if key in values[0]:
                self.assertEqual(values[0][key], values[1][key])


if __name__ == '__main__':
    unittest.main()
