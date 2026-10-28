#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the facade totals and counts cases (#1175)."""
import unittest

import contract_facade_totals_counts as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('import-wrapped-facade-total-guess', (), {'issues': ('798',)}),
            ('import-wrapped-facade-total-segment', (), {'issues': ('798',)}),
            ('import-facade-total-below-wrap', (), {'control': True, 'issues': ('798',)}),
            ('stored-header-facade-total', (), {}),
            ('stored-header-facade-total-small', (), {'control': True}),
            ('system-add-reload-facade-total', (), {}),
            ('system-add-reload-facade-total-small', (), {'control': True}),
            ('token-add-unigram-numerator-guess', (), {'issues': ('801',)}),
            ('longer-choices-numerator-guess', (), {'issues': ('801',)}),
            ('unload-drops-accepted-bump', (), {'issues': ('801',)})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class FacadeTotalsCountsTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

    def test_every_case_function_belongs_to_this_module(self):
        for name, _, _, fn in declarations():
            while hasattr(fn, "func"):
                fn = fn.func
            with self.subTest(name=name):
                self.assertIn(fn.__module__, (family.__name__, 'contract_case_helpers'))

    def test_replay_registers_the_same_functions(self):
        first, second = declarations(), declarations()
        self.assertEqual([c[0] for c in first], [c[0] for c in second])
        for (name, _, _, a), (_, _, _, b) in zip(first, second):
            with self.subTest(name=name):
                self.assertIs(a, b)


if __name__ == '__main__':
    unittest.main()
