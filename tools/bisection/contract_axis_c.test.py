#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for contract-diff's axis C battery (#1175).

The battery takes the registry, the worker and the registration's tables as
parameters, so these drive it with synthetic ones: no driver, no library. The
module's clock is replaced, so the suspend checks run on hosts without
CLOCK_BOOTTIME. contract-diff.test.py keeps the checks through the driver."""

import contextlib
from argparse import Namespace
import io
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import contract_axis_c as axis_c

CLOCK = SimpleNamespace(CLOCK_BOOTTIME=1, CLOCK_MONOTONIC=2, clock_gettime=lambda clock: 0.0)


def live(result, stderr=''):
    return dict(exit=0, result=result, stderr=stderr)


def refused(mode='pinyin', **extra):
    return live(dict(set_scheme=False, observation=None, logs=[[axis_c.WARNING_DOMAIN[mode], 16]],
                     log_messages=['refused'], **extra))


ABORTED = dict(exit=-signal.SIGABRT, result=None, stderr='abort')


class DefinitionTests(unittest.TestCase):
    def test_the_committed_definition_is_read_from_the_repository_root(self):
        definition = axis_c.axis_c_definition()
        self.assertEqual(definition['format'], 'axis-c-replacement-v1')
        self.assertEqual(definition, json.loads((HERE / 'axis-c-replacement.json').read_text()))

    def test_verify_sources_hashes_each_committed_source(self):
        self.assertEqual(axis_c.axis_c_definition(verify_sources=True), axis_c.axis_c_definition())


class ComparisonTests(unittest.TestCase):
    def test_canonical_bytes_are_compact_sorted_ascii(self):
        self.assertEqual(axis_c.axis_c_canonical({'b': 1, 'a': ['你']}), b'{"a":["\\u4f60"],"b":1}')

    def test_only_diagnostic_fields_are_normalized(self):
        value = dict(stderr='/s/user-ab1 failed', reopen_stderr='sys-\udcffq7', log_messages=['/s/x user-z'],
                     result=dict(payload='/s/user-ab1', logs=['/s']))
        self.assertEqual(axis_c.axis_c_normalize_diagnostics(value, '/s'), dict(
            stderr='SCRATCH/user-X failed', reopen_stderr='sys-\udcffX', log_messages=['SCRATCH/x user-X'],
            result=dict(payload='/s/user-ab1', logs=['/s'])))

    def test_a_reproduced_abort_needs_two_identical_bare_aborts(self):
        self.assertTrue(axis_c.axis_c_reproduced_abort(ABORTED, dict(ABORTED)))
        rejected = ((ABORTED, dict(ABORTED, stderr='other')), (ABORTED, refused()),
                    (dict(ABORTED, exit=-signal.SIGSEGV), dict(ABORTED, exit=-signal.SIGSEGV)),
                    (dict(ABORTED, extra=1), dict(ABORTED, extra=1)), (None, None))
        for pin, subject in rejected:
            with self.subTest(pin=pin, subject=subject):
                self.assertFalse(axis_c.axis_c_reproduced_abort(pin, subject))

    def test_covered_abort_accepts_only_the_registered_shapes(self):
        self.assertTrue(axis_c.axis_c_covered_abort(ABORTED, refused('zhuyin'), 'zhuyin'))
        # The pinyin facade reproduces the abort; its refusal is no longer licensed.
        self.assertFalse(axis_c.axis_c_covered_abort(ABORTED, refused('pinyin'), 'pinyin'))
        rejected = ((dict(ABORTED, exit=-signal.SIGSEGV), refused()), (dict(ABORTED, extra=1), refused()),
                    (dict(ABORTED, result={}), refused()), (ABORTED, refused(extra=1)),
                    (ABORTED, refused('zhuyin')), (ABORTED, dict(refused(), exit=1)), (None, refused()))
        for pin, subject in rejected:
            with self.subTest(pin=pin, subject=subject):
                self.assertFalse(axis_c.axis_c_covered_abort(pin, subject, 'pinyin'))

    def test_a_live_row_exited_cleanly_with_a_result(self):
        self.assertTrue(axis_c.axis_c_live(live({})))
        for row in (dict(exit=1, result={}), live(None), None, 'row'):
            with self.subTest(row=row):
                self.assertFalse(axis_c.axis_c_live(row))

    def test_the_residual_removes_only_licensed_fields_of_covered_rows_and_copies(self):
        pin = live(dict(observations={'a': dict(ABORTED, extra=1), 'b': live(1)}))
        subject = live(dict(observations={'a': refused(), 'b': live(2)}))
        left, right = axis_c.axis_c_covered_residual(pin, subject, ['a'])
        self.assertEqual(left['result']['observations'], {'a': {'extra': 1}, 'b': live(1)})
        self.assertEqual(right['result']['observations'], {'a': {}, 'b': live(2)})
        self.assertIn('exit', pin['result']['observations']['a'])
        self.assertEqual(axis_c.axis_c_licensed_remnant(dict(ABORTED, more=2)), {'more': 2})

    def test_the_first_difference_follows_sorted_keys_then_list_order(self):
        self.assertEqual(axis_c.axis_c_first_difference({'b': [1, {'c': 2}], 'a': 1}, {'b': [1, {'c': 3}], 'a': 1}),
                         dict(field='.b[1].c', pin=2, subject=3))
        self.assertEqual(axis_c.axis_c_first_difference([1], [1, 2]), dict(field='', pin=[1], subject=[1, 2]))
        long = axis_c.axis_c_first_difference('x' * 400, 'y')
        self.assertEqual((long['pin'], long['subject']), (repr('x' * 400)[:300], 'y'))


class SnapshotTests(unittest.TestCase):
    def run_check(self, worker, caller=None):
        with tempfile.TemporaryDirectory() as prefix, patch.object(axis_c, 'time', CLOCK), \
                patch.dict(os.environ, {} if caller is None else {'AXIS_C_ALLOCATE_EACH': caller}), \
                contextlib.redirect_stdout(io.StringIO()):
            if caller is None:
                os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
            args = Namespace(prefix=Path(prefix), pinyin_so=Path(prefix, 'subject.so'))
            try:
                return axis_c.check_axis_c_snapshots(args, run_worker=worker)
            finally:
                self.after = os.environ.get('AXIS_C_ALLOCATE_EACH')

    def test_each_pair_runs_allocated_then_snapshot_and_restores_the_caller_value(self):
        calls = []

        def worker(mode, so, data, name, scratch):
            calls.append((so.name, name, os.environ.get('AXIS_C_ALLOCATE_EACH')))
            return dict(exit=0, result={'name': name}, stderr='', userfiles={'varies': len(calls)})

        self.assertEqual(self.run_check(worker, caller='caller'), 0)
        self.assertEqual(self.after, 'caller')
        self.assertEqual(len(calls), 12)
        self.assertEqual([allocate for _, _, allocate in calls], ['1', None] * 6)
        self.assertEqual({so for so, _, _ in calls}, {'libpinyin.so', 'subject.so'})

    def test_a_snapshot_difference_fails_and_the_switch_is_still_cleared(self):
        def worker(mode, so, data, name, scratch):
            return dict(exit=0, result=os.environ.get('AXIS_C_ALLOCATE_EACH'), stderr='')

        with self.assertRaises(AssertionError) as raised:
            self.run_check(worker)
        self.assertEqual(raised.exception.args, ('axis-c-options-single-00000008',))
        self.assertIsNone(self.after)


class BatteryTests(unittest.TestCase):
    CASES = {'plain': dict(mode='pinyin'), 'rows': dict(mode='pinyin'), 'covered': dict(mode='zhuyin'),
             'gone': dict(mode='pinyin'), 'reproduced': dict(mode='pinyin')}

    def battery(self, answers, names, mutation=None, controls=()):
        """Run the battery with each (side, name) answered from `answers`; return status and output rows."""
        calls = []
        with tempfile.TemporaryDirectory() as prefix:
            root = Path(prefix)
            for name in ('lib/libpinyin.so', 'lib/libzhuyin.so', 'subject.so', 'zsubject.so', 'mutant.so',
                         'zmutant.so'):
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).touch()
            side = {(root / name).resolve(): kind for name, kind in (
                ('lib/libpinyin.so', 'pin'), ('lib/libzhuyin.so', 'pin'), ('subject.so', 'subject'),
                ('zsubject.so', 'subject'), ('mutant.so', 'mutant'), ('zmutant.so', 'mutant'))}

            def worker(mode, so, data, name, scratch, extra_env):
                calls.append((name, mode, side[so], side[Path(extra_env['AXIS_C_COMPANION_PINYIN'])]))
                self.assertEqual(data, root / 'lib/libpinyin/data')
                return dict(answers[side[so], name], userfiles={'ignored': len(calls)})

            args = Namespace(prefix=root, pinyin_so=root / 'subject.so', zhuyin_so=root / 'zsubject.so',
                             axis_c_mutation=mutation, axis_c_replacement=None if mutation else 'all',
                             axis_c_mutant_pinyin_so=root / 'mutant.so', axis_c_mutant_zhuyin_so=root / 'zmutant.so')
            output = io.StringIO()
            with patch.object(axis_c, 'time', CLOCK), contextlib.redirect_stdout(output):
                status = axis_c.run_axis_c(
                    args, names, CASES=self.CASES, run_worker=worker,
                    AXIS_C_LAYOUT_COVERED={'covered': '5c', 'reproduced': '5c'},
                    AXIS_C_DEFINITION={'mutations': {'C-m1-r': {'controls': list(controls)}}})
        rows = [json.loads(line) for line in output.getvalue().splitlines() if line.startswith('{')]
        return status, rows, calls

    def test_match_covered_and_differ_verdicts_counts_and_exit_status(self):
        observed = live(dict(observations={'a': live(1), 'b': ABORTED}))
        answers = {('pin', 'plain'): live({'v': 1}), ('subject', 'plain'): live({'v': 1}),
                   ('pin', 'rows'): live(dict(observations={'a': live(1), 'b': live(2)})),
                   ('subject', 'rows'): live(dict(observations={'a': live(1), 'b': live(3)})),
                   ('pin', 'covered'): observed,
                   ('subject', 'covered'): live(dict(observations={'a': live(1), 'b': refused('zhuyin')}))}
        status, rows, calls = self.battery(answers, ['plain', 'rows', 'covered'])
        self.assertEqual(status, 1)
        self.assertEqual(len(calls), 12)
        self.assertEqual({(mode, kind, companion) for name, mode, kind, companion in calls if name == 'covered'},
                         {('zhuyin', 'pin', 'pin'), ('zhuyin', 'subject', 'subject')})
        self.assertEqual(rows[0], dict(case='rows', verdict='DIFFER',
                                       first=dict(field='.result.observations.b.result', pin=2, subject=3)))
        self.assertEqual(rows[1], dict(case='covered', verdict='COVERED', row='5c', covered_observations=1))
        summary = rows[-1]
        self.assertEqual(summary['counts'], dict(cases=3, diagnostics_match=3, observations=5, payload_match=3,
                                                 payload_covered=1, payload_differ=1, case_match=1, covered=1,
                                                 case_differ=1, covered_by_row={'5c': 1}))
        self.assertEqual((summary['battery'], summary['mutation'], summary['repeats']), ('all', None, 'identical'))
        self.assertEqual(rows[-2]['count'], 1)

    def test_a_covered_row_both_sides_abort_alike_matches(self):
        # Rows 5c/5d on the pinyin facade: the setter reproduces the pin's
        # abort(), and the refusal it used to answer is a difference.
        observed = live(dict(observations={'a': live(1), 'b': ABORTED}))
        status, rows, _ = self.battery({('pin', 'reproduced'): observed, ('subject', 'reproduced'): observed},
                                       ['reproduced'])
        self.assertEqual(status, 0)
        self.assertEqual(rows[-1]['counts'], dict(cases=1, diagnostics_match=1, observations=2, payload_match=1,
                                                  payload_reproduced_abort=1, case_match=1))
        refusing = live(dict(observations={'a': live(1), 'b': refused('pinyin')}))
        status, rows, _ = self.battery({('pin', 'reproduced'): observed, ('subject', 'reproduced'): refusing},
                                       ['reproduced'])
        self.assertEqual((status, rows[0]['verdict']), (1, 'DIFFER'))

    def test_identical_failed_rows_never_match(self):
        dead = dict(exit=1, result=None, stderr='same')
        status, rows, _ = self.battery({('pin', 'gone'): dead, ('subject', 'gone'): dead}, ['gone'])
        self.assertEqual((status, rows[0]['verdict']), (1, 'DIFFER'))

    def test_a_covered_case_with_a_residual_difference_differs(self):
        answers = {('pin', 'covered'): live(dict(observations={'b': ABORTED}), stderr='outer'),
                   ('subject', 'covered'): live(dict(observations={'b': refused('zhuyin')}), stderr='other')}
        status, rows, _ = self.battery(answers, ['covered'])
        self.assertEqual(status, 1)
        self.assertEqual(rows[0], dict(case='covered', verdict='DIFFER',
                                       first=dict(field='.stderr', pin='outer', subject='other')))

    def test_a_mutation_compares_the_pin_build_with_the_mutant_and_requires_every_control_to_differ(self):
        answers = {('subject', 'plain'): live({'v': 1}), ('mutant', 'plain'): live({'v': 2}),
                   ('subject', 'gone'): live({'v': 1}), ('mutant', 'gone'): live({'v': 1})}
        status, rows, calls = self.battery(answers, ['plain'], mutation='C-m1-r', controls=['plain'])
        self.assertEqual(status, 0)
        self.assertEqual({(kind, companion) for _, _, kind, companion in calls},
                         {('subject', 'subject'), ('mutant', 'mutant')})
        self.assertEqual(rows[-1], dict(mutation='C-m1-r', control='plain', verdict='DIFFER'))
        with self.assertRaisesRegex(AssertionError, 'control.*not detected: gone'):
            self.battery(answers, ['gone'], mutation='C-m1-r', controls=['gone'])

    def test_an_unstable_repeat_is_not_established(self):
        # Two workers may take these in any order; three equal answers and one
        # different one leave some side's repeat disagreeing whatever the order.
        answers = _Sequence([live({'v': 1}), live({'v': 1}), live({'v': 2}), live({'v': 1})])
        with self.assertRaisesRegex(AssertionError, 'unstable case plain'):
            self.battery(answers, ['plain'])


class _Sequence:
    """Answer each worker call with the next value of a fixed sequence, whatever the case and side."""

    def __init__(self, sequence):
        self.sequence = iter(sequence)

    def __getitem__(self, key):
        return next(self.sequence)


if __name__ == '__main__':
    unittest.main()
