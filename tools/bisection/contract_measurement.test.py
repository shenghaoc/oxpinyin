#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Focused tests for the per-call measurement module (#1175)."""

import contextlib
import ctypes as C
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from argparse import Namespace
from types import SimpleNamespace
from unittest import TestCase, main
from unittest.mock import MagicMock, Mock, patch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
for path in (str(ROOT), str(HERE)):
    if path not in sys.path:
        sys.path.insert(0, path)

import contract_measurement as measurement
from tools.dev import host


class KitTests(TestCase):
    def test_selected_calls_use_the_wrapper_only_during_a_step_and_reset_on_error(self):
        original = C.CFUNCTYPE(measurement.B, measurement.P, measurement.U,
                              C.POINTER(measurement.U))(lambda *_: True)

        class Base:
            def __init__(self, mode, so, data, scratch):
                self.arguments = (mode, so, data, scratch)

            def fn(self, name, res, *args):
                return original

        class Kit(measurement.PerCallKit, Base):
            pass

        native = Mock()
        native.pc_count.return_value = True
        kit = Kit('pinyin', 'so', 'data', 'scratch', native, ('count',))
        self.assertEqual(kit.arguments, ('pinyin', 'so', 'data', 'scratch'))
        bound = kit.fn('token_get_n_pronunciation', measurement.B)
        number = measurement.U(5)
        self.assertTrue(bound(None, 7, C.byref(number)))
        native.pc_count.assert_not_called()
        with self.assertRaisesRegex(RuntimeError, 'stop'):
            with kit.per_call_step():
                self.assertTrue(bound(None, 7, C.byref(number)))
                self.assertIs(kit.fn('other', measurement.B), original)
                raise RuntimeError('stop')
        self.assertFalse(kit.active)
        native.pc_start.assert_called_once_with()
        self.assertEqual(native.pc_count.call_count, 1)
        self.assertEqual(native.pc_count.call_args.args[0].value, C.cast(original, measurement.P).value)
        self.assertTrue(bound(None, 7, C.byref(number)))
        self.assertEqual(native.pc_count.call_count, 1)
        self.assertEqual(native.pc_stop.call_count, 2)


class WorkerTests(TestCase):
    def worker(self, name, operations, native, kit, registry):
        loader = Mock(return_value=native)
        output = io.StringIO()
        with patch.object(measurement.resource, 'setrlimit') as limit, \
                patch.dict(os.environ, TMPDIR='/sample'), contextlib.redirect_stdout(output):
            measurement.per_call_worker('pinyin', 'so', 'data', name, 'helper', 'ir', operations,
                                        PerCallKit=kit, Kit=object, CASES=registry, per_call_native=loader)
        limit.assert_called_once_with(measurement.resource.RLIMIT_CORE, (0, 0))
        loader.assert_called_once_with('helper', True)
        native.pc_stop.assert_called_once_with()
        return json.loads(output.getvalue())

    @staticmethod
    def native():
        native = Mock()
        native.pc_calls.side_effect = [3, 5, 7]
        native.pc_nanos.side_effect = [30, 50, 70]
        return native

    def test_calibration_selects_the_operation_and_reports_counters(self):
        native, kit = self.native(), Mock()
        result = self.worker('calibrate', 'count', native, kit, {})
        native.pc_calibrate.assert_called_once_with(1, 1000)
        kit.assert_not_called()
        self.assertEqual(result, {'observation': {'calibration': True},
                                  'stats': {'add': {'calls': 3, 'ns': 30},
                                            'count': {'calls': 5, 'ns': 50},
                                            'lookup': {'calls': 7, 'ns': 70}}})

    def test_registered_case_receives_the_measurement_kit_and_its_logs_are_reported(self):
        native, kit = self.native(), Mock()
        kit.return_value.logs = [['libpinyin', 16]]
        case = Mock(return_value={'ret': True})
        result = self.worker('example', 'add,count', native, kit, {'example': {'fn': case}})
        kit.assert_called_once_with('pinyin', 'so', 'data', '/sample', native, ['add', 'count'])
        case.assert_called_once_with(kit.return_value)
        self.assertEqual(result['observation'], {'ret': True, 'logs': [['libpinyin', 16]]})
        self.assertEqual(result['stats']['count'], {'calls': 5, 'ns': 50})
        native.pc_calibrate.assert_not_called()

    def test_lookup_only_uses_the_unwrapped_binding_and_frees_its_array(self):
        native = self.native()
        native.pc_lookup_loop.return_value = True
        tokens = (measurement.U * 2)(11, 22)
        array = measurement.Arr(C.cast(tokens, measurement.P), 2)
        create, free = Mock(return_value=C.addressof(array)), Mock()
        kit = Mock()
        kit.return_value.glib = SimpleNamespace(g_array_new=create, g_array_free=free)
        kit.return_value.inst = 123
        kit.return_value.logs = []
        original = C.CFUNCTYPE(measurement.B, measurement.P, measurement.S, measurement.P)(lambda *_: True)
        base = SimpleNamespace(fn=Mock(return_value=original))
        output = io.StringIO()
        with patch.object(measurement.resource, 'setrlimit'), patch.dict(os.environ, TMPDIR='/sample'), \
                contextlib.redirect_stdout(output):
            measurement.per_call_worker('pinyin', 'so', 'data', 'lookup-only', 'helper', 'wall', 'lookup',
                                        PerCallKit=kit, Kit=base, CASES={}, per_call_native=Mock(return_value=native))
        base.fn.assert_called_once_with(kit.return_value, 'lookup_tokens', measurement.B,
                                        measurement.P, measurement.S, measurement.P)
        arguments = native.pc_lookup_loop.call_args.args
        self.assertEqual((arguments[0].value, arguments[1], arguments[2], arguments[3], arguments[4]),
                         (C.cast(original, measurement.P).value, 123, '你好'.encode(), C.addressof(array), 10000))
        free.assert_called_once_with(C.addressof(array), True)
        self.assertEqual(json.loads(output.getvalue())['observation'], {'ret': True, 'tokens': [11, 22], 'logs': []})


class SampleTests(TestCase):
    def test_sample_uses_the_driver_worker_entry_and_cleans_its_private_directory(self):
        process = MagicMock()
        process.__enter__.return_value = process
        process.returncode = 0
        process.communicate.return_value = (b'{"observation":{"ret":true},"stats":{"add":{"calls":2,"ns":10}}}\n',
                                            b'diagnostic\n')
        runtime, cancelled, terminate = Mock(), Mock(), Mock()
        listing = Mock(return_value={'profile': 'contents'})
        ir, profile = Mock(), Mock()
        with tempfile.TemporaryDirectory() as scratch, \
                patch.dict(os.environ, CONTRACT_DIFF_CELL='tkrzw', OXPINYIN_RUNTIME_SIDE='pin'), \
                patch.object(measurement.subprocess, 'Popen', return_value=process) as spawn:
            result = measurement.per_call_run('pinyin', '/lib.so', '/data', 'example', '/helper', 'wall', 'add',
                                              scratch, 3, driver_file='/driver/contract-diff.py', CELL='bdb',
                                              check_tkrzw=runtime, check_cancelled=cancelled, terminate_group=terminate,
                                              user_dir_listing=listing, per_call_ir=ir, per_call_profile=profile)
            command = spawn.call_args.args[0]
            self.assertEqual(command, ['taskset', '-c', '3', sys.executable, '/driver/contract-diff.py',
                                       '--per-call-worker', 'pinyin', '/lib.so', '/data', 'example', '/helper',
                                       'wall', 'add'])
            child_env = spawn.call_args.kwargs['env']
            sample = child_env['TMPDIR']
            self.assertFalse(Path(sample).exists())
            runtime.assert_called_once_with('tkrzw', 'contract-diff.py/per_call', 'pin', '/lib.so', env=child_env)
            listing.assert_called_once_with(sample, set())
        self.assertEqual(result, {'observation': {'exit': 0, 'result': {'ret': True}, 'stderr': 'diagnostic\n',
                                                   'stderr_lines': 1, 'userfiles': {'profile': 'contents'}},
                                  'stats': {'add': {'calls': 2, 'ns': 10}}, 'ir': None, 'profile': None})
        terminate.assert_not_called()
        ir.assert_not_called()
        profile.assert_not_called()
        self.assertGreaterEqual(cancelled.call_count, 2)

    def test_samples_require_a_matching_verdict_and_equal_positive_counts(self):
        reference = {'observation': 'pin', 'stats': {'count': {'calls': 3}}}
        sample = {'observation': 'subject', 'stats': {'count': {'calls': 3}}}
        verdict = Mock(return_value=('MATCH', True, True))
        measurement.per_call_checked('spec', reference, sample, ('count',), verdict_for=verdict)
        verdict.assert_called_once_with('spec', 'pin', 'subject', False)
        for outcome, calls in ((('DIFFER', False, False), 3), (('MATCH', True, True), 0),
                               (('MATCH', True, True), 4)):
            with self.subTest(outcome=outcome, calls=calls), self.assertRaises(ValueError):
                measurement.per_call_checked('spec', reference,
                                             {'observation': 'subject', 'stats': {'count': {'calls': calls}}},
                                             ('count',), verdict_for=Mock(return_value=outcome))

    def test_calibration_and_profile_keep_exclusive_costs_separate_from_edges(self):
        self.assertEqual(measurement.per_call_calibration(24000), 21)
        for total in (3000, 24001):
            with self.subTest(total=total), self.assertRaises(ValueError):
                measurement.per_call_calibration(total)
        profile = measurement.per_call_profile('events: Ir\nfn=caller\n1 5\ncfn=target\ncalls=2 2\n1 9\n'
                                               'fn=target\n2 3\ntotals: 8\n')
        report = measurement.per_call_attribution(profile, 2, 0)
        self.assertEqual(report['raw_ir_per_call'], 4)
        self.assertEqual(report['exclusive_sum_ir_per_call'], 4)
        self.assertEqual(report['call_edges'][0]['inclusive_ir_per_call'], 4.5)
        self.assertTrue(report['inclusive_edges_overlap'])
        with self.assertRaises(ValueError):
            measurement.per_call_profile('events: Ir\nfn=caller\n1 7\ntotals: 8\n')


class ControllerTests(TestCase):
    def test_controller_rotates_three_arms_and_reports_calibrated_results(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            prefix = root / 'pin'
            (prefix / 'lib').mkdir(parents=True)
            pin, base, subject = prefix / 'lib/libpinyin.so', root / 'base.so', root / 'subject.so'
            for path in (pin, base, subject):
                path.write_bytes(path.name.encode())
            args = Namespace(cases=None, issue=[], family=[], expect_parent=False, observations=None, plan_only=False,
                             regenerate_sentence_ties=None, axis_c_replacement=None, axis_c_mutation=None,
                             axis_c_snapshot_check=False, per_call='wrap', per_call_facades='pinyin', per_call_cpu=4,
                             prefix=prefix, per_call_operations=None, per_call_attribution=False,
                             per_call_attribution_limit=25, pinyin_so=subject, zhuyin_so=None,
                             base_pinyin_so=base, base_zhuyin_so=None)
            calls = []

            def run(mode, so, data, name, helper, metric, operations, sample_dir, cpu, **kwargs):
                calls.append((so.name, metric, name, operations, cpu))
                stats = {op: {'calls': 7, 'ns': 70} for op in measurement.PER_CALL_OPS}
                if name == 'calibrate':
                    stats[operations]['calls'] = 1000
                return {'observation': {'result': {'ret': True}}, 'stats': stats,
                        'ir': 24000 if name == 'calibrate' else 3500}

            checked, check_arms = Mock(), Mock()
            stdout = io.StringIO()
            with patch.object(host, 'allowed_cpus', return_value={4, 5}), \
                    contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
                status = measurement.run_per_call(args, None, CASES={'wrap-256th-reading': {'mode': 'pinyin'}},
                                                  per_call_helper=lambda _: Path('/helper.so'), per_call_run=run,
                                                  per_call_checked=checked,
                                                  per_call_calibration=measurement.per_call_calibration,
                                                  per_call_attribution=measurement.per_call_attribution,
                                                  observed=lambda value: value)
            self.assertEqual(status, 0)
            self.assertEqual([name for name, metric, _, _, _ in calls if metric == 'wall'],
                             ['libpinyin.so', 'base.so', 'subject.so', 'base.so', 'subject.so', 'libpinyin.so',
                              'subject.so', 'libpinyin.so', 'base.so'])
            self.assertEqual([(name, operation) for name, metric, case, operation, _ in calls
                              if metric == 'ir' and case != 'calibrate'],
                             [(side, op) for side in ('libpinyin.so', 'base.so', 'subject.so')
                              for op in ('add', 'count', 'lookup')])
            self.assertEqual(checked.call_count, 18)
            rows = [json.loads(line) for line in stdout.getvalue().splitlines()]
            results = [row for row in rows if row['per_call'] == 'result']
            self.assertEqual(len(results), 9)
            self.assertTrue(all(row['wall_ns_per_call'] == 10 and row['ir_per_call'] == 479 for row in results))
            self.assertTrue(all(row['wall_ratio'] == 1 and row['ir_ratio'] == 1 for row in results))


if __name__ == '__main__':
    main()
