#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Native startup worker (tools/workers/startup.c) against a synthetic library.

Standard library only. Uses cc; no oxpinyin or pin library is loaded.
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SOURCE = HERE / 'startup.c'
POINTS = ('start', 'lib_load', 'init', 'config', 'alloc', 'first_candidate', 'end')
SCRIPT_INPUTS = ('nihao', 'ni', 'n', 'zh', 'zhongguo', "xi'an", "bu'tian", "fan'gan", 'fangan',
                 'chongke', 'caisho', 'paolen', 'waimenggu', 'naoxion', 'liangniejue',
                 'chuaipengdengzaimiu', 'a', 'nihao')
CHEWING_INPUTS = ('su3cl3', 'ji3', '5j/ ')
HOSTILE_INPUTS = ('', "'", "''", "'ni", "ni'", "nihao'", 'zzzz', '12345', 'ni hao', 'NIHAO', '你好',
                  'nihao' * 15)
TIMES = ('lib_load', 'init', 'config', 'alloc', 'first_candidate_phase', 'ttfc',
         'setup_plus_first_candidate', 'drop_instance', 'fini', 'worker')

# A deterministic stand-in for both facades. Every call appends one line to
# STUB_LOG, naming the context or instance it reached, so tests can check
# order and which instance saw which input. STUB_FAIL selects one failure.
STUB = r'''
#define _POSIX_C_SOURCE 200809L
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
struct context { int id; unsigned live; };
struct instance { struct context *context; int id; size_t length; char input[512]; };
static int contexts, instances;
static const char *texts[] = {"你好", "尼", "泥"};
static void note(const char *format, ...) __attribute__((format(printf, 1, 2)));
#include <stdarg.h>
static void note(const char *format, ...) {
    const char *path = getenv("STUB_LOG");
    if (!path) return;
    FILE *log = fopen(path, "a");
    if (!log) abort();
    va_list args;
    va_start(args, format);
    vfprintf(log, format, args);
    va_end(args);
    fputc('\n', log);
    fclose(log);
}
static bool failing(const char *what) {
    const char *value = getenv("STUB_FAIL");
    return value && strcmp(value, what) == 0;
}
void *fake_init(const char *data, const char *user) {
    (void)data; (void)user;
    if (failing("init")) return NULL;
    struct context *c = calloc(1, sizeof *c);
    c->id = ++contexts;
    note("init ctx=%d", c->id);
    return c;
}
void fake_fini(void *p) { struct context *c = p; if (c->live) abort(); note("fini ctx=%d", c->id); free(c); }
bool fake_options(void *p, uint32_t flags) { struct context *c = p; note("set_options ctx=%d %#x", c->id, flags); return !failing("options") && flags == 0x18a; }
void *fake_alloc(void *p) {
    struct context *c = p;
    if (failing("alloc")) return NULL;
    struct instance *i = calloc(1, sizeof *i);
    i->context = c; i->id = ++instances; c->live++;
    note("alloc ctx=%d inst=%d", c->id, i->id);
    return i;
}
void fake_free(void *p) { struct instance *i = p; i->context->live--; note("free inst=%d", i->id); free(i); }
static size_t parse(void *p, const char *input, const char *kind) {
    struct instance *i = p;
    const char *abort_on = getenv("STUB_ABORT_ON");
    if (abort_on && strcmp(abort_on, input) == 0) abort();
    note("%s inst=%d [%s]", kind, i->id, input);
    i->length = strlen(input);
    snprintf(i->input, sizeof i->input, "%s", input);
    return i->length;
}
size_t fake_parse(void *p, const char *input) { return parse(p, input, "parse"); }
size_t fake_chewing(void *p, const char *input) { return parse(p, input, "chewing"); }
size_t fake_length(void *p) { struct instance *i = p; note("parsed_length inst=%d", i->id); return i->length; }
bool fake_reset(void *p) { struct instance *i = p; note("reset inst=%d", i->id); i->length = 0; return true; }
bool fake_guess_sentence(void *p) { struct instance *i = p; return i->length > 0; }
bool fake_guess(void *p, size_t offset, uint32_t sort) { struct instance *i = p; note("guess inst=%d %zu %#x", i->id, offset, sort); return offset == 0 && sort == 0x1e; }
bool fake_after(void *p, size_t offset) { struct instance *i = p; note("guess_after inst=%d %zu", i->id, offset); return offset == 0; }
bool fake_count(void *p, unsigned *count) {
    struct instance *i = p;
    note("count inst=%d", i->id);
    *count = failing("count0") || !i->length ? 0 : 3;
    return true;
}
bool fake_candidate(void *p, unsigned index, void **out) {
    struct instance *i = p;
    note("candidate inst=%d %u", i->id, index);
    if (index >= 3) return false;
    *out = (void *)&texts[index];
    return true;
}
bool fake_type(void *p, void *candidate, int *type) {
    (void)p;
    *type = candidate == (void *)&texts[0] ? 1 : 2;
    note("type");
    return true;
}
bool fake_string(void *p, void *candidate, const char **text) { (void)p; *text = *(const char **)candidate; note("string"); return true; }
bool fake_nbest(void *p, void *candidate, uint8_t *index) { (void)p; (void)candidate; *index = 1; return true; }
bool fake_save(void *p) { struct context *c = p; note("save ctx=%d", c->id); return !failing("save"); }
bool fake_load(void *p, uint8_t index) { (void)p; return index == 1; }
void *fake_begin(void *p, uint8_t index) { (void)index; return p; }
bool fake_add(void *p, const char *s, const char *r, int n) { (void)p; note("add [%s] [%s] %d", s, r, n); return n > 0; }
void fake_end(void *p) { (void)p; }
bool fake_sentence(void *p, uint8_t rank, char **text) { struct instance *i = p; *text = strdup(rank ? "尼好" : "你好"); return i->length > 0; }
bool fake_zhuyin_sentence(void *p, char **text) { *text = strdup("你好"); return p != NULL; }
void g_free(void *p) { free(p); }
'''

SIGNATURES = {
    'init': ('void *', 'const char *data, const char *user', 'data, user', 'init'),
    'fini': ('void', 'void *p', 'p', 'fini'),
    'alloc_instance': ('void *', 'void *p', 'p', 'alloc'),
    'free_instance': ('void', 'void *p', 'p', 'free'),
    'set_options': ('bool', 'void *p, uint32_t flags', 'p, flags', 'options'),
    'parse_more_full_pinyins': ('size_t', 'void *p, const char *input', 'p, input', 'parse'),
    'parse_more_chewings': ('size_t', 'void *p, const char *input', 'p, input', 'chewing'),
    'reset': ('bool', 'void *p', 'p', 'reset'),
    'guess_sentence': ('bool', 'void *p', 'p', 'guess_sentence'),
    'get_n_candidate': ('bool', 'void *p, unsigned *count', 'p, count', 'count'),
    'get_candidate': ('bool', 'void *p, unsigned index, void **out', 'p, index, out', 'candidate'),
    'get_candidate_type': ('bool', 'void *p, void *candidate, int *type', 'p, candidate, type', 'type'),
    'get_candidate_string': ('bool', 'void *p, void *candidate, const char **text', 'p, candidate, text', 'string'),
    'save': ('bool', 'void *p', 'p', 'save'),
    'load_phrase_library': ('bool', 'void *p, uint8_t index', 'p, index', 'load'),
    'begin_add_phrases': ('void *', 'void *p, uint8_t index', 'p, index', 'begin'),
    'iterator_add_phrase': ('bool', 'void *p, const char *s, const char *r, int n', 'p, s, r, n', 'add'),
    'end_add_phrases': ('void', 'void *p', 'p', 'end'),
}
FACADE_SIGNATURES = {
    'pinyin': {
        'guess_candidates': ('bool', 'void *p, size_t offset, uint32_t sort', 'p, offset, sort', 'guess'),
        'get_parsed_input_length': ('size_t', 'void *p', 'p', 'length'),
        'get_candidate_nbest_index': ('bool', 'void *p, void *candidate, uint8_t *index', 'p, candidate, index', 'nbest'),
        'get_sentence': ('bool', 'void *p, uint8_t rank, char **text', 'p, rank, text', 'sentence'),
    },
    'zhuyin': {
        'guess_candidates_after_cursor': ('bool', 'void *p, size_t offset', 'p, offset', 'after'),
        'get_sentence': ('bool', 'void *p, char **text', 'p, text', 'zhuyin_sentence'),
    },
}


def stub_exports(facade):
    return [facade + '_' + name for name in (*SIGNATURES, *FACADE_SIGNATURES[facade])]


def shared(root, name, source):
    cfile = root / (name + '.c')
    cfile.write_text(source)
    library = root / (name + '.so')
    link = ['-dynamiclib'] if sys.platform == 'darwin' else ['-shared']
    subprocess.run(['cc', *link, '-fPIC', '-Wall', '-Wextra', '-Werror', '-O2', '-o', str(library), str(cfile)],
                   check=True)
    return library


def build_stub(root, name='stub', stub_source=STUB):
    source = stub_source
    for facade in ('pinyin', 'zhuyin'):
        # Mach-O has no alias attribute, so export wrappers instead.
        for suffix, (returns, parameters, arguments, target) in {**SIGNATURES, **FACADE_SIGNATURES[facade]}.items():
            call = f'fake_{target}({arguments})'
            body = f'{call};' if returns == 'void' else f'return {call};'
            source += f'{returns} {facade}_{suffix}({parameters}) {{ {body} }}\n'
    return shared(root, name, source)


def build_worker(root):
    executable = root / 'startup'
    command = ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2', '-g', '-o', str(executable), str(SOURCE)]
    if sys.platform != 'darwin':
        command.append('-ldl')
    subprocess.run(command, check=True)
    return executable


def records(stdout):
    return [json.loads(line) for line in stdout.decode().splitlines()]


class WorkerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.root = Path(cls.directory.name)
        cls.worker = build_worker(cls.root)
        cls.stub = build_stub(cls.root)
        cls.data = cls.root / 'data'
        cls.data.mkdir()

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def setUp(self):
        self.user = Path(tempfile.mkdtemp(dir=self.root))
        self.log = self.user.parent / (self.user.name + '.log')

    def env(self, facade='pinyin', **extra):
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(('OXPINYIN_STARTUP_', 'STUB_'))}
        env.update(OXPINYIN_STARTUP_EXPORTS=','.join(stub_exports(facade)), STUB_LOG=str(self.log), **extra)
        return env

    def run_worker(self, mode, arg='-', facade='pinyin', library=None, bind='now', env=None, **kwargs):
        argv = [str(self.worker), mode, facade, str(library or self.stub), str(self.data), str(self.user), bind, arg]
        return subprocess.run(argv, capture_output=True, env=env or self.env(facade), timeout=30, **kwargs)

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def ok(self, *args, **kwargs):
        result = self.run_worker(*args, **kwargs)
        self.assertEqual(result.returncode, 0, result.stderr)
        return records(result.stdout)

    def refused(self, result, status):
        self.assertEqual(result.returncode, status, result.stderr)
        self.assertEqual(result.stdout, b'')

    # Refusals (§7.7): a non-zero exit with nothing on stdout.

    def test_missing_library_is_refused(self):
        self.refused(self.run_worker('measure', library=self.root / 'absent.so'), 1)

    def test_file_that_is_not_a_shared_library_is_refused(self):
        text = self.root / 'not-a-library.so'
        text.write_text('not a shared library\n')
        result = self.run_worker('measure', library=text)
        self.refused(result, 1)
        self.assertIn(b'dlopen', result.stderr)

    def test_library_without_symbols_names_every_missing_symbol(self):
        empty = shared(self.root, 'empty', 'int unrelated(void) { return 0; }\n')
        for facade in ('pinyin', 'zhuyin'):
            result = self.run_worker('measure', facade=facade, library=empty)
            self.refused(result, 1)
            named = [line.removeprefix('startup: missing symbol ')
                     for line in result.stderr.decode().splitlines() if 'missing symbol' in line]
            self.assertEqual(sorted(named), sorted({*stub_exports(facade), 'g_free'}))

    def test_bad_invocations_are_refused_with_status_two(self):
        cases = [
            ['measure', 'pinyin'],
            ['typing', 'pinyin', 'x', 'd', 'u', 'now', '-'],
            ['measure', 'wubi', 'x', 'd', 'u', 'now', '-'],
            ['measure', 'pinyin', 'x', 'd', 'u', 'eager', '-'],
            ['stop', 'pinyin', 'x', 'd', 'u', 'now', 'teardown'],
            ['probe', 'pinyin', 'x', 'd', 'u', 'now', '12'],
            ['probe', 'pinyin', 'x', 'd', 'u', 'now', 'one'],
            ['persist', 'pinyin', 'x', 'd', 'u', 'now', 'rows'],
            ['measure', 'pinyin', 'x', 'd', 'u', 'now', 'init,parse'],
            ['measure', 'pinyin', 'x', 'd', 'u', 'now', 'init,init'],
            ['measure', 'pinyin', 'x', 'd', 'u', 'now', 'init,'],
            ['measure', 'pinyin', 'x', 'd', 'u', 'now', 'init'],
        ]
        for argv in cases:
            with self.subTest(argv=argv):
                result = subprocess.run([str(self.worker), *argv], capture_output=True, env=self.env(), timeout=30)
                self.refused(result, 2)
        env = self.env()
        del env['OXPINYIN_STARTUP_EXPORTS']
        self.refused(self.run_worker('measure', env=env), 2)

    def test_failed_unit_is_refused(self):
        for failure in ('init', 'options', 'alloc', 'count0'):
            with self.subTest(failure=failure):
                self.refused(self.run_worker('measure', env=self.env(STUB_FAIL=failure)), 1)

    def test_missing_witness_file_is_a_failure(self):
        self.refused(self.run_worker('verify', str(self.root / 'absent-witness.txt')), 1)

    # Units and records.

    def test_units_run_in_order_and_teardown_last(self):
        self.ok('measure')
        self.assertEqual(self.calls(), [
            'init ctx=1', 'set_options ctx=1 0x18a', 'alloc ctx=1 inst=1', 'parse inst=1 [nihao]',
            'parsed_length inst=1', 'guess inst=1 0 0x1e', 'count inst=1', 'candidate inst=1 0', 'type', 'string',
            'free inst=1', 'fini ctx=1'])

    def test_zhuyin_units_use_its_own_guess_and_no_parsed_length(self):
        rows = self.ok('measure', facade='zhuyin')
        self.assertEqual(self.calls(), [
            'init ctx=1', 'set_options ctx=1 0x18a', 'alloc ctx=1 inst=1', 'parse inst=1 [nihao]',
            'guess_after inst=1 0', 'count inst=1', 'candidate inst=1 0', 'type', 'string',
            'free inst=1', 'fini ctx=1'])
        self.assertIsNone(rows[1]['parsed'])

    def test_canonical_record_is_shared_by_verify_and_measure(self):
        for facade in ('pinyin', 'zhuyin'):
            lines = set()
            for mode, bind in (('measure', 'now'), ('measure', 'lazy'), ('verify', 'now')):
                result = self.run_worker(mode, facade=facade, bind=bind)
                self.assertEqual(result.returncode, 0, result.stderr)
                units = [line for line in result.stdout.splitlines() if b'"startup":"units"' in line]
                self.assertEqual(len(units), 1)
                lines.add(units[0])
            self.assertEqual(len(lines), 1)
            line = lines.pop()
            self.assertNotIn(str(self.user).encode(), line)
            self.assertNotIn(str(self.stub).encode(), line)
            self.assertEqual(len(hashlib.sha256(line).hexdigest()), 64)
            record = json.loads(line)
            self.assertEqual(record['stopped_at'], 'end')
            self.assertEqual((record['consumed'], record['count'], record['text'], record['teardown']),
                             (5, 3, '你好', True))

    def test_run_record_names_mode_and_bind(self):
        self.assertEqual(self.ok('measure', bind='lazy')[0],
                         dict(startup='run', mode='measure', facade='pinyin', bind='lazy'))

    def test_times_carry_every_metric_and_ttfc_spans_the_phases(self):
        rows = self.ok('measure')
        times = rows[2]
        self.assertEqual(times['startup'], 'times')
        self.assertEqual(sorted(key for key in times if key != 'startup'), sorted(TIMES))
        phases = sum(times[name]['wall_ns'] for name in ('lib_load', 'init', 'config', 'alloc', 'first_candidate_phase'))
        self.assertEqual(times['setup_plus_first_candidate']['wall_ns'], phases)
        self.assertGreaterEqual(times['ttfc']['wall_ns'], phases)

    def test_stop_points_truncate_the_units(self):
        expected_calls = {'start': 0, 'lib_load': 0, 'init': 1, 'config': 2, 'alloc': 3, 'first_candidate': 10, 'end': 12}
        for point in POINTS:
            with self.subTest(point=point):
                self.log.unlink(missing_ok=True)
                rows = self.ok('stop', point)
                self.assertEqual(rows[1]['stopped_at'], point)
                self.assertEqual(len(self.calls()), expected_calls[point])
                self.assertEqual('teardown' in rows[1], point == 'end')
        self.assertEqual(sorted(self.ok('stop', 'start')[1]), ['facade', 'startup', 'stopped_at'])

    # Verification transcript (§7.3, §7.7).

    def test_transcript_uses_a_fresh_instance_after_the_units(self):
        witness = self.root / 'witness.txt'
        witness.write_text('ceshi\n\nzhongwen\n')
        rows = self.ok('verify', str(witness))
        calls = self.calls()
        unit_parses = [line for line in calls if line.startswith('parse inst=1')]
        self.assertEqual(unit_parses, ['parse inst=1 [nihao]'])
        self.assertLess(calls.index('fini ctx=1'), calls.index('init ctx=2'))
        transcript = [row for row in rows if row['startup'] == 'transcript']
        self.assertEqual([row['input'] for row in transcript], [*SCRIPT_INPUTS, 'ceshi', 'zhongwen'])
        self.assertTrue(all(line.startswith(('parse inst=2', 'parsed_length inst=2', 'reset inst=2', 'guess inst=2',
                                             'count inst=2', 'candidate inst=2', 'type', 'string', 'free inst=2',
                                             'fini ctx=2', 'init ctx=2', 'set_options ctx=2', 'alloc ctx=2'))
                            for line in calls[calls.index('init ctx=2'):]))
        after = [row for row in rows if row['startup'] == 'after_units']
        self.assertEqual(len(after), 1)
        self.assertEqual([candidate['text'] for candidate in after[0]['candidates']], ['你好', '尼', '泥'])

    def test_pinyin_records_nbest_and_extra_sentences(self):
        first = [row for row in self.ok('verify') if row['startup'] == 'transcript'][0]
        self.assertEqual([candidate['nbest'] for candidate in first['candidates']], [1, None, None])
        self.assertEqual(first['sentences'], [dict(rank=1, ok=True, text='尼好')])
        self.assertEqual((first['sentence_ok'], first['sentence'], first['parsed']), (True, '你好', 5))

    def test_zhuyin_records_chewing_inputs_and_no_nbest(self):
        rows = [row for row in self.ok('verify', facade='zhuyin') if row['startup'] == 'transcript']
        self.assertEqual([row['input'] for row in rows], [*SCRIPT_INPUTS, *CHEWING_INPUTS])
        self.assertEqual([row['parser'] for row in rows[-3:]], ['chewing'] * 3)
        self.assertEqual({candidate['nbest'] for row in rows for candidate in row['candidates']},
                         {'not available on this facade'})
        self.assertTrue(all(row['sentences'] == [] and row['parsed'] is None for row in rows))

    def test_unreadable_listed_candidate_is_recorded_as_none(self):
        source = STUB.replace('if (index >= 3) return false;', 'if (index >= 2) return false;')
        stub = self.stub
        try:
            type(self).stub = build_stub(self.root, 'short', source)
            first = [row for row in self.ok('verify') if row['startup'] == 'transcript'][0]
        finally:
            type(self).stub = stub
        self.assertEqual(first['candidates'][2], dict(index=2, none=True))

    # Hostile probes and persistence.

    def test_probe_observes_one_hostile_input_on_a_fresh_instance(self):
        for index, value in enumerate(HOSTILE_INPUTS):
            with self.subTest(index=index):
                self.log.unlink(missing_ok=True)
                rows = self.ok('probe', str(index))
                probes = [row for row in rows if row['startup'] == 'probe']
                self.assertEqual([row['input'] for row in probes], [value])
                self.assertIn(f'parse inst=2 [{value}]', self.calls())

    def test_probe_crash_is_a_signal_with_empty_stdout(self):
        result = self.run_worker('probe', '9', env=self.env(STUB_ABORT_ON='NIHAO'))
        self.assertEqual(result.returncode, -signal.SIGABRT)
        self.assertEqual(result.stdout, b'')

    def test_persist_imports_saves_reopens_and_observes(self):
        rows = self.ok('persist')
        ops = [(row['op'], row['value']) for row in rows if row['startup'] == 'persist' and 'op' in row]
        self.assertEqual(ops, [('init', True), ('set_options', True), ('load_phrase_library', True),
                               ('add_phrases', [True, True, True, False]), ('save', True), ('fini', True),
                               ('reopen', True)])
        self.assertEqual([row['input'] for row in rows if row['startup'] == 'persist' and 'input' in row],
                         ['ceshi', 'aihe', 'a', 'cece', 'nihao'])
        self.assertIn("add [测试] [ce4'shi4] 5", self.calls())

    def test_persist_save_false_is_a_failure(self):
        self.refused(self.run_worker('persist', env=self.env(STUB_FAIL='save')), 1)

    # Handshake (§7.8 item 2).

    def test_handshake_blocks_at_each_point_in_order(self):
        to_parent, from_worker = os.pipe()
        to_worker, from_parent = os.pipe()
        env = self.env(OXPINYIN_STARTUP_HANDSHAKE=f'{from_worker},{to_worker}')
        argv = [str(self.worker), 'measure', 'pinyin', str(self.stub), str(self.data), str(self.user), 'now',
                ','.join(POINTS)]
        last_call = dict(start=None, lib_load=None, init='init ctx=1', config='set_options ctx=1 0x18a',
                         alloc='alloc ctx=1 inst=1', first_candidate='string', end='fini ctx=1')
        with subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                              pass_fds=(from_worker, to_worker)) as process:
            os.close(from_worker)
            os.close(to_worker)
            reader = os.fdopen(to_parent, 'r')
            seen = []
            for _ in POINTS:
                line = reader.readline()
                point = line.removeprefix('point ').strip()
                seen.append(point)
                calls = self.calls()
                self.assertEqual(calls[-1] if calls else None, last_call[point])
                os.write(from_parent, b'\n')
            stdout, stderr = process.communicate(timeout=30)
            reader.close()
            os.close(from_parent)
        self.assertEqual(process.returncode, 0, stderr)
        self.assertEqual(seen, list(POINTS))
        self.assertEqual(records(stdout)[1]['stopped_at'], 'end')

    def test_lost_acknowledgement_fails_with_empty_stdout(self):
        to_parent, from_worker = os.pipe()
        to_worker, from_parent = os.pipe()
        env = self.env(OXPINYIN_STARTUP_HANDSHAKE=f'{from_worker},{to_worker}')
        argv = [str(self.worker), 'measure', 'pinyin', str(self.stub), str(self.data), str(self.user), 'now', 'init']
        with subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                              pass_fds=(from_worker, to_worker)) as process:
            os.close(from_worker)
            os.close(to_worker)
            os.close(from_parent)
            stdout, stderr = process.communicate(timeout=30)
            os.close(to_parent)
        self.assertEqual(process.returncode, 1, stderr)
        self.assertEqual(stdout, b'')
        self.assertIn(b'acknowledgement lost at init', stderr)


if __name__ == '__main__':
    unittest.main()
