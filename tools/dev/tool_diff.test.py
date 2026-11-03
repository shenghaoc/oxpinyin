#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Utility observations, cancellation and complete policy binding."""
import json
import os
from pathlib import Path
import shutil
import subprocess
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

    def test_tkrzw_abort_timestamp_masks_the_whole_field_only(self):
        original = bytearray(b'TkrzwHDB\n' + bytes(range(128)))
        def observed(data):
            files = {'tmp/test.db': 'hex:' + data.hex()}
            tool_diff.mask_tkrzw_abort_timestamps(files, ['tmp/test.db'], 'tkrzw')
            return dict(exit=-6, stdout='', stderr='', files=files)
        expected = observed(original)
        changed = original.copy()
        changed[48:56] = bytes(value ^ 255 for value in changed[48:56])
        case = dict(pin_signal=-6, reproduces_signal=True)
        self.assertEqual(tool_diff.verdict(case, expected, observed(changed), self.root, 'tkrzw')[0], 'MATCH')
        masked = bytes.fromhex(expected['files']['tmp/test.db'][4:])
        self.assertEqual(masked, original[:48] + bytes(8) + original[56:])
        for offset in (47, 56):
            changed = original.copy()
            changed[offset] ^= 255
            self.assertEqual(tool_diff.verdict(case, expected, observed(changed), self.root, 'tkrzw')[0], 'DIFFER')

    def test_tkrzw_timestamp_mask_requires_observed_abort(self):
        original = b'TkrzwHDB\n' + bytes(range(128))
        name = 'tmp/test.db'
        case = dict(utility='test_ngram', pin_signal=-6, reproduces_signal=True,
                    tkrzw_abort_timestamp=[name])
        ctx = SimpleNamespace(env={}, settings=SimpleNamespace(cell='tkrzw'),
            services=SimpleNamespace(context=pool.UnitContext('status', threading.Event())))
        for index, status in enumerate((0, -11, -6)):
            with patch.object(tool_diff, 'isolated_launch', return_value=pool.Completed(status, b'', b'')), \
                    patch.object(tool_diff, 'snapshot', return_value={name: 'hex:' + original.hex()}):
                result = tool_diff.observation(ctx, Path(sys.executable), case,
                    self.root / str(index), {}, b'', time.monotonic() + 5)
            actual = bytes.fromhex(result['files'][name][4:])
            expected = original[:48] + bytes(8) + original[56:] if status == -6 else original
            self.assertEqual(actual, expected)

    def test_null_policy_cells_uses_default_scope(self):
        case = dict(id='null-scope', utility='test_ngram', source='test_ngram.cpp:40',
                    policy_record='absent.md', policy_cells=None)
        path = self.root / 'null-policy.json'
        path.write_text(json.dumps(dict(schema=1, cases=[case])))
        self.assertEqual(tool_diff.load_cases([path]), [case])
        side = dict(exit=0, stdout='', stderr='', files={})
        self.assertEqual(tool_diff.verdict(case, side, side, self.root, 'bdb')[0], 'INVALID')
        self.assertEqual(tool_diff.verdict(dict(case, policy_cells=[]), side, side, self.root, 'bdb')[0], 'MATCH')

    def test_tkrzw_abort_timestamp_leaves_other_files_and_cells_alone(self):
        for cell, data in (('kc', b'TkrzwHDB\n' + bytes(range(128))),
                           ('bdb', b'TkrzwHDB\n' + bytes(range(128))),
                           ('tkrzw', b'not-tkrzw' + bytes(range(128))),
                           ('tkrzw', b'TkrzwHDB\n')):
            files = {'named': 'hex:' + data.hex(), 'other': 'hex:' + data.hex()}
            expected = files.copy()
            tool_diff.mask_tkrzw_abort_timestamps(files, ['named'], cell)
            self.assertEqual(files, expected)
        files = {'other': 'hex:' + (b'TkrzwHDB\n' + bytes(range(128))).hex()}
        expected = files.copy()
        tool_diff.mask_tkrzw_abort_timestamps(files, ['named'], 'tkrzw')
        self.assertEqual(files, expected)

    def test_tkrzw_abort_timestamp_requires_named_reproduced_abort(self):
        case = dict(id='abort', utility='test_ngram', source='test_ngram.cpp:78',
                    pin_signal=-6, reproduces_signal=True, tkrzw_abort_timestamp=['tmp/test.db'])
        path = self.root / 'case.json'
        path.write_text(json.dumps(dict(schema=1, cases=[case])))
        self.assertEqual(tool_diff.load_cases([path]), [case])
        for fields in (dict(pin_signal=None), dict(reproduces_signal=False),
                       dict(tkrzw_abort_timestamp=True), dict(tkrzw_abort_timestamp=[]),
                       dict(tkrzw_abort_timestamp=['../escape'])):
            path.write_text(json.dumps(dict(schema=1, cases=[dict(case, **fields)])))
            with self.assertRaises(ValueError):
                tool_diff.load_cases([path])

    def test_pinyin_index_mask_keeps_tokens_keys_order_and_even_items(self):
        # N=1: token + key + two padding bytes, with two items in stored order.
        original = b'0100 010000000100abcd0200000001001234\n'
        expected = b'0100 01000000010000000200000001000000\n'
        self.assertEqual(tool_diff.pinyin_index_rows(original), expected)
        for changed in (original.replace(b'01000000', b'03000000', 1),
                        original.replace(b'0100abcd', b'0200abcd'),
                        b'0100 0200000001001234010000000100abcd\n'):
            self.assertNotEqual(tool_diff.pinyin_index_rows(changed), expected)
        for untouched in (b'01000200 0100000001000200\n', b'0100 \n'):
            self.assertEqual(tool_diff.pinyin_index_rows(untouched), untouched)
        # N=3: ten field bytes in a twelve-byte item.
        self.assertEqual(tool_diff.pinyin_index_rows(b'010002000300 01000000010002000300abcd\n'),
                         b'010002000300 010000000100020003000000\n')
        for malformed in (b'01 010000000100abcd\n', b'0100 010000000100ab\n'):
            with self.assertRaises(ValueError):
                tool_diff.pinyin_index_rows(malformed)
        case = dict(dbm_rows_pinyin_index=['pinyin_index.bin'])
        observation = dict(exit=0, stdout='', stderr='', files={'pinyin_index.bin': 'rows:digest'})
        self.assertEqual(tool_diff.verdict(case, observation, observation, self.root, 'tkrzw')[0],
                         'POLICY-ACCEPTED')
        for changed in (dict(observation, stdout='changed'),
                        dict(observation, files={'pinyin_index.bin': 'rows:changed'})):
            self.assertEqual(tool_diff.verdict(case, observation, changed, self.root, 'tkrzw')[0], 'DIFFER')

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

    @unittest.skipUnless(sys.platform == "linux", "Linux sealed memory files")
    def test_sealed_input_can_be_read_but_not_truncated(self):
        context = pool.UnitContext('sealed input', threading.Event())
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=context))
        code = ("from pathlib import Path\nimport sys\np=Path('chunk')\n"
                "assert p.read_bytes()==b'data'\ntry:\n p.write_bytes(b'changed')\n"
                "except OSError:\n sys.exit(2)\n")
        case = dict(args=['-c', code], sealed_inputs=['chunk'])
        result = tool_diff.observation(ctx, Path(sys.executable), case, self.root / 'sealed',
                                       {'chunk': b'data'}, b'', time.monotonic() + 5)
        self.assertEqual(result['exit'], 2)
        self.assertEqual(result['files']['chunk'], 'hex:64617461')

    def test_expected_failure_rejects_two_successful_observations(self):
        case = dict(expected_failure=dict(exit=2, stderr='save chunk failed.'))
        success = dict(exit=0, stdout='', stderr='', files={})
        failure = dict(exit=2, stdout='', stderr='save chunk failed.', files={})
        self.assertEqual(tool_diff.verdict(case, success, success, self.root, 'tkrzw')[0], 'DIFFER')
        self.assertEqual(tool_diff.verdict(case, failure, failure, self.root, 'tkrzw')[0], 'MATCH')
        self.assertEqual(tool_diff.verdict(case, failure, success, self.root, 'tkrzw')[0], 'DIFFER')

    def test_masks_only_glib_pid_and_time(self):
        value = "\n** (process:123): CRITICAL **: 12:34:56.789: message 123\nfile:456 time 12:34:56.789\n"
        self.assertEqual(tool_diff.GLIB_PREFIX.sub(r'\1PID\2TIME: ', value),
            "\n** (process:PID): CRITICAL **: TIME: message 123\nfile:456 time 12:34:56.789\n")

    def test_print_time_masks_only_clock_fields_and_preserves_other_bytes(self):
        context = pool.UnitContext('timer', threading.Event())
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=context))
        lines = [b'Spent 10 us for 1000 operations, 0.010000 us/op, 100000000.000000 times/s.\n\n',
                 b'Spent 0 us for 1000 operations, 0.000000 us/op, inf times/s.\n\n']
        observations = []
        for index, line in enumerate(lines):
            case = dict(utility='test_parser2', mask_print_time=True, args=['-c',
                'import sys; sys.stdout.buffer.write(' + repr(b'before\xff\n' + line + b'after\n') + '); '
                'sys.stderr.write("Spent 99 us")'])
            observations.append(tool_diff.observation(ctx, Path(sys.executable), case,
                self.root / str(index), {}, b'', time.monotonic() + 5))
        self.assertEqual(observations[0], observations[1])
        self.assertEqual(observations[0]['stdout'],
            'before\udcff\nSpent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\nafter\n')
        self.assertEqual(observations[0]['stderr'], 'Spent 99 us')
        mask = lambda text: tool_diff.PRINT_TIME.sub('TIME for \\1\n', text.decode())
        original = mask(lines[0])
        for changed in (lines[0].replace(b'1000 operations', b'999 operations'),
                        lines[0][:-1], lines[0] + lines[0], b'prefix ' + lines[0]):
            self.assertNotEqual(mask(changed), original)

    def test_timer_mask_rejects_unrelated_programs_and_nonbooleans(self):
        path = self.root / 'case.json'
        for utility, enabled in (('test_pinyin', True), ('test_parser2', 1)):
            path.write_text(json.dumps(dict(schema=1, cases=[dict(id='time', utility=utility,
                source='tests/timer.h:36-44', mask_print_time=enabled)])))
            with self.assertRaisesRegex(ValueError, 'mask_print_time'):
                tool_diff.load_cases([path])

    def test_nested_cwd_observes_parent_inputs_and_writes(self):
        context = pool.UnitContext('nested tool', threading.Event())
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=context))
        case = dict(cwd='tests', args=['-c',
            'from pathlib import Path; import sys; '
            'sys.stdout.buffer.write(Path("../data/input").read_bytes()); '
            'Path("../data/output").write_bytes(b"saved")'])
        result = tool_diff.observation(ctx, Path(sys.executable), case, self.root / 'run',
            {'data/input': b'fixed path'}, b'', time.monotonic() + 5)
        self.assertEqual(result, dict(exit=0, stdout='fixed path', stderr='', files={
            'data/': 'directory', 'data/input': 'hex:' + b'fixed path'.hex(),
            'data/output': 'hex:' + b'saved'.hex(), 'tests/': 'directory'}))

    def test_two_level_cwd_keeps_each_sides_data_separate(self):
        context = pool.UnitContext('two levels', threading.Event())
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=context))
        case = dict(cwd='tests/storage', args=['-c',
            'from pathlib import Path; import sys; '
            'sys.stdout.buffer.write(Path("../../data/input").read_bytes()); '
            'Path("../../data/saved").write_bytes(b"saved")'])
        for side in ('pin', 'subject'):
            result = tool_diff.observation(ctx, Path(sys.executable), case, self.root / side,
                {'data/input': side.encode()}, b'', time.monotonic() + 5)
            self.assertEqual(result['stdout'], side)
            self.assertEqual(result['files']['data/saved'], 'hex:' + b'saved'.hex())
        self.assertEqual((self.root / 'pin/data/input').read_bytes(), b'pin')
        self.assertEqual((self.root / 'subject/data/input').read_bytes(), b'subject')

    def require_namespaces(self):
        wrapper = shutil.which('bwrap')
        if wrapper is None:
            self.fail('bubblewrap is not installed')
        probe = subprocess.run([wrapper, '--unshare-user', '--unshare-pid', '--ro-bind', '/', '/',
                                '--tmpfs', '/tmp', '--', '/usr/bin/true'], capture_output=True)
        if probe.returncode:
            self.fail('user namespaces unavailable: ' + probe.stderr.decode())

    @unittest.skipUnless(os.environ.get('OXPINYIN_TEST_NAMESPACES') == '1',
                         'needs bubblewrap and user namespaces (set OXPINYIN_TEST_NAMESPACES=1)')
    def test_private_tmp_retains_exact_exit_status_streams_and_files(self):
        self.require_namespaces()
        context = pool.UnitContext('private tmp', threading.Event())
        ctx = SimpleNamespace(env=dict(os.environ), services=SimpleNamespace(context=context))
        for index, (code, status) in enumerate((('raise SystemExit(134)', 134), ('os.abort()', -6))):
            case = dict(utility='test_ngram', cwd='tests/storage', args=['-c',
                'import os,sys; from pathlib import Path; '
                'Path("/tmp/test.db").write_bytes(b"saved"); '
                'sys.stdout.buffer.write(sys.stdin.buffer.read()); sys.stdout.flush(); '
                'sys.stderr.write("diagnostic"); sys.stderr.flush(); ' + code])
            result = tool_diff.observation(ctx, Path(sys.executable), case, self.root / str(index),
                {}, b'input\xff', time.monotonic() + 10)
            self.assertEqual((result['exit'], result['stdout'], result['stderr']),
                             (status, 'input\udcff', 'diagnostic'))
            self.assertEqual(result['files']['tmp/test.db'], 'hex:' + b'saved'.hex())

    def test_private_tmp_with_sealed_input_fails_explicitly(self):
        context = pool.UnitContext('private sealed input', threading.Event())
        ctx = SimpleNamespace(env=dict(os.environ), services=SimpleNamespace(context=context))
        case = dict(utility='test_ngram', sealed_inputs=['chunk'])
        with self.assertRaisesRegex(ValueError, 'sealed inputs cannot be combined'):
            tool_diff.observation(ctx, Path(sys.executable), case, self.root / 'sealed-private',
                                  {'chunk': b'data'}, b'', time.monotonic() + 10)

    def test_private_tmp_failure_never_becomes_a_matching_observation(self):
        context = pool.UnitContext('refused namespace', threading.Event())
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=context))
        with patch.object(tool_diff.shutil, 'which', return_value='/usr/bin/bwrap'), \
                patch.object(pool.UnitContext, 'launch', return_value=pool.Completed(1, b'', b'refused')):
            with self.assertRaisesRegex(ValueError, 'private /tmp launcher failed: refused'):
                tool_diff.observation(ctx, Path(sys.executable), dict(utility='test_ngram'),
                                      self.root / 'run', {}, b'', time.monotonic() + 5)

    @unittest.skipUnless(os.environ.get('OXPINYIN_TEST_NAMESPACES') == '1',
                         'needs bubblewrap and user namespaces (set OXPINYIN_TEST_NAMESPACES=1)')
    def test_separate_runner_processes_cannot_share_fixed_tmp_files(self):
        self.require_namespaces()
        # Each subprocess is an independent runner invocation, not a worker
        # sharing this process's locks. Both tools write before either reads.
        program = ('from pathlib import Path; import sys,time; '
                   'Path("/tmp/test.db").write_text(sys.argv[1]); Path("ready").touch(); '
                   '\nwhile not Path(sys.argv[2]).exists(): time.sleep(.01)\n'
                   'sys.stdout.write(Path("/tmp/test.db").read_text())')
        runner = ('import json,os,sys,threading,time; from pathlib import Path; '
                  'from types import SimpleNamespace; from tools.dev import pool,tool_diff; '
                  'config=json.loads(sys.argv[1]); '
                  'ctx=SimpleNamespace(env=dict(os.environ),services=SimpleNamespace('
                  'context=pool.UnitContext("independent",threading.Event()))); '
                  'print(json.dumps(tool_diff.observation(ctx,Path(sys.executable),'
                  'config["case"],Path(config["root"]),{},b"",time.monotonic()+10)))')
        # The barrier must stay outside the private /tmp mount as well.
        with tempfile.TemporaryDirectory(dir=ROOT) as scratch:
            root = Path(scratch)
            barrier = root / 'go'
            processes = []
            try:
                for side in ('pin', 'subject'):
                    config = dict(root=str(root / side), case=dict(utility='test_ngram',
                        args=['-c', program, side, str(barrier)]))
                    processes.append(subprocess.Popen([sys.executable, '-c', runner, json.dumps(config)],
                        cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE))
                deadline = time.monotonic() + 10
                while not all((root / side / 'ready').exists() for side in ('pin', 'subject')):
                    if time.monotonic() >= deadline or any(p.poll() is not None for p in processes):
                        self.fail('both isolated tools did not reach the shared read barrier')
                    time.sleep(.01)
                barrier.touch()
                for side, process in zip(('pin', 'subject'), processes):
                    stdout, stderr = process.communicate(timeout=10)
                    self.assertEqual((process.returncode, stderr), (0, b''))
                    result = json.loads(stdout)
                    self.assertEqual(result['stdout'], side)
                    self.assertEqual(result['files']['tmp/test.db'], 'hex:' + side.encode().hex())
            finally:
                for process in processes:
                    if process.poll() is None:
                        process.kill()
                    process.communicate()

    def test_fixed_name_overlap_batches_only_colliding_cases(self):
        utilities = ['test_ngram', 'test_pinyin', 'test_punct_table', 'test_table_info',
                     'test_ngram', 'test_phrase']
        cases = [dict(id=str(index), utility=name) for index, name in enumerate(utilities)]
        batches = tool_diff.case_batches(cases)
        self.assertEqual(sorted([case['id'] for case in batch] for batch in batches),
                         [['0', '2', '4'], ['1'], ['3'], ['5']])

    def test_nested_paths_cannot_escape_case_root(self):
        for field in ('cwd', 'fixture_prefix'):
            for value in ('../data', '/tmp/data', 1):
                path = self.root / 'case.json'
                path.write_text(json.dumps(dict(schema=1, cases=[dict(
                    id='nested', utility='test_pinyin', source='tests/test_pinyin.cpp:36',
                    **{field: value})])))
                with self.assertRaises(ValueError):
                    tool_diff.load_cases([path])

    def test_user_snapshot_bytes_remain_observed_outside_dbm_row_cells(self):
        context = pool.UnitContext('snapshot tool', threading.Event())
        ctx = SimpleNamespace(env={}, services=SimpleNamespace(context=context),
                              settings=SimpleNamespace(cell='kc'))
        case = dict(args=['-c', 'pass'], dbm_rows=['user.db'], dbm_rows_cells=['bdb', 'tkrzw'])
        snapshot = b'KCSS\n\x00\xff'
        with patch.object(tool_diff, 'logical_dbm_rows') as logical:
            result = tool_diff.observation(ctx, Path(sys.executable), case, self.root / 'run',
                {'user.db': snapshot}, b'', time.monotonic() + 5)
            logical.assert_not_called()
        self.assertEqual(result['files']['user.db'], 'hex:' + snapshot.hex())
        changed = dict(result, files={'user.db': 'hex:' + snapshot[:-1].hex()})
        self.assertEqual(tool_diff.verdict(case, result, changed, self.root, 'kc')[0], 'DIFFER')
        ctx.settings.cell = 'bdb'
        with patch.object(tool_diff, 'logical_dbm_rows') as logical:
            tool_diff.observation(ctx, Path(sys.executable), case, self.root / 'bdb',
                {'user.db': b'container'}, b'', time.monotonic() + 5)
            logical.assert_called_once()

    def test_dbm_row_cells_reject_unknown_cells(self):
        path = self.root / 'case.json'
        for value in ('kc', ['other'], [1]):
            path.write_text(json.dumps(dict(schema=1, cases=[dict(
                id='snapshot', utility='test_pinyin', source='ngram_kyotodb.cpp:82-101',
                dbm_rows=['user.db'], dbm_rows_cells=value)])))
            with self.assertRaisesRegex(ValueError, 'dbm_rows_cells'):
                tool_diff.load_cases([path])

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

    def test_policy_cells_limit_the_record_to_the_cells_they_name(self):
        case = dict(id='bdb-only', policy_record='absent.md', policy_cells=['bdb'])
        side = dict(exit=0, stdout='', stderr='', files={})
        self.assertEqual(tool_diff.verdict(case, side, side, self.root, 'kc')[0], 'MATCH')
        self.assertEqual(tool_diff.verdict(case, side, dict(side, stderr='x'), self.root, 'tkrzw')[0], 'DIFFER')
        # On a named cell the record decides; this one is not registered.
        self.assertEqual(tool_diff.verdict(case, side, side, self.root, 'bdb')[0], 'INVALID')

    def class_b_record(self, case):
        record = dict(schema=contract_policy.SCHEMA, record='compatibility-policy',
                      status='current', disposition='POLICY-ACCEPTED', cases={case['id']:
                          dict(classification='b', pin_not_run=contract_policy.side_entry(case['pin_not_run']))})
        path = self.root / 'compatibility-policy.md'
        path.write_text('# Registered class (b)\n\n```contract-diff-policy\n'
                        + json.dumps(record) + '\n```\n')
        return path, record

    def test_registered_pin_not_run_binds_every_subject_byte_and_never_launches_pin(self):
        expected = dict(exit=0, stdout='source-defined\n', stderr='', files={'out': 'hex:00'})
        case = dict(id='safe', utility='test_table_info', policy_record='compatibility-policy.md',
                    pin_not_run=expected)
        self.class_b_record(case)
        self.assertEqual(tool_diff.not_run_expected(case, self.root), expected)
        with patch.object(tool_diff, 'observation', return_value=expected) as observe:
            pin, subject = tool_diff.observe_sides(None, (Path('PIN-MUST-NOT-RUN'), Path('subject')),
                case, self.root, {}, b'', 0, None)
        observe.assert_called_once()
        self.assertEqual(observe.call_args.args[1], Path('subject'))
        self.assertIsNone(pin)
        self.assertEqual(tool_diff.verdict(case, pin, subject, self.root, 'tkrzw')[0], 'POLICY-ACCEPTED')
        for changed in (dict(expected, exit=2), dict(expected, stdout=''),
                        dict(expected, stderr='unexpected'), dict(expected, files={'out': 'hex:01'})):
            self.assertEqual(tool_diff.verdict(case, None, changed, self.root, 'tkrzw')[0], 'DIFFER')
        self.assertEqual(tool_diff.verdict(case, expected, expected, self.root, 'tkrzw')[0], 'INVALID')
        self.assertEqual(tool_diff.verdict(case, None, dict(expected, exit=-6), self.root, 'tkrzw')[0], 'INVALID')

    def test_unregistered_pin_not_run_and_changed_declarations_are_refused(self):
        expected = dict(exit=0, stdout='', stderr='', files={})
        case = dict(id='safe', utility='test_table_info', policy_record='compatibility-policy.md',
                    pin_not_run=expected)
        self.class_b_record(case)
        for changed in (dict(case, id='unregistered'), dict(case, policy_record='elsewhere.md'),
                        dict(case, pin_not_run=dict(expected, stdout='invented'))):
            self.assertEqual(tool_diff.verdict(changed, None, expected, self.root, 'tkrzw')[0], 'INVALID')
        path, record = self.class_b_record(case)
        record['cases']['safe']['classification'] = 'a'
        path.write_text('```contract-diff-policy\n' + json.dumps(record) + '\n```\n')
        self.assertEqual(tool_diff.verdict(case, None, expected, self.root, 'tkrzw')[0], 'INVALID')

    def test_pin_not_run_does_not_admit_parity_records_or_pin_captures(self):
        expected = dict(exit=0, stdout='', stderr='', files={})
        with self.assertRaisesRegex(ValueError, 'difference, not parity'):
            contract_policy.entry(expected, expected)
        pair = dict(pin=contract_policy.side_entry(expected), subject=contract_policy.side_entry(expected),
                    divergence=['claimed exception'])
        with self.assertRaisesRegex(ValueError, 'difference, not parity'):
            contract_policy.check_entry(pair)
        case = dict(id='safe', policy_record='compatibility-policy.md', pin_not_run=expected)
        path, record = self.class_b_record(case)
        record['cases']['safe'] = pair
        path.write_text('```contract-diff-policy\n' + json.dumps(record) + '\n```\n')
        self.assertEqual(tool_diff.verdict(case, None, expected, self.root, 'tkrzw')[0], 'INVALID')

    def test_pin_not_run_rejects_incomplete_or_masked_declarations(self):
        expected = dict(exit=0, stdout='', stderr='', files={})
        case = dict(id='safe', utility='test_table_info', source='table_info.cpp:406',
                    policy_record='compatibility-policy.md', pin_not_run=expected)
        path = self.root / 'case.json'
        for changed in (dict(case, pin_not_run=True), dict(case, pin_not_run={}),
                        dict(case, pin_signal=-6), dict(case, dbm_rows=['out']),
                        dict(case, expected_failure={'exit': 2, 'stderr': 'failed'}),
                        dict(case, policy_record='elsewhere.md')):
            path.write_text(json.dumps(dict(schema=1, cases=[changed])))
            with self.assertRaises(ValueError):
                tool_diff.load_cases([path])

    def test_policy_record_is_not_applied_outside_its_named_cells(self):
        case = dict(id='abort', policy_record='record.md', policy_cells=['bdb'],
                    pin_signal=-6, reproduces_signal=True)
        pin = dict(exit=-6, stdout='', stderr='assertion\n', files={})
        subject = dict(pin, stderr='')
        row = dict(runner='tool-diff', case='abort', cell='bdb', userdir=False, abort=False,
                   pin_signal=-6, reproduces_signal=True, database_free=False, pin=pin, subject=subject)
        self.database_free_record(row)
        self.assertEqual(tool_diff.verdict(case, pin, subject, self.root, 'bdb')[0], 'POLICY-ACCEPTED')
        self.assertEqual(tool_diff.verdict(case, pin, subject, self.root, 'kc')[0], 'DIFFER')

    def test_a_cell_without_an_applicable_record_compares_as_plain_parity(self):
        case = dict(id='empty', policy_record='absent.md', policy_cells=['kc', 'tkrzw'],
                    pin_signal=-6, reproduces_signal=True)
        side = dict(exit=0, stdout='complete\n', stderr='', files={'out': 'hex:01'})
        self.assertEqual(tool_diff.verdict(case, side, side, self.root, 'bdb')[0], 'MATCH')
        self.assertEqual(tool_diff.verdict(case, side, dict(side, stdout=''), self.root, 'bdb')[0], 'DIFFER')
        self.assertEqual(tool_diff.verdict(case, side, side, self.root, 'kc')[0], 'INVALID')

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
        for bad, message in ((dict(utility='gen_k_mixture_model_unregistered'), 'no registered pin build directory'),
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
