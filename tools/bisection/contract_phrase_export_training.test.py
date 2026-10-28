#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the phrase export and training cases (#1175)."""
import unittest

import contract_phrase_export_training as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue790-export-after-train-system', (), {'issues': ('790',)}),
            ('issue790-export-no-train-system', (), {'control': True, 'issues': ('790',)}),
            ('issue790-export-unconstrained-system', (), {'control': True, 'issues': ('790',)}),
            ('issue790-export-open-before-system', (), {'control': False, 'issues': ('790',)}),
            ('issue790-export-save-reopen-system', (), {'control': True, 'issues': ('790',)}),
            ('issue790-export-replayed-train-system', (), {'control': False, 'issues': ('790',)}),
            ('issue790-export-zero-and-signed-system', (), {'issues': ('790',)}),
            ('issue790-export-prefix-overflow-system', (), {'issues': ('790',)}),
            ('issue790-export-override-only-system', (), {'issues': ('790',)}),
            ('issue790-export-appended-system', (), {'issues': ('790',)}),
            ('issue790-guess-after-train-system', (), {'issues': ('790',)}),
            ('issue790-character-offset-after-train-system', (), {'issues': ('790',)}),
            ('issue759-export-open-across-import-system', (), {'issues': ('759',)}),
            ('issue759-export-open-across-import-user', (), {'issues': ('759',)})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class PhraseExportTrainingTests(unittest.TestCase):
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
