#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Utility observations, cancellation and complete policy binding."""
import json
from pathlib import Path
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import policy, pool, tool_diff
from tools.bisection import contract_policy, contract_verdict


class ToolDiffTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_stream_bytes_environment_stdin_and_all_files(self):
        context = pool.UnitContext('tool test', threading.Event())
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=context))
        case = dict(args=['-c', 'import os,sys; from pathlib import Path; '
                          'sys.stdout.buffer.write(sys.stdin.buffer.read()+b"\\xff"); '
                          'sys.stderr.write(os.environ["VALUE"]); '
                          'Path("nested").mkdir(); Path("nested/empty").touch(); '
                          'Path("input").write_bytes(b"changed"); sys.exit(2)'], env={'VALUE': 'diagnostic', 'LC_ALL': 'C'})
        result = tool_diff.observation(ctx, Path(sys.executable), case, self.root / 'run',
                                      {'input': b'original'}, b'stdin', time.monotonic() + 5)
        self.assertEqual(result, dict(exit=2, stdout='stdin\udcff', stderr='diagnostic',
            files={'input': 'hex:' + b'changed'.hex(), 'nested/': 'directory', 'nested/empty': 'hex:'}))
        self.assertEqual(tool_diff.verdict({}, result, result, self.root, 'bdb')[0], 'MATCH')
        self.assertEqual(tool_diff.verdict({}, result, dict(result, stdout='stdin\ufffd'), self.root, 'bdb')[0], 'DIFFER')

    def test_masks_only_glib_pid_and_time(self):
        value = "\n** (process:123): CRITICAL **: 12:34:56.789: message 123\nfile:456 time 12:34:56.789\n"
        self.assertEqual(tool_diff.GLIB_PREFIX.sub(r'\1PID\2TIME: ', value),
            "\n** (process:PID): CRITICAL **: TIME: message 123\nfile:456 time 12:34:56.789\n")

    def test_glib_domain_prefix_keeps_domain_and_utility(self):
        value = "\n(gen_zhuyin_table:123): GLib-CRITICAL **: 12:34:56.789: assertion failed\n"
        self.assertEqual(tool_diff.GLIB_PREFIX.sub(r'\1PID\2TIME: ', value),
                         "\n(gen_zhuyin_table:PID): GLib-CRITICAL **: TIME: assertion failed\n")

    def test_policy_record_binds_both_complete_observations(self):
        case = dict(id='invalid', policy_record='record.md', pin_signal=-11)
        pin = dict(exit=-11, stdout='', stderr='', files={'out': 'hex:'})
        subject = dict(exit=1, stdout='nodes\n', stderr='refused\n', files={'out': 'hex:'})
        capture = self.root / 'capture.jsonl'
        row = dict(runner='tool-diff', case='invalid', cell='bdb', userdir=False, abort=False,
                   pin_signal=-11, database_free=False, pin=pin, subject=subject)
        capture.write_text(json.dumps(row)+'\n')
        record = self.root / 'record.md'
        record.write_text('# Class (b)\n')
        (self.root / 'upstream-divergences.md').write_text('[record](record.md)\n')
        policy.record(record, [capture], out=lambda _: None)
        self.assertEqual(tool_diff.verdict(case, pin, subject, self.root, 'bdb')[0], 'POLICY-ACCEPTED')
        for side in ('pin', 'subject'):
            pair = dict(pin=pin, subject=subject)
            pair[side] = dict(pair[side], files={'out': 'hex:00'})
            self.assertEqual(tool_diff.verdict(case, **pair, policy_root=self.root, cell='bdb')[0], 'DIFFER')
        self.assertEqual(tool_diff.verdict(case, pin, dict(subject, exit=-11), self.root, 'bdb')[0], 'INVALID')
        with self.assertRaises(policy.RecordError):
            policy.entry_of('bad', dict(row, subject=dict(subject, exit=-11)))
        self.assertIsNotNone(contract_policy.tool_observation_problem(dict(pin, exit='TIMEOUT'), pin_signal=-11))

    def test_a_reproduced_abort_compares_without_a_policy_record(self):
        case = dict(id='abort', pin_signal=-6, reproduces_signal=True)
        side = dict(exit=-6, stdout='', stderr='assertion\n', files={})
        self.assertEqual(tool_diff.verdict(case, side, side, self.root, 'bdb')[0], 'MATCH')
        self.assertEqual(tool_diff.verdict(case, side, dict(side, stderr=''), self.root, 'bdb')[0], 'DIFFER')
        self.assertEqual(tool_diff.verdict(case, side, dict(side, exit=-11), self.root, 'bdb')[0], 'INVALID')
        self.assertEqual(tool_diff.verdict(dict(case, reproduces_signal=None), side, side, self.root, 'bdb')[0],
                         'INVALID')

    def test_a_reproduced_abort_binds_its_stderr_difference(self):
        case = dict(id='abort', policy_record='record.md', pin_signal=-6, reproduces_signal=True)
        pin = dict(exit=-6, stdout='lines\n', stderr='assertion\n', files={})
        subject = dict(pin, stderr='')
        row = dict(runner='tool-diff', case='abort', cell='bdb', userdir=False, abort=False,
                   pin_signal=-6, reproduces_signal=True, database_free=False, pin=pin, subject=subject)
        capture = self.root / 'capture.jsonl'
        capture.write_text(json.dumps(row)+'\n')
        record = self.root / 'record.md'
        record.write_text('# Language artefact\n')
        (self.root / 'upstream-divergences.md').write_text('[record](record.md)\n')
        policy.record(record, [capture], out=lambda _: None)
        self.assertEqual(tool_diff.verdict(case, pin, subject, self.root, 'bdb')[0], 'POLICY-ACCEPTED')
        self.assertEqual(tool_diff.verdict(case, pin, dict(subject, stdout=''), self.root, 'bdb')[0], 'DIFFER')
        with self.assertRaises(policy.RecordError):
            policy.entry_of('bad', dict(row, reproduces_signal=False))

    def database_free_record(self, *rows):
        capture = self.root / 'capture.jsonl'
        capture.write_text(''.join(json.dumps(row) + '\n' for row in rows))
        record = self.root / 'record.md'
        if not record.exists():
            record.write_text('# Class (b)\n')
        (self.root / 'upstream-divergences.md').write_text('[record](record.md)\n')
        lines = []
        policy.record(record, [capture], out=lines.append)
        return record, lines

    def test_a_database_free_case_is_recorded_once_and_accepted_on_every_cell(self):
        case = dict(id='invalid', utility='gen_pinyin_table', policy_record='record.md', pin_signal=-11)
        pin = dict(exit=-11, stdout='', stderr='', files={'in.txt': 'hex:ff'})
        subject = dict(exit=1, stdout='', stderr='refused\n', files={'in.txt': 'hex:ff'})
        row = dict(runner='tool-diff', case='invalid', userdir=False, abort=False, pin_signal=-11,
                   reproduces_signal=False, database_free=True, pin=pin, subject=subject)
        record, lines = self.database_free_record(*(dict(row, cell=cell) for cell in ('bdb', 'kc', 'tkrzw')))
        self.assertIn('recorded invalid [every cell, from kc]: pin', '\n'.join(lines))
        block = json.loads(record.read_text().split('```contract-diff-policy\n')[1].split('\n```')[0])
        self.assertEqual(block['cases'], {'invalid': {'database_free': contract_policy.entry(pin, subject)}})
        for cell in ('bdb', 'kc', 'tkrzw'):
            self.assertEqual(tool_diff.verdict(case, pin, subject, self.root, cell)[0], 'POLICY-ACCEPTED')
        self.assertEqual(tool_diff.verdict(case, pin, dict(subject, stderr=''), self.root, 'kc')[0], 'DIFFER')
        # A case that opens a database refuses the entry, whichever runner reads it.
        for refused in (dict(case, utility='estimate_k_mixture_model'), dict(case, utility=None)):
            result, detail = tool_diff.verdict(refused, pin, subject, self.root, 'bdb')
            self.assertEqual(result, 'INVALID')
            self.assertIn('cannot stand for a case that opens a database', detail)
        with self.assertRaisesRegex(ValueError, 'opens a database'):
            contract_verdict.policy_expectations(dict(policy_record='record.md', case_id='invalid'),
                                                 policy_root=self.root, cell='bdb')

    def test_database_free_rows_must_agree_and_replace_per_cell_entries(self):
        pin = dict(exit=-11, stdout='', stderr='', files={})
        subject = dict(exit=1, stdout='', stderr='refused\n', files={})
        row = dict(runner='tool-diff', case='invalid', userdir=False, abort=False, pin_signal=-11,
                   reproduces_signal=False, pin=pin, subject=subject)
        record, _ = self.database_free_record(dict(row, cell='bdb', database_free=False),
                                              dict(row, cell='kc', database_free=False))
        before = record.read_text()
        self.assertIn('"cells"', before)
        with self.assertRaisesRegex(policy.RecordError, r'invalid \[kc\] digests differently from its \[bdb\] row'):
            self.database_free_record(dict(row, cell='bdb', database_free=True),
                                      dict(row, cell='kc', database_free=True, subject=dict(subject, stderr='')))
        self.assertEqual(record.read_text(), before)
        _, lines = self.database_free_record(dict(row, cell='bdb', database_free=True))
        self.assertIn('notice: invalid had per-cell or bare entries; one database-free entry replaces them', lines)
        self.assertNotIn('"cells"', record.read_text())
        for bad in (dict(row, cell='bdb'), dict(row, cell='bdb', database_free='yes')):
            with self.assertRaisesRegex(policy.RecordError, 'needs a boolean database_free'):
                policy.entry_of('bad', bad)
        with self.assertRaisesRegex(policy.RecordError, 'only a tool-diff row can be database-free'):
            policy.entry_of('bad', dict({k: v for k, v in row.items() if k != 'runner'}, cell='bdb', database_free=True))

    def test_database_free_names_registered_utilities(self):
        self.assertLessEqual(tool_diff.DATABASE_FREE, set(tool_diff.PIN_UTILITIES))

    def test_case_file_names_registered_utilities_and_reproducible_signals(self):
        path = self.root / 'cases.json'
        case = dict(id='one', utility='k_mixture_model_to_interpolation', source='pin.cpp:1')
        for bad, message in ((dict(utility='gen_ngram'), 'no registered pin build directory'),
                             (dict(pin_signal=-11, reproduces_signal=True), 'only a registered SIGABRT'),
                             (dict(pin_signal=-6), 'requires a named policy record')):
            path.write_text(json.dumps(dict(schema=1, cases=[dict(case, **bad)])))
            with self.assertRaisesRegex(ValueError, message):
                tool_diff.load_cases([path])
        path.write_text(json.dumps(dict(schema=1, cases=[dict(case, pin_signal=-6, reproduces_signal=True)])))
        self.assertEqual(len(tool_diff.load_cases([path])), 1)

    def test_deadline_reaps_the_child_and_cannot_match(self):
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=pool.UnitContext('timeout', threading.Event())))
        with self.assertRaises(pool.UnitStopped):
            tool_diff.observation(ctx, Path(sys.executable), dict(args=['-c', 'import time; time.sleep(10)']),
                                  self.root / 'run', {}, b'', time.monotonic() + .05)

    def test_case_file_rejects_duplicates_and_escaping_input(self):
        path = self.root / 'cases.json'
        case = dict(id='one', utility='gen_pinyin_table', source='pin.cpp:1')
        path.write_text(json.dumps(dict(schema=1, cases=[case, case])))
        with self.assertRaisesRegex(ValueError, 'unique'):
            tool_diff.load_cases([path])
        path.write_text(json.dumps(dict(schema=1, cases=[dict(case, files={'../escape': {'text': ''}})])))
        with self.assertRaisesRegex(ValueError, 'working directory'):
            tool_diff.load_cases([path])

    def test_fixture_directory_is_the_cells_committed_data_set(self):
        files = tool_diff.fixture_inputs(ROOT, 'tkrzw', 'fixtures/w3')
        self.assertIn('table.conf', files)
        self.assertIn('phrase_index.bin', files)
        self.assertIn('bigram.db', files)
        self.assertTrue(files['table.conf'].startswith(b'binary format version:'))
        bdb = tool_diff.fixture_inputs(ROOT, 'bdb', 'fixtures/w3')
        kc = tool_diff.fixture_inputs(ROOT, 'kc', 'fixtures/w3')
        self.assertEqual(set(files), set(bdb))
        self.assertEqual(set(files), set(kc))
        self.assertNotEqual(files['phrase_index.bin'], bdb['phrase_index.bin'])
        with self.assertRaisesRegex(ValueError, 'no tkt directory'):
            tool_diff.fixture_inputs(self.root, 'tkrzw', 'fixtures/w3')

    def test_case_file_validates_fixture_directory(self):
        path = self.root / 'cases.json'
        case = dict(id='one', utility='ngseg', source='pin.cpp:1', fixture_dir='fixtures/w3')
        path.write_text(json.dumps(dict(schema=1, cases=[case])))
        self.assertEqual(len(tool_diff.load_cases([path])), 1)
        for bad in ('../fixtures/w3', '/fixtures/w3', 7):
            path.write_text(json.dumps(dict(schema=1, cases=[dict(case, fixture_dir=bad)])))
            with self.assertRaises((ValueError, TypeError)):
                tool_diff.load_cases([path])

    def test_model_fixture_reorders_fields_and_uses_committed_zhuyin_spelling(self):
        model = self.root / 'model'
        model.mkdir()
        (model / 'tiny.table').write_text("ni3'hao3\t你好\t17\t5\n")
        self.assertEqual(tool_diff.payload(dict(model_table='tiny.table', format='zhuyin'), ROOT, model),
                         "你好 5 ㄋㄧˇ'ㄏㄠˇ\n".encode())
        self.assertEqual(tool_diff.payload(dict(model_table='tiny.table', format='pinyin'), ROOT, model),
                         "你好 ni3'hao3 5\n".encode())


if __name__ == '__main__':
    unittest.main()
