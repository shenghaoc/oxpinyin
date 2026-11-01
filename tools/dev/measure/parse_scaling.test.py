#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The parse-scaling workload against a stub library, and the recipe's parsing (#1501); no model needed."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.measure import bench, parse_scaling

# The parse records the input it saw and holds a block of eight bytes per
# input byte for the length of the call, so the heap build must report it.
STUB = r'''
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static size_t last;
void *pinyin_init(const char *a, const char *b) { (void)a; (void)b; return malloc(1); }
void pinyin_fini(void *c) { free(c); }
void *pinyin_alloc_instance(void *c) { (void)c; return malloc(1); }
void pinyin_free_instance(void *i) { free(i); }
int pinyin_set_options(void *c, unsigned o) { (void)c; (void)o; return 1; }
size_t pinyin_parse_more_full_pinyins(void *i, const char *text) {
    (void)i;
    size_t n = strlen(text);
    char *block = malloc(n * 8);
    memset(block, 1, n * 8);
    fprintf(stderr, "input %c%c..%c%c apostrophes %zu\n", text[0], text[1], text[n - 2], text[n - 1],
            (size_t)(strchr(text, 'i') ? strspn(text + 2, "'") : 0));
    free(block);
    return last = n;
}
size_t pinyin_get_parsed_input_length(void *i) { (void)i; return last; }
int pinyin_guess_candidates(void *i, size_t o, unsigned s) { (void)i; (void)o; (void)s; return 1; }
int pinyin_get_n_candidate(void *i, unsigned *n) { (void)i; *n = 0; return 1; }
int pinyin_reset(void *i) { (void)i; return 1; }
'''


class HarnessTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.work = tempfile.TemporaryDirectory()
        root = Path(cls.work.name)
        (root / 'stub.c').write_text(STUB)
        cls.stub = root / 'libstub.so'
        subprocess.run(['gcc', '-shared', '-fPIC', '-o', cls.stub, root / 'stub.c'], check=True)
        cls.plain, cls.heap = root / 'bisect', root / 'bisect-heap'
        subprocess.run(bench.harness_build(cls.plain), check=True, capture_output=True)
        subprocess.run(bench.harness_build(cls.heap, flags=(*bench.STRICT_FLAGS, *parse_scaling.HEAP_FLAGS)),
                       check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.work.cleanup()

    def parse(self, harness, shape, length):
        done = subprocess.run([harness, '--perf-parse', self.stub, self.work.name, shape, str(length)],
                              capture_output=True, text=True)
        return done.returncode, done.stdout, done.stderr

    def test_each_shape_is_generated_at_its_exact_length(self):
        for shape, seen in (('ni', 'input ni..ni apostrophes 0'), ('apostrophe', 'input ni..ni apostrophes 9996')):
            with self.subTest(shape=shape):
                status, out, err = self.parse(self.plain, shape, 10000)
                self.assertEqual(status, 0, err)
                row = parse_scaling.observation(out, shape, 10000)
                self.assertEqual((row['parsed'], row['heap_peak_bytes']), (10000, None))
                self.assertIn(seen, err)

    def test_the_heap_build_reports_the_peak_inside_the_call(self):
        status, out, err = self.parse(self.heap, 'ni', 4096)
        self.assertEqual(status, 0, err)
        self.assertGreaterEqual(json.loads(out)['heap_peak_bytes'], 4096 * 8)

    def test_bad_shapes_and_lengths_are_refused_before_loading(self):
        for shape, length in (('ni', 0), ('apostrophe', 3), ('nope', 8), ('ni', '1x')):
            with self.subTest(shape=shape, length=length):
                self.assertEqual(self.parse(self.plain, shape, length)[0], 2)


class RecipeTests(unittest.TestCase):
    def test_a_reply_must_be_whole_and_agree_with_the_stored_length(self):
        good = dict(pid=1, shape='ni', length=8, parsed=8, stored=8, elapsed_ns=5, heap_peak_bytes=None)
        self.assertEqual(parse_scaling.observation(json.dumps(good), 'ni', 8)['parsed'], 8)
        for change in (dict(stored=7), dict(parsed=0, stored=0), dict(shape='apostrophe'), dict(elapsed_ns=-1)):
            with self.subTest(change=change), self.assertRaises(ValueError):
                parse_scaling.observation(json.dumps(dict(good, **change)), 'ni', 8)

    def test_the_callgrind_total_is_read_from_the_profile(self):
        self.assertEqual(parse_scaling.callgrind_total('events: Ir\nfn=x\n1 2\ntotals: 1234\n'), 1234)
        with self.assertRaises(ValueError):
            parse_scaling.callgrind_total('events: Ir\n')

    def test_arms_and_lengths_are_validated(self):
        self.assertEqual(parse_scaling.arm('base=/x.so'), ('base', Path('/x.so')))
        self.assertEqual(parse_scaling.counts('4096,262144'), [4096, 262144])
        for bad in ('base', '=x', 'Base=x'):
            with self.subTest(arm=bad), self.assertRaises(Exception):
                parse_scaling.arm(bad)
        with self.assertRaises(Exception):
            parse_scaling.counts('3')

    def test_the_table_has_a_row_per_arm_and_a_ratio_per_pair(self):
        rows = {name: dict(parsed=8, median_ns=8e6, ns_per_byte=1e6, instructions=80, instructions_per_byte=10,
                           heap_peak_bytes=64, heap_per_byte=8) for name in ('base', 'branch')}
        ratio = dict(median=1.5, ci95=[1.4, 1.6])
        text = parse_scaling.table([dict(shape='ni', length=8, arms=rows, time_ratios={'branch/base': ratio})],
                                   ['base', 'branch'])
        self.assertIn('| ni | 8 | branch | 8 | 8.000 | 1000000.0 | 80 | 10 | 64 | 8.0 |', text)
        self.assertIn('| ni | 8 | 1.500 (1.400-1.600) |', text)


if __name__ == '__main__':
    unittest.main()
