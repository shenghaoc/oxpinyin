#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for contract-diff's case registration (#1175).

The registry is a parameter, so these register into a fresh dict: no driver,
no 4,000-case registration. contract-diff.test.py keeps the checks that go
through the driver's CASES and its `case` decorator."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_registry as registry
from contract_verdict import NO_ABORT


def case(k):
    return {}


class CaseSpecTests(unittest.TestCase):
    def test_defaults(self):
        self.assertEqual(registry.case_spec(), dict(
            fn=None, mode='pinyin', control=False, abort=NO_ABORT, stderr=False, crash=None, userdir=False,
            cells=('bdb', 'kc', 'tkrzw'), either=False, host_scanf=(), dir_chunk=None, stderr_varies=False,
            abort_logs=None, fatal=False, both_abort=False, strjoinv_over_read=False, issues=(),
            policy_record=None, case_id=None, pin_not_run=None))

    def test_crash_enables_stderr_and_sequences_become_tuples(self):
        spec = registry.case_spec(crash={'ret': 0}, host_scanf=['1e'], issues=['#532', ' Row 54', 'W11'])
        self.assertIs(spec['stderr'], True)
        self.assertEqual(spec['host_scanf'], ('1e',))
        self.assertEqual(spec['issues'], ('532', 'row:54', 'w11'))

    def test_positional_options_keep_their_order(self):
        spec = registry.case_spec('zhuyin', True)
        self.assertEqual((spec['mode'], spec['control']), ('zhuyin', True))


class RegisterTests(unittest.TestCase):
    def test_stores_the_spec_under_its_id_in_registration_order_and_returns_the_function(self):
        cases = {}
        self.assertIs(registry.register(cases, 'first', case, issues=('532',)), case)
        self.assertIs(registry.register(cases, 'second', case, 'zhuyin'), case)
        self.assertEqual(list(cases), ['first', 'second'])
        self.assertEqual(cases['first'], registry.case_spec(fn=case, case_id='first', issues=('532',)))
        self.assertEqual(cases['second']['mode'], 'zhuyin')

    def test_a_duplicate_id_is_rejected_and_leaves_the_first_spec(self):
        cases = {}
        registry.register(cases, 'same', case)
        original = cases['same']
        with self.assertRaisesRegex(ValueError, '^duplicate case name: same$'):
            registry.register(cases, 'same', case, mode='zhuyin')
        self.assertIs(cases['same'], original)

    def test_invalid_option_combinations_assert_with_the_id_and_store_nothing(self):
        invalid = (dict(either=True), dict(either=True, abort=0), dict(both_abort=True),
                   dict(dir_chunk='other', abort=0), dict(dir_chunk='user'),
                   dict(pin_not_run={}, stderr=True), dict(pin_not_run={}, policy_record='r.md'),
                   dict(policy_record='r.md', host_scanf=('1e',)), dict(policy_record='r.md', userdir=True),
                   dict(policy_record='r.md', crash={}))
        for options in invalid:
            with self.subTest(options=options):
                cases = {}
                with self.assertRaises(AssertionError) as raised:
                    registry.register(cases, 'bad', case, **options)
                self.assertEqual(raised.exception.args, ('bad',))
                self.assertEqual(cases, {})

    def test_valid_exception_combinations_register(self):
        valid = (dict(either=True, abort=0, crash={}), dict(fatal=True, both_abort=True),
                 dict(dir_chunk='system', abort=0), dict(pin_not_run={'ret': 0}),
                 dict(policy_record='r.md', stderr=True, control=True))
        for options in valid:
            with self.subTest(options=options):
                cases = {}
                registry.register(cases, 'ok', case, **options)
                self.assertEqual(list(cases), ['ok'])


if __name__ == '__main__':
    unittest.main()
