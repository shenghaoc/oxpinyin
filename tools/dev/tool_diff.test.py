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
from tools.bisection import contract_policy


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
                   pin_signal=-11, pin=pin, subject=subject)
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
