#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Native ABI call-order and campaign refusal tests for #1348; no model needed."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.measure import bench, sentence
from tools.dev.runners import Finished

STUB = r'''
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static char parsed[256];
static int state;
void *pinyin_init(const char *data, const char *user) {
    (void)data; fprintf(stderr, "USER:%s\n", user); return (void *)1;
}
void pinyin_fini(void *p) { (void)p; fputs("FINI\n", stderr); }
void *pinyin_alloc_instance(void *p) { fputs("ALLOC\n", stderr); return p; }
void pinyin_free_instance(void *p) { (void)p; fputs("FREE\n", stderr); }
bool pinyin_set_options(void *p, uint32_t flags) { return p && flags == 0x18a; }
bool pinyin_reset(void *p) { state=0; fputs("RESET\n", stderr); return p; }
size_t pinyin_parse_more_full_pinyins(void *p, const char *s) {
    (void)p; strcpy(parsed, s); state=1; fprintf(stderr, "PARSE:%s\n", s); return strlen(s);
}
bool pinyin_guess_sentence(void *p) {
    (void)p; if (state != 1) abort(); state=2; fputs("SENTENCE\n", stderr);
    return getenv("FAIL_SENTENCE") == NULL;
}
bool pinyin_get_sentence(void *p, uint8_t n, char **s) {
    (void)p; if (state < 2 || n) abort(); *s=strdup(parsed); return true;
}
bool pinyin_guess_candidates(void *p, size_t offset, uint32_t sort) {
    (void)p; if (state != 2 || offset || sort != 0x1e) abort();
    state=3; fputs("CANDIDATES\n", stderr); return true;
}
bool pinyin_get_n_candidate(void *p, unsigned *n) { (void)p; *n=12; return state==3; }
bool pinyin_get_candidate(void *p, unsigned n, void **c) {
    (void)p; *c=(void *)(uintptr_t)(n+1); return n < 12;
}
bool pinyin_get_candidate_type(void *p, void *c, int *t) { (void)p; *t=(uintptr_t)c==1 ? 1 : 2; return true; }
bool pinyin_get_candidate_string(void *p, void *c, const char **s) {
    (void)p; *s=(uintptr_t)c==12 ? "tail\"\\\n" : "duplicate"; return true;
}
bool pinyin_get_candidate_nbest_index(void *p, void *c, uint8_t *n) {
    (void)p; if ((uintptr_t)c != 1) abort(); *n=7; return true;
}
'''


class NativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='sentence-harness-')
        cls.root = Path(cls.temp.name)
        cls.harness, cls.library = cls.root / 'bisect', cls.root / 'stub.so'
        source = cls.root / 'stub.c'
        source.write_text(STUB)
        subprocess.run(bench.harness_build(cls.harness), check=True, capture_output=True)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-o', cls.library, source], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def launch(self, mode, input='nihao', iterations='1', extra=None):
        return subprocess.run([self.harness, '--perf-' + mode, self.library, self.root, input, iterations],
                              capture_output=True, env=dict(os.environ, **(extra or {})))

    def test_sentence_calls_parse_then_sentence_without_candidates(self):
        done = self.launch('sentence', iterations='2')
        self.assertEqual(done.returncode, 0, done.stderr)
        row = sentence.observation(done.stdout, types.SimpleNamespace(workload='sentence', iterations=2, input='nihao'))
        self.assertEqual(row['observations'][0], row['observations'][1])
        self.assertEqual(done.stderr.decode().splitlines()[1:],
                         ['ALLOC', 'RESET', 'PARSE:nihao', 'SENTENCE', 'RESET', 'PARSE:nihao', 'SENTENCE', 'FREE', 'FINI'])
        user = done.stderr.decode().splitlines()[0].removeprefix('USER:')
        self.assertFalse(Path(user).exists())

    def test_elapsed_ns_accepts_zero_and_rejects_negative_or_non_int(self):
        args = types.SimpleNamespace(workload='sentence', iterations=1, input='ni')
        row = dict(pid=123, workload='sentence', iterations=1, observations=[[dict(
            input='ni', parsed=2, guess_sentence=True, get_sentence=True, sentence='x')]])
        self.assertEqual(sentence.observation(json.dumps(dict(row, elapsed_ns=0)), args)['elapsed_ns'], 0)
        for bad in (-1, 1.0, '0', True, None):
            with self.subTest(elapsed_ns=bad), self.assertRaises(ValueError):
                sentence.observation(json.dumps(dict(row, elapsed_ns=bad)), args)

    def test_typing_reuses_instance_and_retains_every_candidate_in_order(self):
        done = self.launch('typing-sequence', 'ni')
        self.assertEqual(done.returncode, 0, done.stderr)
        row = sentence.observation(done.stdout, types.SimpleNamespace(workload='typing-sequence', iterations=1, input='ni'))
        steps = row['observations'][0]
        self.assertEqual([s['sentence'] for s in steps], ['n', 'ni'])
        self.assertEqual(len(steps[0]['candidates']), 12)
        self.assertEqual(steps[0]['candidates'][0], dict(type=1, nbest=7, text='duplicate'))
        self.assertEqual(steps[0]['candidates'][-1], dict(type=2, nbest=None, text='tail"\\\n'))
        self.assertEqual(done.stderr.decode().splitlines()[1:],
                         ['ALLOC', 'RESET', 'PARSE:n', 'SENTENCE', 'CANDIDATES',
                          'PARSE:ni', 'SENTENCE', 'CANDIDATES', 'FREE', 'FINI'])

    def test_failure_is_nonzero_and_releases_instance_context_and_user_directory(self):
        done = self.launch('sentence', extra={'FAIL_SENTENCE': '1'})
        self.assertEqual(done.returncode, 1)
        lines = done.stderr.decode().splitlines()
        self.assertEqual(lines[-3:], ['FREE', 'FINI', 'sentence workload failed'])
        self.assertFalse(Path(lines[0].removeprefix('USER:')).exists())

    def test_bad_inputs_and_counts_are_rejected_before_dlopen(self):
        for text, count in [('', '1'), ('你好', '1'), ('a' * 256, '1'), ('ni', '0'),
                            ('ni', '-1'), ('ni', '100001'), ('ni', '1x'), ('ni', '')]:
            with self.subTest(text=text, count=count):
                done = self.launch('sentence', text, count)
                self.assertEqual(done.returncode, 2)
                self.assertEqual(done.stdout, b'')
                self.assertNotIn(b'USER:', done.stderr)

    def test_distinct_anchors_survive_optimization(self):
        symbols = subprocess.check_output(['nm', self.harness], text=True).splitlines()
        # Mach-O prefixes C symbols with an underscore; ELF does not.
        addresses = {s.split()[2].removeprefix('_'): s.split()[0] for s in symbols if len(s.split()) == 3}
        names = list(sentence.ANCHORS.values()) + ['bisect_perf_cold_unit', 'bisect_perf_steady_unit']
        self.assertEqual(len({addresses[name] for name in names}), 4)


class CampaignTests(unittest.TestCase):
    def fake_campaign(self, *, changed_answer=False, varying_control=False, vary='f', workload='typing-sequence',
                      changed=None, vary_arm='pctl'):
        temporary = self.enterContext(tempfile.TemporaryDirectory(prefix='sentence-campaign-'))
        root = Path(temporary)
        libraries = {arm: root / arm / 'libpinyin.so' for arm in sentence.ARMS}
        for path in libraries.values():
            path.parent.mkdir()
            path.write_bytes(b'library')
        args = types.SimpleNamespace(out=root, workload=workload, input='ni', cell='bdb',
                                     iterations=1, rounds=2, cpu=0, main=Path('/main'), pr=Path('/pr'),
                                     pin=Path('/pin'), main_shipping=Path('/ship-main'),
                                     pr_shipping=Path('/ship-pr'), system_dir=Path('/data'), unordered_arm=[])
        commands = []

        def capture(command, **kwargs):
            if command[0] == 'nm':
                return types.SimpleNamespace(status=0, stdout=b'1 T bisect_sentence_unit\n2 T bisect_typing_unit\n')
            self.assertEqual(command[:3], ['taskset', '-c', '0'])
            commands.append(command)
            self.assertIn('--toggle-collect=' + sentence.ANCHORS[workload], command)
            self.assertEqual(command[-5:], ['--perf-' + workload, command[-4], '/data', 'ni', '1'])
            arm = Path(command[-4]).parent.name
            output = Path(next(c.split('=', 1)[1] for c in command if c.startswith('--callgrind-out-file=')))
            count = 101 if varying_control and arm == vary_arm else 100
            costs = {vary: count}
            if changed and arm in ('pr__', 'pctl'):
                costs[changed] = 7  # a deterministic main/PR difference, identical in both PR copies
            total = sum(costs.values())
            body = ''.join(f'fn={name}\n1 {cost}\n' for name, cost in costs.items())
            output.write_text(f'creator: callgrind-3.26\npid: 123\ncmd: {root}/bisect --perf-typing-sequence x\n'
                              f'events: Ir\nsummary: {total}\nob=/lib.so\n{body}totals: {total}\n')
            steps = [dict(input=i, parsed=len(i), guess_sentence=True, get_sentence=True, sentence=i,
                          guess_candidates=True, candidates=[])
                     for i in (('n', 'ni') if workload == 'typing-sequence' else ('ni',))]
            if changed_answer and arm == 'pr__':
                steps[-1]['sentence'] = 'changed'
            row = dict(pid=123, workload=args.workload, iterations=1, elapsed_ns=1000, observations=[steps])
            return types.SimpleNamespace(status=0, stdout=json.dumps(row).encode(), stderr=b'')

        with mock.patch.object(sentence, 'prepare', return_value=libraries), \
                mock.patch.object(bench, 'run'), mock.patch.object(bench, 'guard') as guard, \
                mock.patch.object(bench, 'capture', side_effect=capture), \
                mock.patch.object(sentence.identity, 'cpu_model', return_value='test'), \
                contextlib.redirect_stdout(io.StringIO()):
            code = sentence.campaign(args, {})
        self.assertEqual(guard.call_count, 10)
        self.assertEqual([Path(c[-4]).parent.name for c in commands],
                         list(sentence.ARMS) + list(sentence.ARMS[1:] + sentence.ARMS[:1]))
        return code, json.loads((root / 'result.json').read_text())

    def test_campaign_rotates_serial_arms_and_checks_the_complete_answers(self):
        code, result = self.fake_campaign()
        self.assertEqual(code, 0)
        self.assertEqual(result['verdict'], 'VALID')
        self.assertEqual(result['main_minus_pr_bounds'], [0, 0])
        self.assertTrue(result['main_pr_observations_equal'])
        self.assertTrue(result['main_pin_observations_equal'])

    def test_changed_answer_or_same_bytes_instruction_variation_invalidates_campaign(self):
        for options in (dict(changed_answer=True), dict(varying_control=True)):
            with self.subTest(options=options):
                code, result = self.fake_campaign(**options)
                self.assertEqual(code, 3)
                self.assertEqual(result['verdict'], 'INVALID')
                self.assertNotIn('Ir_per_invocation', result)

    def test_randomstate_scopes_make_a_typing_control_diagnostic_only(self):
        for name in ('oxpinyin_engine::session::order_window_tail', 'dedup_by_text_keep_first',
                     'oxpinyin_engine::session::dedup_ranked_by_text', '__memcmp_avx2_movbe'):
            with self.subTest(name=name):
                code, result = self.fake_campaign(varying_control=True, vary=name)
                self.assertEqual((code, result['verdict'], result['rule']), (0, 'DIAGNOSTIC', 'randomstate'))
                self.assertEqual(result['outside_allowlist'], [])
                self.assertEqual(result['Ir_ranges']['pctl'], [101, 101])
                self.assertFalse(result['controls']['pr__']['valid'])
        for name in ('f', 'dedup_by_text_keep_first_clone', 'oxpinyin_engine::nbest::order_window_tail'):
            with self.subTest(name=name):
                code, result = self.fake_campaign(varying_control=True, vary=name)
                self.assertEqual((code, result['verdict']), (3, 'INVALID'))
                self.assertEqual(result['outside_allowlist'], [dict(pair='pr__', function=name)])
        code, result = self.fake_campaign(varying_control=True, vary='order_window_tail', workload='sentence')
        self.assertEqual((code, result['verdict'], 'rule' in result), (3, 'INVALID', False))

    def test_main_pr_difference_is_not_control_variation(self):
        code, result = self.fake_campaign(varying_control=True, vary='order_window_tail',
                                          changed='oxpinyin_engine::cursor::matrix_keys')
        self.assertEqual((code, result['verdict']), (0, 'DIAGNOSTIC'))
        self.assertEqual(result['varying_by_pair'], dict(main=[], pr__=['order_window_tail']))
        self.assertEqual(result['varying_functions'], ['order_window_tail'])
        self.assertEqual(result['outside_allowlist'], [])
        self.assertNotEqual(result['counts']['main'], result['counts']['pr__'])

    def test_each_pair_is_checked_and_names_the_function_and_pair(self):
        for arm, pair in (('mctl', 'main'), ('pctl', 'pr__')):
            with self.subTest(pair=pair):
                code, result = self.fake_campaign(varying_control=True, vary='matrix_keys', vary_arm=arm,
                                                  changed='order_window_tail')
                self.assertEqual((code, result['verdict']), (3, 'INVALID'))
                self.assertEqual(result['outside_allowlist'], [dict(pair=pair, function='matrix_keys')])

    def test_controls_reject_both_within_arm_and_between_copy_variation(self):
        counts = {arm: [100, 100] for arm in sentence.ARMS}
        self.assertTrue(all(c['valid'] for c in sentence.instruction_controls(counts).values()))
        counts['mctl'][1] = 101
        counts['pctl'] = [99, 99]
        controls = sentence.instruction_controls(counts)
        self.assertEqual(controls['main'], dict(valid=False, spread=1))
        self.assertEqual(controls['pr__'], dict(valid=False, spread=1))

    def test_profile_requires_identity_events_positive_and_reconciled_counts(self):
        raw = b'creator: callgrind-3.26\npid: 123\ncmd: /tmp/bisect --perf-sentence x\nevents: Ir\nsummary: 7\nob=/lib.so\nfn=f\n1 7\ntotals: 7\n'
        self.assertEqual(sentence.checked_profile(raw, Path('/tmp/bisect'), 123, Path('/lib.so'))['total'], 7)
        for bad in (raw.replace(b'pid: 123', b'pid: 124'), raw.replace(b'events: Ir', b'events: Ir Dr'),
                    raw.replace(b'1 7', b'1 6'), raw.replace(b'totals: 7', b'totals: 6')):
            with self.subTest(raw=bad), self.assertRaises(ValueError):
                sentence.checked_profile(bad, Path('/tmp/bisect'), 123, Path('/lib.so'))

    def test_wrong_cell_is_refused_before_output_or_workers(self):
        args = types.SimpleNamespace(main=Path('/main'), pr=Path('/pr'), pin=Path('/pin'), cell='bdb')
        with mock.patch.object(sentence.function_order, 'library_backend', return_value='tkrzw'), \
                mock.patch.object(bench, 'capture') as capture, contextlib.redirect_stderr(io.StringIO()), \
                self.assertRaises(Finished):
            sentence.prepare(args, {})
        capture.assert_not_called()


if __name__ == '__main__':
    unittest.main()
